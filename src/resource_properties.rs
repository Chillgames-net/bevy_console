//! Opt-in resource-backed console properties.
//!
//! This module is enabled by the `resource-properties` feature. The
//! [`ConsoleResource`] derive generates the typed accessors; this module keeps
//! their runtime registry and all console command integration in one place.

use crate::{
    Args, ArgumentSpec, BuiltinCommand, CommandSpec, CompletionItem, CompletionRequest,
    ConsoleAppExt, ConsoleConfig, ConsoleRegistry, ConsoleResult,
};
use bevy::prelude::*;
use std::collections::BTreeMap;

type Getter = fn(&World) -> Result<String, String>;
type Setter = fn(&mut World, &str) -> Result<String, String>;
type Adjuster = fn(&mut World, &str, bool) -> Result<String, String>;

/// Generated metadata and accessors for one field exposed by a
/// [`ConsoleResource`].
#[derive(Clone, Copy)]
pub struct ConsoleProperty {
    name: &'static str,
    help: &'static str,
    getter: Getter,
    setter: Option<Setter>,
    adjuster: Option<Adjuster>,
    is_boolean: bool,
    is_numeric: bool,
}

impl ConsoleProperty {
    #[doc(hidden)]
    pub const fn new(
        name: &'static str,
        help: &'static str,
        getter: Getter,
        setter: Option<Setter>,
        adjuster: Option<Adjuster>,
        is_boolean: bool,
        is_numeric: bool,
    ) -> Self {
        Self {
            name,
            help,
            getter,
            setter,
            adjuster,
            is_boolean,
            is_numeric,
        }
    }
}

/// A Bevy resource that exposes selected fields through the console.
///
/// Implement this with `#[derive(ConsoleResource)]`; the resource must also
/// implement Bevy's [`Resource`].
pub trait ConsoleResource: Resource {
    #[doc(hidden)]
    fn console_properties() -> &'static [ConsoleProperty];
}

/// A field type that can be read from and written from the console.
///
/// The built-in implementations cover booleans, all primitive integer and
/// float types, and [`String`]. Implement this trait for application-specific
/// field types to make them available to `#[derive(ConsoleResource)]`.
pub trait ConsolePropertyValue: 'static {
    #[doc(hidden)]
    const IS_BOOLEAN: bool = false;
    #[doc(hidden)]
    const IS_NUMERIC: bool = false;

    #[doc(hidden)]
    fn parse_console_value(input: &str) -> Result<Self, String>
    where
        Self: Sized;

    #[doc(hidden)]
    fn format_console_value(&self) -> String;

    #[doc(hidden)]
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

/// Runtime registry for fields exposed by registered [`ConsoleResource`] types.
#[derive(Resource, Default)]
pub struct ConsoleResources {
    properties: BTreeMap<String, ConsoleProperty>,
}

impl ConsoleResources {
    fn register<R: ConsoleResource>(&mut self) {
        for property in R::console_properties() {
            let name = property.name.to_ascii_lowercase();
            assert!(!name.is_empty(), "console property names cannot be empty");
            assert!(
                !self.properties.contains_key(&name),
                "console property `{}` is already registered",
                property.name
            );
            self.properties.insert(name, *property);
        }
    }

    fn get(&self, name: &str) -> Option<ConsoleProperty> {
        self.properties.get(&name.to_ascii_lowercase()).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (&str, ConsoleProperty)> {
        self.properties
            .iter()
            .map(|(name, property)| (name.as_str(), *property))
    }
}

/// Registers the resource-property built-in commands and completion providers.
pub(crate) fn plugin(app: &mut App) {
    let enabled = app
        .world()
        .resource::<ConsoleConfig>()
        .builtin_commands
        .clone();
    app.init_resource::<ConsoleResources>();
    {
        let mut registry = app.world_mut().resource_mut::<ConsoleRegistry>();
        if enabled.contains(&BuiltinCommand::Res) {
            registry.register_exclusive_spec(
                CommandSpec::new("res")
                    .help(
                        "res <get|set|add|sub|toggle> <property> [value] - inspect or modify a resource property",
                    )
                    .summary("Inspect or modify a resource property")
                    .args([
                        ArgumentSpec::new("operation"),
                        ArgumentSpec::new("property").help("Property name"),
                        ArgumentSpec::new("value").help("Value or amount"),
                    ]),
                res_property,
            );
        }
    }
    if enabled.contains(&BuiltinCommand::Res) {
        app.add_console_completer("res", 0, complete_res_operations)
            .add_console_completer("res", 1, complete_res_property_names)
            .add_console_completer("res", 2, complete_res_property_values);
    }
}

pub(crate) fn register_resource<R: ConsoleResource>(app: &mut App) {
    app.init_resource::<ConsoleResources>();
    app.world_mut()
        .resource_mut::<ConsoleResources>()
        .register::<R>();
}

fn property(world: &World, name: &str) -> Result<ConsoleProperty, String> {
    world
        .get_resource::<ConsoleResources>()
        .and_then(|properties| properties.get(name))
        .ok_or_else(|| format!("Unknown property: {name}"))
}

fn get_value(world: &World, name: &str) -> Result<String, String> {
    let property = property(world, name)?;
    (property.getter)(world)
}

fn set_value(world: &mut World, name: &str, input: &str) -> Result<String, String> {
    let property = property(world, name)?;
    let setter = property
        .setter
        .ok_or_else(|| format!("{name} is read-only"))?;
    let value = setter(world, input)?;
    Ok(value)
}

fn adjust_value(
    world: &mut World,
    name: &str,
    amount: &str,
    subtract: bool,
) -> Result<String, String> {
    let property = property(world, name)?;
    if !property.is_numeric {
        return Err(format!("{name} is not a numeric property"));
    }
    let adjuster = property
        .adjuster
        .ok_or_else(|| format!("{name} is read-only"))?;
    adjuster(world, amount, subtract)
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
        "get" => show_property(world, name),
        "set" => {
            let Some(value) = args.get(2) else {
                return ConsoleResult::error("Usage: res set <property> <value>");
            };
            match set_value(world, name, value) {
                Ok(value) => ConsoleResult::info(format!("{name} = {value}")),
                Err(error) => ConsoleResult::error(format!("Invalid value for {name}: {error}")),
            }
        }
        "add" | "sub" => {
            let Some(amount) = args.get(2) else {
                return ConsoleResult::error(format!("Usage: res {operation} <property> <amount>"));
            };
            match adjust_value(world, name, amount, operation.eq_ignore_ascii_case("sub")) {
                Ok(value) => ConsoleResult::info(format!("{name} = {value}")),
                Err(error) => ConsoleResult::error(format!("Invalid amount for {name}: {error}")),
            }
        }
        "toggle" => toggle_property(world, name),
        _ => ConsoleResult::error("Usage: res <get|set|add|sub|toggle> <property> [value]"),
    }
}

fn toggle_property(world: &mut World, name: &str) -> ConsoleResult {
    let Ok(property) = property(world, name) else {
        return ConsoleResult::error(format!("Unknown property: {name}"));
    };
    if !property.is_boolean {
        return ConsoleResult::error(format!("{name} is not a boolean property"));
    }
    let Ok(value) = (property.getter)(world) else {
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

fn complete_res_property_names(
    In(request): In<CompletionRequest>,
    properties: Res<ConsoleResources>,
) -> Vec<CompletionItem> {
    let operation = request
        .parsed
        .tokens
        .get(1)
        .map(|token| token.value.as_str());
    property_items(&request, &properties, |property| match operation {
        Some("toggle") => property.is_boolean,
        Some("add" | "sub") => property.is_numeric,
        _ => true,
    })
}

fn complete_res_operations(In(request): In<CompletionRequest>) -> Vec<CompletionItem> {
    [
        ("add", "Adds to a numeric resource"),
        ("get", "Shows a resource value"),
        ("set", "Sets a resource value"),
        ("sub", "Subtracts from a numeric resource"),
        ("toggle", "Toggles a boolean resource"),
    ]
    .into_iter()
    .map(|(operation, detail)| {
        let mut item = CompletionItem::new(operation, request.parsed.replacement_range());
        item.detail = detail.into();
        item
    })
    .collect()
}

fn complete_res_property_values(
    In(request): In<CompletionRequest>,
    properties: Res<ConsoleResources>,
) -> Vec<CompletionItem> {
    if !matches!(
        request
            .parsed
            .tokens
            .get(1)
            .map(|token| token.value.as_str()),
        Some("set")
    ) {
        return Vec::new();
    }
    let Some(name) = request
        .parsed
        .tokens
        .get(2)
        .map(|token| token.value.as_str())
    else {
        return Vec::new();
    };
    let Some(property) = properties.get(name) else {
        return Vec::new();
    };
    if !property.is_boolean {
        return Vec::new();
    }
    ["true", "false"]
        .into_iter()
        .map(|value| CompletionItem::new(value, request.parsed.replacement_range()))
        .collect()
}

fn property_items(
    request: &CompletionRequest,
    properties: &ConsoleResources,
    predicate: impl Fn(ConsoleProperty) -> bool,
) -> Vec<CompletionItem> {
    properties
        .iter()
        .filter(|(_, property)| predicate(*property))
        .map(|(name, property)| {
            let mut item = CompletionItem::new(name, request.parsed.replacement_range());
            item.detail = if property.help.is_empty() {
                "resource property".into()
            } else {
                property.help.into()
            };
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConsoleAliases, ConsoleAppExt, ConsoleBuffer, ConsoleCommandExecuted, ConsoleCommandQueue,
        ConsoleConfig, ConsoleRegistry, ConsoleRequest, ConsoleState,
    };

    #[derive(Resource, super::super::ConsoleResource)]
    #[console_resource(prefix = "debug")]
    struct DebugSettings {
        #[console(help = "Draw collider shapes")]
        draw_colliders: bool,
        #[console(readonly)]
        label: String,
        #[console()]
        max_fps: u32,
    }

    #[test]
    fn res_builtin_enables_property_commands() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig {
            builtin_commands: [BuiltinCommand::Res].into_iter().collect(),
            ..default()
        })
        .init_resource::<ConsoleRegistry>()
        .add_plugins(super::plugin);

        let registry = app.world().resource::<ConsoleRegistry>();
        assert!(registry.get("get").is_none());
        assert!(registry.get("res").is_some());
    }

    #[test]
    fn properties_mutate_the_registered_resource() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
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
            })
            .add_console_resource::<DebugSettings>();
        let registry = app.world().resource::<ConsoleRegistry>();
        assert!(registry.get("get").is_none());
        assert!(registry.get("res").is_some());
        assert!(registry.get("set").is_none());
        assert!(registry.get("toggle").is_none());

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res get debug.draw_colliders"));
        crate::input::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .lines()
                .back()
                .unwrap()
                .text,
            "debug.draw_colliders = false - Draw collider shapes"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res set debug.draw_colliders true"));
        crate::input::execute_pending_commands(app.world_mut());
        assert!(app.world().resource::<DebugSettings>().draw_colliders);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res set debug.draw_colliders maybe"));
        crate::input::execute_pending_commands(app.world_mut());

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
            .push(ConsoleRequest::new("res add debug.max_fps invalid"));
        crate::input::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource_ref::<DebugSettings>().last_changed(),
            last_changed,
            "a failed adjustment must not mark the resource as changed"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res add debug.max_fps 24"));
        crate::input::execute_pending_commands(app.world_mut());
        assert_eq!(app.world().resource::<DebugSettings>().max_fps, 84);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res sub debug.max_fps 30"));
        crate::input::execute_pending_commands(app.world_mut());
        assert_eq!(app.world().resource::<DebugSettings>().max_fps, 54);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("res toggle debug.draw_colliders"));
        crate::input::execute_pending_commands(app.world_mut());
        assert!(!app.world().resource::<DebugSettings>().draw_colliders);
    }
}
