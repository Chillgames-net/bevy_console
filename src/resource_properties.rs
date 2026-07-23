//! Opt-in console access to reflected Bevy resource fields.

use crate::model::CommandSpec;
use crate::{
    Args, ArgumentSpec, BuiltinCommand, CompletionItem, CompletionRequest,
    ConsoleCompletionRequest, ConsoleRegistry, ConsoleResult, completion::static_completion_items,
};
use bevy::ecs::{component::ComponentId, reflect::AppTypeRegistry};
use bevy::prelude::*;
use bevy::reflect::{
    FromType, GetTypeRegistration, PartialReflect, ReflectMut, ReflectRef, TypeInfo, TypePath,
    Typed,
};
use std::{any::TypeId, collections::BTreeMap};

/// Optional console metadata for a reflected resource field.
///
/// Fields with a registered [`ConsolePropertyValue`] adapter are exposed by
/// default. Attach this through Bevy's custom reflection attributes only when
/// a field needs an override:
///
/// ```
/// # use bevy::prelude::*;
/// # use chill_bevy_console::ConsoleProperty;
/// #[derive(Resource, Reflect)]
/// #[reflect(Resource)]
/// struct Settings {
///     #[reflect(@ConsoleProperty::readonly())]
///     build_label: String,
/// }
/// ```
#[derive(Clone, Default, Reflect)]
#[reflect(opaque)]
pub struct ConsoleProperty {
    name: Option<String>,
    help: Option<String>,
    readonly: bool,
}

impl ConsoleProperty {
    /// Creates field metadata without any overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates metadata that prevents the field from being changed through the console.
    pub fn readonly() -> Self {
        Self {
            readonly: true,
            ..default()
        }
    }

    /// Overrides the field's segment of its console property name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Overrides the help inferred from the field's documentation comment.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Makes this field read-only.
    pub fn with_readonly(mut self) -> Self {
        self.readonly = true;
        self
    }
}

/// A reflected field type that can be read from and written through the console.
///
/// The built-in implementations cover booleans, all primitive integer and
/// float types, and [`String`]. For an application-specific reflected type,
/// implement this trait and call
/// [`ConsoleAppExt::register_console_property_value`](crate::ConsoleAppExt::register_console_property_value)
/// before registering a resource that contains it.
pub trait ConsolePropertyValue: Reflect + TypePath {
    #[doc(hidden)]
    const IS_BOOLEAN: bool = false;
    #[doc(hidden)]
    const IS_NUMERIC: bool = false;

    /// Parses a value entered through the console.
    fn parse_console_value(input: &str) -> Result<Self, String>
    where
        Self: Sized;

    /// Formats the value for console output.
    fn format_console_value(&self) -> String;

    /// Returns this value adjusted by the supplied amount.
    fn adjusted_console_value(&self, _amount: &str, _subtract: bool) -> Result<Self, String>
    where
        Self: Sized,
    {
        Err("not a numeric value".into())
    }
}

impl ConsolePropertyValue for bool {
    const IS_BOOLEAN: bool = true;

    fn parse_console_value(input: &str) -> Result<Self, String> {
        match input.to_ascii_lowercase().as_str() {
            "true" | "1" | "on" => Ok(true),
            "false" | "0" | "off" => Ok(false),
            _ => Err("expected a boolean (true/false)".into()),
        }
    }

    fn format_console_value(&self) -> String {
        self.to_string()
    }
}

macro_rules! integer_property_value {
    ($($type:ty => $description:literal),* $(,)?) => {
        $(
            impl ConsolePropertyValue for $type {
                const IS_NUMERIC: bool = true;

                fn parse_console_value(input: &str) -> Result<Self, String> {
                    input.parse().map_err(|_| format!("expected {}", $description))
                }

                fn format_console_value(&self) -> String {
                    self.to_string()
                }

                fn adjusted_console_value(&self, amount: &str, subtract: bool) -> Result<Self, String> {
                    let amount = amount.parse::<$type>().map_err(|_| format!("expected {}", $description))?;
                    if subtract {
                        self.checked_sub(amount)
                    } else {
                        self.checked_add(amount)
                    }
                    .ok_or_else(|| "result would overflow".to_string())
                }
            }
        )*
    };
}

integer_property_value!(
    i8 => "an integer", i16 => "an integer", i32 => "an integer", i64 => "an integer",
    i128 => "an integer", isize => "an integer",
    u8 => "an unsigned integer", u16 => "an unsigned integer", u32 => "an unsigned integer",
    u64 => "an unsigned integer", u128 => "an unsigned integer", usize => "an unsigned integer",
);

macro_rules! float_property_value {
    ($($type:ty),* $(,)?) => {
        $(
            impl ConsolePropertyValue for $type {
                const IS_NUMERIC: bool = true;

                fn parse_console_value(input: &str) -> Result<Self, String> {
                    input.parse().map_err(|_| "expected a number".to_string())
                }

                fn format_console_value(&self) -> String {
                    self.to_string()
                }

                fn adjusted_console_value(&self, amount: &str, subtract: bool) -> Result<Self, String> {
                    let amount = amount.parse::<$type>().map_err(|_| "expected a number".to_string())?;
                    let mut value = *self;
                    if subtract {
                        value -= amount;
                    } else {
                        value += amount;
                    }
                    Ok(value)
                }
            }
        )*
    };
}

float_property_value!(f32, f64);

impl ConsolePropertyValue for String {
    fn parse_console_value(input: &str) -> Result<Self, String> {
        Ok(input.into())
    }

    fn format_console_value(&self) -> String {
        self.clone()
    }
}

type ParseValue = fn(&str) -> Result<Box<dyn Reflect>, String>;
type FormatValue = fn(&dyn PartialReflect) -> Result<String, String>;
type AdjustValue = fn(&dyn PartialReflect, &str, bool) -> Result<Box<dyn Reflect>, String>;
type ReplaceValue = fn(&mut dyn PartialReflect, Box<dyn Reflect>) -> Result<String, String>;

/// Reflection type data that adapts a [`ConsolePropertyValue`] for dynamic access.
#[derive(Clone)]
struct ReflectConsolePropertyValue {
    parse: ParseValue,
    format: FormatValue,
    adjust: AdjustValue,
    replace: ReplaceValue,
    is_boolean: bool,
    is_numeric: bool,
}

impl<T: ConsolePropertyValue> FromType<T> for ReflectConsolePropertyValue {
    fn from_type() -> Self {
        Self {
            parse: |input| Ok(Box::new(T::parse_console_value(input)?)),
            format: |value| {
                value
                    .try_downcast_ref::<T>()
                    .map(ConsolePropertyValue::format_console_value)
                    .ok_or_else(|| "reflected property type does not match its adapter".into())
            },
            adjust: |value, amount, subtract| {
                let value = value.try_downcast_ref::<T>().ok_or_else(|| {
                    "reflected property type does not match its adapter".to_string()
                })?;
                Ok(Box::new(value.adjusted_console_value(amount, subtract)?))
            },
            replace: |field, value| {
                let field = field.try_downcast_mut::<T>().ok_or_else(|| {
                    "reflected property type does not match its adapter".to_string()
                })?;
                let value = value
                    .take::<T>()
                    .map_err(|_| "parsed property type does not match its adapter".to_string())?;
                *field = value;
                Ok(field.format_console_value())
            },
            is_boolean: T::IS_BOOLEAN,
            is_numeric: T::IS_NUMERIC,
        }
    }
}

#[derive(Clone)]
struct RegisteredConsoleProperty {
    name: String,
    resource_short_type_path: &'static str,
    console_field_name: String,
    help: String,
    resource_type_id: TypeId,
    resource_component_id: ComponentId,
    resource_type_path: &'static str,
    field_name: &'static str,
    value: ReflectConsolePropertyValue,
    readonly: bool,
}

/// Runtime registry for reflected fields exposed through the console.
#[derive(Resource, Default)]
pub(crate) struct ConsoleResources {
    properties: BTreeMap<String, RegisteredConsoleProperty>,
}

impl ConsoleResources {
    fn register(&mut self, properties: Vec<RegisteredConsoleProperty>) {
        assert!(
            !properties.is_empty(),
            "a console resource must contain at least one supported reflected field"
        );
        let resource_type_id = properties[0].resource_type_id;
        let resource_type_path = properties[0].resource_type_path;
        let resource_short_type_path = properties[0].resource_short_type_path;
        assert!(
            !self
                .properties
                .values()
                .any(|property| property.resource_type_id == resource_type_id),
            "console resource `{resource_type_path}` is already registered"
        );

        let short_path_collision = self.properties.values().any(|property| {
            property
                .resource_short_type_path
                .eq_ignore_ascii_case(resource_short_type_path)
        });
        if short_path_collision {
            let existing = std::mem::take(&mut self.properties);
            for mut property in existing.into_values() {
                if property
                    .resource_short_type_path
                    .eq_ignore_ascii_case(resource_short_type_path)
                {
                    property.use_full_type_path();
                }
                self.insert(property);
            }
        }

        for mut property in properties {
            if short_path_collision {
                property.use_full_type_path();
            }
            self.insert(property);
        }
    }

    fn insert(&mut self, property: RegisteredConsoleProperty) {
        let key = property.name.to_ascii_lowercase();
        assert!(!key.is_empty(), "console property names cannot be empty");
        assert!(
            !self.properties.contains_key(&key),
            "console property `{}` is already registered",
            property.name
        );
        self.properties.insert(key, property);
    }

    fn get(&self, name: &str) -> Option<RegisteredConsoleProperty> {
        self.properties.get(&name.to_ascii_lowercase()).cloned()
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &RegisteredConsoleProperty)> {
        self.properties
            .values()
            .map(|property| (property.name.as_str(), property))
    }
}

impl RegisteredConsoleProperty {
    fn use_full_type_path(&mut self) {
        self.name = format!("{}.{}", self.resource_type_path, self.console_field_name);
    }
}

/// Registers the resource-property built-in command and completion provider.
pub(crate) fn plugin(app: &mut App) {
    let enabled = app.world().resource::<crate::BuiltinCommands>().clone();
    app.init_resource::<ConsoleResources>();
    if enabled.contains(&BuiltinCommand::Res) {
        let completer = app.world_mut().register_system(complete_res);
        app.world_mut()
            .resource_mut::<ConsoleRegistry>()
            .register_exclusive_spec_with_completer(
                CommandSpec {
                    summary: "Inspect or modify a resource property",
                    args: vec![
                        ArgumentSpec::new("operation"),
                        ArgumentSpec::new("property").help("Property name"),
                        ArgumentSpec::new("value").help("Value or amount"),
                    ],
                    ..CommandSpec::new(
                        "res",
                        "res <get|set|add|sub|toggle> <property> [value] - inspect or modify a resource property",
                    )
                },
                res_property,
                completer,
            );
    }
}

pub(crate) fn register_property_value<T>(app: &mut App)
where
    T: ConsolePropertyValue + GetTypeRegistration,
{
    app.register_type::<T>()
        .register_type_data::<T, ReflectConsolePropertyValue>();
}

fn register_builtin_property_values(app: &mut App) {
    let already_registered = app
        .world()
        .resource::<AppTypeRegistry>()
        .read()
        .get_type_data::<ReflectConsolePropertyValue>(TypeId::of::<bool>())
        .is_some();
    if already_registered {
        return;
    }

    macro_rules! register {
        ($($type:ty),* $(,)?) => {
            $(register_property_value::<$type>(app);)*
        };
    }
    register!(
        bool, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, String,
    );
}

pub(crate) fn register_resource<R>(app: &mut App)
where
    R: Resource + Reflect + FromReflect + GetTypeRegistration + Typed,
{
    app.register_type::<R>();
    register_builtin_property_values(app);
    let resource_component_id = app.world_mut().register_component::<R>();

    let properties = {
        let registry = app.world().resource::<AppTypeRegistry>().read();
        let registration = registry
            .get(TypeId::of::<R>())
            .expect("the console resource type was just registered");
        assert!(
            registration
                .data::<bevy::ecs::reflect::ReflectResource>()
                .is_some(),
            "console resource `{}` must derive Reflect with #[reflect(Resource)]",
            R::type_path()
        );
        let TypeInfo::Struct(info) = R::type_info() else {
            panic!(
                "console resource `{}` must reflect as a struct with named fields",
                R::type_path()
            );
        };
        let resource_short_type_path = registration.type_info().type_path_table().short_path();

        info.iter()
            .filter_map(|field| {
                let value = registry
                    .get_type_data::<ReflectConsolePropertyValue>(field.type_id())?
                    .clone();
                let options = field.get_attribute::<ConsoleProperty>();
                let field_name = options
                    .and_then(|options| options.name.as_deref())
                    .unwrap_or_else(|| field.name());
                assert!(
                    !field_name.is_empty(),
                    "console property field names cannot be empty"
                );
                let name = format!("{resource_short_type_path}.{field_name}");
                let help = options
                    .and_then(|options| options.help.clone())
                    .or_else(|| field.docs().map(str::trim).map(str::to_owned))
                    .unwrap_or_default();
                Some(RegisteredConsoleProperty {
                    name,
                    resource_short_type_path,
                    console_field_name: field_name.to_owned(),
                    help,
                    resource_type_id: TypeId::of::<R>(),
                    resource_component_id,
                    resource_type_path: R::type_path(),
                    field_name: field.name(),
                    value,
                    readonly: options.is_some_and(|options| options.readonly),
                })
            })
            .collect::<Vec<_>>()
    };

    app.init_resource::<ConsoleResources>();
    app.world_mut()
        .resource_mut::<ConsoleResources>()
        .register(properties);
}

fn property(world: &World, name: &str) -> Result<RegisteredConsoleProperty, String> {
    world
        .get_resource::<ConsoleResources>()
        .and_then(|properties| properties.get(name))
        .ok_or_else(|| format!("Unknown property: {name}"))
}

fn reflected_field<'w>(
    world: &'w World,
    property: &RegisteredConsoleProperty,
) -> Result<&'w dyn PartialReflect, String> {
    let entity = world
        .resource_entities()
        .get(property.resource_component_id)
        .ok_or_else(|| format!("Resource `{}` is not inserted", property.resource_type_path))?;
    let resource = world
        .get_reflect(entity, property.resource_type_id)
        .map_err(|error| {
            format!(
                "Unable to reflect `{}`: {error}",
                property.resource_type_path
            )
        })?;
    let ReflectRef::Struct(resource) = resource.reflect_ref() else {
        return Err(format!(
            "Resource `{}` no longer reflects as a struct",
            property.resource_type_path
        ));
    };
    resource.field(property.field_name).ok_or_else(|| {
        format!(
            "Resource `{}` has no reflected field `{}`",
            property.resource_type_path, property.field_name
        )
    })
}

fn replace_value(
    world: &mut World,
    property: &RegisteredConsoleProperty,
    value: Box<dyn Reflect>,
) -> Result<String, String> {
    let entity = world
        .resource_entities()
        .get(property.resource_component_id)
        .ok_or_else(|| format!("Resource `{}` is not inserted", property.resource_type_path))?;
    let mut resource = world
        .get_reflect_mut(entity, property.resource_type_id)
        .map_err(|error| {
            format!(
                "Unable to reflect `{}`: {error}",
                property.resource_type_path
            )
        })?;
    let result = {
        let ReflectMut::Struct(resource) = resource.bypass_change_detection().reflect_mut() else {
            return Err(format!(
                "Resource `{}` no longer reflects as a struct",
                property.resource_type_path
            ));
        };
        let field = resource.field_mut(property.field_name).ok_or_else(|| {
            format!(
                "Resource `{}` has no reflected field `{}`",
                property.resource_type_path, property.field_name
            )
        })?;
        (property.value.replace)(field, value)
    };
    if result.is_ok() {
        resource.set_changed();
    }
    result
}

fn get_value(world: &World, name: &str) -> Result<String, String> {
    let property = property(world, name)?;
    (property.value.format)(reflected_field(world, &property)?)
}

fn set_value(world: &mut World, name: &str, input: &str) -> Result<String, String> {
    let property = property(world, name)?;
    if property.readonly {
        return Err(format!("{name} is read-only"));
    }
    let value = (property.value.parse)(input)?;
    replace_value(world, &property, value)
}

fn adjust_value(
    world: &mut World,
    name: &str,
    amount: &str,
    subtract: bool,
) -> Result<String, String> {
    let property = property(world, name)?;
    if !property.value.is_numeric {
        return Err(format!("{name} is not a numeric property"));
    }
    if property.readonly {
        return Err(format!("{name} is read-only"));
    }
    let value = (property.value.adjust)(reflected_field(world, &property)?, amount, subtract)?;
    replace_value(world, &property, value)
}

fn show_property(world: &mut World, name: &str) -> ConsoleResult {
    match get_value(world, name) {
        Ok(value) => {
            let help = property(world, name)
                .ok()
                .filter(|property| !property.help.is_empty())
                .map(|property| format!(" - {}", property.help))
                .unwrap_or_default();
            ConsoleResult::info(format!("{name} = {value}{help}"))
        }
        Err(error) => ConsoleResult::error(error),
    }
}

fn res_property(world: &mut World, args: Args) -> ConsoleResult {
    let Some(operation) = args.get(0) else {
        return ConsoleResult::error("Usage: res <get|set|add|sub|toggle> <property> [value]");
    };
    let Some(name) = args.get(1) else {
        return ConsoleResult::error("Usage: res <get|set|add|sub|toggle> <property> [value]");
    };
    match operation.to_ascii_lowercase().as_str() {
        "get" if args.len() == 2 => show_property(world, name),
        "set" if args.len() == 3 => {
            let value = args.get(2).expect("set arity was checked");
            match set_value(world, name, value) {
                Ok(value) => ConsoleResult::info(format!("{name} = {value}")),
                Err(error) => ConsoleResult::error(format!("Invalid value for {name}: {error}")),
            }
        }
        "add" | "sub" if args.len() == 3 => {
            let amount = args.get(2).expect("adjustment arity was checked");
            match adjust_value(world, name, amount, operation.eq_ignore_ascii_case("sub")) {
                Ok(value) => ConsoleResult::info(format!("{name} = {value}")),
                Err(error) => ConsoleResult::error(format!("Invalid amount for {name}: {error}")),
            }
        }
        "toggle" if args.len() == 2 => toggle_property(world, name),
        "get" => ConsoleResult::error("Usage: res get <property>"),
        "set" => ConsoleResult::error("Usage: res set <property> <value>"),
        "add" | "sub" => {
            ConsoleResult::error(format!("Usage: res {operation} <property> <amount>"))
        }
        "toggle" => ConsoleResult::error("Usage: res toggle <property>"),
        _ => ConsoleResult::error("Usage: res <get|set|add|sub|toggle> <property> [value]"),
    }
}

fn toggle_property(world: &mut World, name: &str) -> ConsoleResult {
    let Ok(property) = property(world, name) else {
        return ConsoleResult::error(format!("Unknown property: {name}"));
    };
    if !property.value.is_boolean {
        return ConsoleResult::error(format!("{name} is not a boolean property"));
    }
    if property.readonly {
        return ConsoleResult::error(format!("{name} is read-only"));
    }
    let Ok(field) = reflected_field(world, &property) else {
        return ConsoleResult::error(format!("Unable to read property: {name}"));
    };
    let Ok(value) = (property.value.format)(field) else {
        return ConsoleResult::error(format!("Unable to read property: {name}"));
    };
    let next = match value.to_ascii_lowercase().as_str() {
        "true" => "false",
        "false" => "true",
        _ => return ConsoleResult::error(format!("{name} is not a boolean property")),
    };
    match set_value(world, name, next) {
        Ok(value) => ConsoleResult::info(format!("{name} = {value}")),
        Err(error) => ConsoleResult::error(format!("Invalid value for {name}: {error}")),
    }
}

fn complete_res(
    In(request): ConsoleCompletionRequest,
    properties: Res<ConsoleResources>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => complete_res_operations(),
        1 => complete_res_property_names(&request, &properties),
        2 => complete_res_property_values(&request, &properties),
        _ => Vec::new(),
    }
}

fn complete_res_property_names(
    request: &CompletionRequest,
    properties: &ConsoleResources,
) -> Vec<CompletionItem> {
    let operation = request.argument(0);
    property_items(properties, |property| {
        if operation.is_some_and(|operation| operation.eq_ignore_ascii_case("toggle")) {
            property.value.is_boolean && !property.readonly
        } else if operation.is_some_and(|operation| {
            operation.eq_ignore_ascii_case("add") || operation.eq_ignore_ascii_case("sub")
        }) {
            property.value.is_numeric && !property.readonly
        } else if operation.is_some_and(|operation| operation.eq_ignore_ascii_case("set")) {
            !property.readonly
        } else {
            true
        }
    })
}

fn complete_res_operations() -> Vec<CompletionItem> {
    static_completion_items([
        ("add", "Adds to a numeric resource"),
        ("get", "Shows a resource value"),
        ("set", "Sets a resource value"),
        ("sub", "Subtracts from a numeric resource"),
        ("toggle", "Toggles a boolean resource"),
    ])
}

fn complete_res_property_values(
    request: &CompletionRequest,
    properties: &ConsoleResources,
) -> Vec<CompletionItem> {
    if !request
        .argument(0)
        .is_some_and(|operation| operation.eq_ignore_ascii_case("set"))
    {
        return Vec::new();
    }
    let Some(name) = request.argument(1) else {
        return Vec::new();
    };
    let Some(property) = properties.get(name) else {
        return Vec::new();
    };
    if !property.value.is_boolean || property.readonly {
        return Vec::new();
    }
    ["true", "false"]
        .into_iter()
        .map(CompletionItem::from)
        .collect()
}

fn property_items(
    properties: &ConsoleResources,
    predicate: impl Fn(&RegisteredConsoleProperty) -> bool,
) -> Vec<CompletionItem> {
    properties
        .iter()
        .filter(|(_, property)| predicate(property))
        .map(|(name, property)| {
            CompletionItem::new(
                name,
                if property.help.is_empty() {
                    "resource property"
                } else {
                    &property.help
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConsoleAliases, ConsoleAppExt, ConsoleBuffer, ConsoleCommandExecuted, ConsoleCommandQueue,
        ConsoleRegistry, ConsoleRequest, ConsoleState,
    };

    #[derive(Resource, Reflect)]
    #[reflect(Resource)]
    struct DebugSettings {
        /// Draw collider shapes
        draw_colliders: bool,
        #[reflect(@ConsoleProperty::readonly())]
        label: String,
        max_fps: u32,
        ignored_unsupported_type: Vec<String>,
    }

    #[derive(Reflect)]
    struct CustomValue(u32);

    impl ConsolePropertyValue for CustomValue {
        fn parse_console_value(input: &str) -> Result<Self, String> {
            input
                .parse()
                .map(Self)
                .map_err(|_| "expected a custom value".into())
        }

        fn format_console_value(&self) -> String {
            self.0.to_string()
        }
    }

    #[derive(Resource, Reflect)]
    #[reflect(Resource)]
    struct CustomSettings {
        value: CustomValue,
    }

    mod first {
        use bevy::prelude::*;

        #[derive(Resource, Reflect)]
        #[reflect(Resource)]
        pub struct Settings {
            pub value: u32,
        }
    }

    mod second {
        use bevy::prelude::*;

        #[derive(Resource, Reflect)]
        #[reflect(Resource)]
        pub struct Settings {
            pub value: u32,
        }
    }

    #[test]
    fn res_builtin_enables_property_commands() {
        let mut app = App::new();
        app.insert_resource(crate::BuiltinCommands::from([BuiltinCommand::Res]))
            .init_resource::<ConsoleRegistry>()
            .add_plugins(super::plugin);

        let registry = app.world().resource::<ConsoleRegistry>();
        assert!(registry.get("get").is_none());
        assert!(registry.get("res").is_some());
    }

    #[test]
    fn reflected_properties_mutate_the_registered_resource() {
        let mut app = App::new();
        app.insert_resource(crate::BuiltinCommands::from([BuiltinCommand::Res]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ConsoleRegistry>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(super::plugin)
            .insert_resource(DebugSettings {
                draw_colliders: false,
                label: "development".into(),
                max_fps: 60,
                ignored_unsupported_type: Vec::new(),
            })
            .add_console_resource::<DebugSettings>();
        let registry = app.world().resource::<ConsoleRegistry>();
        assert!(registry.get("get").is_none());
        assert!(registry.get("res").is_some());
        assert!(registry.get("set").is_none());
        assert!(registry.get("toggle").is_none());
        assert!(
            app.world()
                .resource::<ConsoleResources>()
                .get("DebugSettings.ignored_unsupported_type")
                .is_none()
        );
        let property_completions =
            property_items(app.world().resource::<ConsoleResources>(), |_| true);
        assert!(
            property_completions
                .iter()
                .any(|item| { item.label == "DebugSettings.draw_colliders" })
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res get DebugSettings.draw_colliders"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .text,
            "DebugSettings.draw_colliders = false - Draw collider shapes"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(
                "ReS SeT debugsettings.DRAW_COLLIDERS true",
            ));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(app.world().resource::<DebugSettings>().draw_colliders);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(
                "res set DebugSettings.label production",
            ));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(app.world().resource::<DebugSettings>().label, "development");

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(
                "res set DebugSettings.draw_colliders maybe",
            ));
        crate::execution::execute_pending_commands(app.world_mut());

        let messages = app.world().resource::<Messages<ConsoleCommandExecuted>>();
        let mut cursor = messages.get_cursor();
        assert!(!cursor.read(messages).last().unwrap().succeeded);
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .lines()
                .back()
                .unwrap()
                .level,
            crate::ConsoleLevel::Error
        );

        let last_changed = app.world().resource_ref::<DebugSettings>().last_changed();
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res add DebugSettings.max_fps invalid"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource_ref::<DebugSettings>().last_changed(),
            last_changed,
            "a failed adjustment must not mark the resource as changed"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res add DebugSettings.max_fps 24"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(app.world().resource::<DebugSettings>().max_fps, 84);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res sub DebugSettings.max_fps 30"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(app.world().resource::<DebugSettings>().max_fps, 54);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(
                "res toggle DebugSettings.draw_colliders",
            ));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(!app.world().resource::<DebugSettings>().draw_colliders);

        for command in [
            "res get DebugSettings.max_fps ignored",
            "res set DebugSettings.max_fps 120 ignored",
            "res add DebugSettings.max_fps 1 ignored",
            "res sub DebugSettings.max_fps 1 ignored",
            "res toggle DebugSettings.draw_colliders ignored",
        ] {
            app.world_mut()
                .resource_mut::<ConsoleCommandQueue>()
                .push(ConsoleRequest::new(command));
            crate::execution::execute_pending_commands(app.world_mut());
            assert_eq!(
                app.world()
                    .resource::<ConsoleBuffer>()
                    .last_line()
                    .unwrap()
                    .level,
                crate::ConsoleLevel::Error,
                "extra arguments should be rejected for `{command}`"
            );
        }
        assert_eq!(app.world().resource::<DebugSettings>().max_fps, 54);
        assert!(!app.world().resource::<DebugSettings>().draw_colliders);
    }

    #[test]
    fn application_specific_property_values_can_be_registered() {
        let mut app = App::new();
        app.register_console_property_value::<CustomValue>()
            .add_console_resource::<CustomSettings>()
            .insert_resource(CustomSettings {
                value: CustomValue(3),
            });

        assert_eq!(get_value(app.world(), "CustomSettings.value").unwrap(), "3");
        assert_eq!(
            set_value(app.world_mut(), "CustomSettings.value", "7").unwrap(),
            "7"
        );
        assert_eq!(app.world().resource::<CustomSettings>().value.0, 7);
    }

    #[test]
    fn duplicate_resource_names_use_full_type_paths() {
        let mut app = App::new();
        app.add_console_resource::<first::Settings>()
            .add_console_resource::<second::Settings>();
        let resources = app.world().resource::<ConsoleResources>();
        let first = format!("{}.value", first::Settings::type_path());
        let second = format!("{}.value", second::Settings::type_path());

        assert!(resources.get("Settings.value").is_none());
        assert!(resources.get(&first).is_some());
        assert!(resources.get(&second).is_some());
    }
}
