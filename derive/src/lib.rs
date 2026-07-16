use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, LitStr, Result, parse_macro_input, parse_quote};

/// Exposes explicitly opted-in named fields on a Bevy resource to
/// `chill_bevy_console`'s `get` and `res` commands.
///
/// Use `#[console_resource(prefix = "...")]` on the resource and
/// `#[console(...)]` on each field to expose. Field options are `name`, `help`,
/// and `readonly`.
#[proc_macro_derive(ConsoleResource, attributes(console_resource, console))]
pub fn derive_console_resource(input: TokenStream) -> TokenStream {
    match derive_console_resource_impl(parse_macro_input!(input as DeriveInput)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn derive_console_resource_impl(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    let resource_options = parse_resource_options(&input.attrs)?;
    let console_crate: syn::Path = parse_quote!(::chill_bevy_console);
    let bevy_crate: syn::Path = parse_quote!(::bevy);
    let ident = input.ident;
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            input.generics,
            "ConsoleResource does not yet support generic resources",
        ));
    }
    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            _ => {
                return Err(Error::new_spanned(
                    ident,
                    "ConsoleResource only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                ident,
                "ConsoleResource can only be derived for a struct",
            ));
        }
    };

    let mut accessors = Vec::new();
    let mut descriptors = Vec::new();
    let mut errors: Option<Error> = None;

    for field in fields {
        match parse_field(&field.attrs, &field.ident) {
            Ok(Some(options)) => {
                let field_ident = field.ident.expect("named fields always have an identifier");
                let field_type = field.ty;
                let default_name = match &resource_options.prefix {
                    Some(prefix) => format!("{prefix}.{}", field_ident),
                    None => field_ident.to_string(),
                };
                let name = options.name.unwrap_or(default_name);
                let help = options.help.unwrap_or_default();
                let getter = format_ident!("__chill_console_get_{field_ident}");
                let setter = format_ident!("__chill_console_set_{field_ident}");
                let adjuster = format_ident!("__chill_console_adjust_{field_ident}");
                let writable = !options.readonly;

                let setter_impl = writable.then(|| {
                    quote! {
                        fn #setter(
                            world: &mut #bevy_crate::prelude::World,
                            input: &str,
                        ) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                            let value = <#field_type as #console_crate::ConsolePropertyValue>::parse_console_value(input)?;
                            let mut resource = world
                                .get_resource_mut::<Self>()
                                .ok_or_else(|| format!("Resource `{}` is not inserted", ::std::any::type_name::<Self>()))?;
                            resource.#field_ident = value;
                            Ok(#console_crate::ConsolePropertyValue::format_console_value(&resource.#field_ident))
                        }

                        fn #adjuster(
                            world: &mut #bevy_crate::prelude::World,
                            amount: &str,
                            subtract: bool,
                        ) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                            let value = {
                                let resource = world
                                    .get_resource::<Self>()
                                    .ok_or_else(|| format!("Resource `{}` is not inserted", ::std::any::type_name::<Self>()))?;
                                #console_crate::ConsolePropertyValue::adjusted_console_value(
                                    &resource.#field_ident,
                                    amount,
                                    subtract,
                                )?
                            };
                            let mut resource = world
                                .get_resource_mut::<Self>()
                                .ok_or_else(|| format!("Resource `{}` is not inserted", ::std::any::type_name::<Self>()))?;
                            resource.#field_ident = value;
                            Ok(#console_crate::ConsolePropertyValue::format_console_value(&resource.#field_ident))
                        }
                    }
                });
                let setter_descriptor = if writable {
                    quote!(Some(Self::#setter))
                } else {
                    quote!(None)
                };
                let adjuster_descriptor = if writable {
                    quote!(Some(Self::#adjuster))
                } else {
                    quote!(None)
                };

                accessors.push(quote! {
                    fn #getter(
                        world: &#bevy_crate::prelude::World,
                    ) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                        let resource = world
                            .get_resource::<Self>()
                            .ok_or_else(|| format!("Resource `{}` is not inserted", ::std::any::type_name::<Self>()))?;
                        Ok(#console_crate::ConsolePropertyValue::format_console_value(&resource.#field_ident))
                    }

                    #setter_impl
                });
                descriptors.push(quote! {
                    #console_crate::ConsoleProperty::new(
                        #name,
                        #help,
                        Self::#getter,
                        #setter_descriptor,
                        #adjuster_descriptor,
                        <#field_type as #console_crate::ConsolePropertyValue>::IS_BOOLEAN,
                        <#field_type as #console_crate::ConsolePropertyValue>::IS_NUMERIC,
                    )
                });
            }
            Ok(None) => {}
            Err(error) => {
                if let Some(combined) = &mut errors {
                    combined.combine(error);
                } else {
                    errors = Some(error);
                }
            }
        }
    }
    if let Some(error) = errors {
        return Err(error);
    }
    if descriptors.is_empty() {
        return Err(Error::new_spanned(
            ident,
            "ConsoleResource requires at least one field marked #[console]",
        ));
    }

    Ok(quote! {
        impl #ident {
            #(#accessors)*

            const __CHILL_CONSOLE_PROPERTIES: &'static [#console_crate::ConsoleProperty] = &[
                #(#descriptors),*
            ];
        }

        impl #console_crate::ConsoleResource for #ident {
            fn console_properties() -> &'static [#console_crate::ConsoleProperty] {
                Self::__CHILL_CONSOLE_PROPERTIES
            }
        }
    })
}

#[derive(Default)]
struct ResourceOptions {
    prefix: Option<String>,
}

fn parse_resource_options(attributes: &[syn::Attribute]) -> Result<ResourceOptions> {
    let mut options = ResourceOptions::default();
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("console_resource"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("prefix") {
                let value: LitStr = meta.value()?.parse()?;
                if options.prefix.replace(value.value()).is_some() {
                    return Err(meta.error("prefix may only be specified once"));
                }
                Ok(())
            } else {
                Err(meta.error("unsupported console_resource option; expected prefix"))
            }
        })?;
    }
    Ok(options)
}

#[derive(Default)]
struct FieldOptions {
    name: Option<String>,
    help: Option<String>,
    readonly: bool,
}

fn parse_field(
    attributes: &[syn::Attribute],
    field: &Option<syn::Ident>,
) -> Result<Option<FieldOptions>> {
    let mut options = None;
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("console"))
    {
        if options.is_some() {
            return Err(Error::new_spanned(
                attribute,
                "only one #[console] attribute is allowed per field",
            ));
        }
        let mut parsed = FieldOptions::default();
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let value: LitStr = meta.value()?.parse()?;
                parsed.name = Some(value.value());
                Ok(())
            } else if meta.path.is_ident("help") {
                let value: LitStr = meta.value()?.parse()?;
                parsed.help = Some(value.value());
                Ok(())
            } else if meta.path.is_ident("readonly") {
                parsed.readonly = true;
                Ok(())
            } else {
                Err(meta.error("unsupported console option; expected name, help, or readonly"))
            }
        })?;
        options = Some(parsed);
    }
    if field.is_none() {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "ConsoleResource only supports named fields",
        ));
    }
    Ok(options)
}
