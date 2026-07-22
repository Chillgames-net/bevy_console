//! Opt-in console access to reflected, freely mutable Bevy states.

use crate::model::CommandSpec;
use crate::{
    Args, ArgumentSpec, BuiltinCommand, CompletionItem, ConsoleCompletionRequest, ConsoleRegistry,
    ConsoleResult, completion::static_completion_items,
};
use bevy::ecs::reflect::AppTypeRegistry;
use bevy::prelude::*;
use bevy::reflect::{
    ReflectRef, TypeInfo, TypeRegistry,
    enums::{DynamicEnum, EnumInfo, VariantInfo},
};
use bevy::state::reflect::{ReflectFreelyMutableState, ReflectState};
use std::collections::HashMap;

/// Registers the built-in `state` command and its completion provider.
pub(crate) fn plugin(app: &mut App) {
    let enabled = app.world().resource::<crate::BuiltinCommands>().clone();
    if enabled.contains(&BuiltinCommand::State) {
        let completer = app.world_mut().register_system(complete_state);
        app.world_mut()
            .resource_mut::<ConsoleRegistry>()
            .register_exclusive_spec_with_completer(
                CommandSpec {
                    summary: "Inspect or change a reflected Bevy state",
                    args: vec![
                        ArgumentSpec::new("operation"),
                        ArgumentSpec::new("state").help("Reflected state type path"),
                        ArgumentSpec::new("value").help("Unit enum variant"),
                    ],
                    ..CommandSpec::new(
                        "state",
                        "state <get|set> <state> [value] - inspect or change a reflected Bevy state",
                    )
                },
                state_command,
                completer,
            );
    }
}

#[derive(Clone)]
struct ReflectedState {
    name: &'static str,
    type_path: &'static str,
    type_info: &'static TypeInfo,
    state: ReflectState,
    mutable_state: ReflectFreelyMutableState,
}

impl ReflectedState {
    fn enum_info(&self) -> &EnumInfo {
        self.type_info
            .as_enum()
            .expect("reflected console states are filtered to enum types")
    }

    fn current_value(&self, world: &World) -> Result<String, String> {
        let value = self
            .state
            .reflect(world)
            .ok_or_else(|| format!("State `{}` is not initialized", self.name))?;
        let ReflectRef::Enum(value) = value.reflect_ref() else {
            return Err(format!("State `{}` is not an enum", self.name));
        };
        Ok(value.variant_name().to_owned())
    }

    fn set_value(
        &self,
        world: &mut World,
        registry: &TypeRegistry,
        input: &str,
    ) -> Result<String, String> {
        let Some(variant) = self
            .enum_info()
            .variant_names()
            .iter()
            .find(|variant| variant.eq_ignore_ascii_case(input))
        else {
            return Err(format!(
                "expected one of: {}",
                self.unit_variants().join(", ")
            ));
        };
        if !matches!(
            self.enum_info().variant(variant),
            Some(VariantInfo::Unit(_))
        ) {
            return Err(format!(
                "{variant} has data; only unit enum variants are supported"
            ));
        }

        let value = self
            .state
            .reflect(world)
            .ok_or_else(|| format!("State `{}` is not initialized", self.name))?;
        let mut value = value
            .reflect_clone()
            .map_err(|error| format!("Unable to clone state `{}`: {error}", self.name))?;
        value.apply(&DynamicEnum::new(*variant, ()));
        self.mutable_state.set_next_state(world, &*value, registry);
        Ok((*variant).to_owned())
    }

    fn unit_variants(&self) -> Vec<&'static str> {
        self.enum_info()
            .variant_names()
            .iter()
            .copied()
            .filter(|name| matches!(self.enum_info().variant(name), Some(VariantInfo::Unit(_))))
            .collect()
    }
}

fn reflected_states(registry: &TypeRegistry) -> Vec<ReflectedState> {
    let mut states = registry
        .iter()
        .filter_map(|registration| {
            matches!(registration.type_info(), TypeInfo::Enum(_)).then_some(())?;
            Some(ReflectedState {
                name: registration.type_info().type_path_table().short_path(),
                type_path: registration.type_info().type_path(),
                type_info: registration.type_info(),
                state: registration.data::<ReflectState>()?.clone(),
                mutable_state: registration.data::<ReflectFreelyMutableState>()?.clone(),
            })
        })
        .collect::<Vec<_>>();
    let name_counts = states.iter().fold(HashMap::new(), |mut counts, state| {
        *counts.entry(state.name).or_insert(0) += 1;
        counts
    });
    for state in &mut states {
        if name_counts[state.name] > 1 {
            state.name = state.type_path;
        }
    }
    states
}

fn find_state(registry: &TypeRegistry, name: &str) -> Option<ReflectedState> {
    reflected_states(registry).into_iter().find(|state| {
        state.name.eq_ignore_ascii_case(name) || state.type_path.eq_ignore_ascii_case(name)
    })
}

fn type_registry(world: &World) -> Result<bevy::reflect::TypeRegistryArc, String> {
    world
        .get_resource::<AppTypeRegistry>()
        .map(|registry| registry.0.clone())
        .ok_or_else(|| {
            "No reflected states are registered; call add_console_state::<S>() for each console state"
                .into()
        })
}

fn state_command(world: &mut World, args: Args) -> ConsoleResult {
    let Some(operation) = args.get(0) else {
        return ConsoleResult::error("Usage: state <get|set> <state> [value]");
    };
    let Some(name) = args.get(1) else {
        return ConsoleResult::error("Usage: state <get|set> <state> [value]");
    };
    let registry = match type_registry(world) {
        Ok(registry) => registry,
        Err(error) => return ConsoleResult::error(error),
    };
    let registry = registry.read();
    let Some(state) = find_state(&registry, name) else {
        return ConsoleResult::error(format!(
            "Unknown reflected state: {name}. Call add_console_state::<S>() to expose it"
        ));
    };

    match operation.to_ascii_lowercase().as_str() {
        "get" if args.len() == 2 => match state.current_value(world) {
            Ok(value) => ConsoleResult::info(format!("{} = {value}", state.name)),
            Err(error) => ConsoleResult::error(error),
        },
        "set" if args.len() == 3 => match state.set_value(world, &registry, args.get(2).unwrap()) {
            Ok(value) => ConsoleResult::info(format!("{} = {value} (pending)", state.name)),
            Err(error) => {
                ConsoleResult::error(format!("Invalid value for {}: {error}", state.name))
            }
        },
        "get" => ConsoleResult::error("Usage: state get <state>"),
        "set" => ConsoleResult::error("Usage: state set <state> <value>"),
        _ => ConsoleResult::error("Usage: state <get|set> <state> [value]"),
    }
}

fn complete_state(
    In(request): ConsoleCompletionRequest,
    registry: Option<Res<AppTypeRegistry>>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => static_completion_items([
            ("get", "Shows the current state value"),
            ("set", "Queues a state transition"),
        ]),
        1 => registry
            .as_ref()
            .map(|registry| {
                reflected_states(&registry.0.read())
                    .into_iter()
                    .map(|state| CompletionItem::new(state.name, "reflected state"))
                    .collect()
            })
            .unwrap_or_default(),
        2 => {
            if !request
                .argument(0)
                .is_some_and(|operation| operation.eq_ignore_ascii_case("set"))
            {
                return Vec::new();
            }
            let Some(registry) = registry else {
                return Vec::new();
            };
            let Some(name) = request.argument(1) else {
                return Vec::new();
            };
            let registry = registry.0.read();
            let Some(state) = find_state(&registry, name) else {
                return Vec::new();
            };
            state
                .unit_variants()
                .into_iter()
                .map(CompletionItem::from)
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConsoleAliases, ConsoleAppExt, ConsoleBuffer, ConsoleCommandExecuted, ConsoleCommandQueue,
        ConsoleRequest, ConsoleState,
    };

    #[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
    enum GameState {
        #[default]
        Menu,
        Playing,
        Loading {
            level: u32,
        },
    }

    mod first {
        use bevy::prelude::*;

        #[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
        pub enum GameState {
            #[default]
            Menu,
        }
    }

    mod second {
        use bevy::prelude::*;

        #[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
        pub enum GameState {
            #[default]
            Menu,
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(crate::BuiltinCommands::from([BuiltinCommand::State]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ConsoleRegistry>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<GameState>()
            .add_console_state::<GameState>()
            .add_plugins(super::plugin);
        app
    }

    fn state_name() -> &'static str {
        <GameState as bevy::reflect::TypePath>::short_type_path()
    }

    #[test]
    fn state_builtin_discovers_and_transitions_reflected_states() {
        let mut app = app();
        assert!(
            app.world()
                .resource::<ConsoleRegistry>()
                .get("state")
                .is_some()
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(format!("state get {}", state_name())));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .text,
            format!("{} = Menu", state_name())
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(format!(
                "state set {} playing",
                state_name()
            )));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Pending(GameState::Playing)
        ));
    }

    #[test]
    fn state_command_rejects_data_carrying_variants() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(format!(
                "state set {} loading",
                state_name()
            )));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Unchanged
        ));
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .level,
            crate::ConsoleLevel::Error
        );
    }

    #[test]
    fn duplicate_enum_names_use_full_type_paths() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<first::GameState>()
            .add_console_state::<first::GameState>()
            .init_state::<second::GameState>()
            .add_console_state::<second::GameState>();
        let registry = app.world().resource::<AppTypeRegistry>().0.clone();
        let names = reflected_states(&registry.read())
            .into_iter()
            .map(|state| state.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&<first::GameState as bevy::reflect::TypePath>::type_path()));
        assert!(names.contains(&<second::GameState as bevy::reflect::TypePath>::type_path()));
    }
}
