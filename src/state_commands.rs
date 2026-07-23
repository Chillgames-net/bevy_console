//! Opt-in console access to reflected, freely mutable Bevy states.

use crate::model::CommandSpec;
use crate::{
    Args, ArgumentSpec, BuiltinCommand, CompletionItem, ConsoleCompletionRequest, ConsoleRegistry,
    ConsoleResult, completion::static_completion_items,
};
use bevy::ecs::reflect::AppTypeRegistry;
use bevy::prelude::*;
use bevy::reflect::{
    FromReflect, GetTypeRegistration, ReflectRef, TypeInfo, TypeRegistry, Typed,
    enums::{DynamicEnum, EnumInfo, VariantInfo},
};
use bevy::state::reflect::{ReflectFreelyMutableState, ReflectState};
use bevy::state::{app::AppExtStates, state::FreelyMutableState};
use std::any::{TypeId, type_name};

/// Registers the built-in `state` command and its completion provider.
pub(crate) fn plugin(app: &mut App) {
    let enabled = app.world().resource::<crate::BuiltinCommands>().clone();
    app.init_resource::<ConsoleStates>();
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
    short_name: &'static str,
    type_path: &'static str,
    type_id: TypeId,
    type_info: &'static TypeInfo,
    state: ReflectState,
    mutable_state: ReflectFreelyMutableState,
}

/// Cached metadata for states explicitly exposed through [`crate::ConsoleAppExt`].
#[derive(Resource, Default)]
struct ConsoleStates {
    states: Vec<ReflectedState>,
}

impl ConsoleStates {
    fn register(&mut self, state: ReflectedState) {
        assert!(
            !self
                .states
                .iter()
                .any(|registered| registered.type_id == state.type_id),
            "console state `{}` is already registered",
            state.type_path
        );
        self.states.push(state);

        for index in 0..self.states.len() {
            let short_name = self.states[index].short_name;
            let has_collision = self
                .states
                .iter()
                .enumerate()
                .any(|(other, state)| other != index && state.short_name == short_name);
            self.states[index].name = if has_collision {
                self.states[index].type_path
            } else {
                short_name
            };
        }

        for (index, state) in self.states.iter().enumerate() {
            assert!(
                !self.states[..index]
                    .iter()
                    .any(|other| other.name == state.name),
                "console state name `{}` is ambiguous even when using its full type path",
                state.name
            );
        }
    }

    fn get(&self, name: &str) -> Option<ReflectedState> {
        self.states
            .iter()
            .find(|state| state.name == name || state.type_path == name)
            .cloned()
    }

    fn search(&self, name: &str) -> Option<ReflectedState> {
        self.get(name).or_else(|| {
            let mut matches = self.states.iter().filter(|state| {
                state.name.eq_ignore_ascii_case(name) || state.type_path.eq_ignore_ascii_case(name)
            });
            let state = matches.next()?;
            matches.next().is_none().then(|| state.clone())
        })
    }

    fn iter(&self) -> impl Iterator<Item = &ReflectedState> {
        self.states.iter()
    }
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
            .find(|variant| **variant == input)
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

pub(crate) fn register_state<S>(app: &mut App) -> &mut App
where
    S: FreelyMutableState + FromReflect + GetTypeRegistration + Typed,
{
    app.register_type_mutable_state::<S>();
    let state = {
        let registry = app.world().resource::<AppTypeRegistry>().read();
        let registration = registry
            .get(TypeId::of::<S>())
            .expect("the console state type was just registered");
        assert!(
            matches!(registration.type_info(), TypeInfo::Enum(_)),
            "console state `{}` must reflect as an enum",
            type_name::<S>()
        );
        ReflectedState {
            name: registration.type_info().type_path_table().short_path(),
            short_name: registration.type_info().type_path_table().short_path(),
            type_path: registration.type_info().type_path(),
            type_id: TypeId::of::<S>(),
            type_info: registration.type_info(),
            state: registration
                .data::<ReflectState>()
                .expect("the console state was registered with ReflectState")
                .clone(),
            mutable_state: registration
                .data::<ReflectFreelyMutableState>()
                .expect("the console state was registered with ReflectFreelyMutableState")
                .clone(),
        }
    };

    app.init_resource::<ConsoleStates>();
    app.world_mut()
        .resource_mut::<ConsoleStates>()
        .register(state);
    app
}

fn state_command(world: &mut World, args: Args) -> ConsoleResult {
    let Some(operation) = args.get(0) else {
        return ConsoleResult::error("Usage: state <get|set> <state> [value]");
    };
    let Some(name) = args.get(1) else {
        return ConsoleResult::error("Usage: state <get|set> <state> [value]");
    };
    let Some(state) = world
        .get_resource::<ConsoleStates>()
        .and_then(|states| states.get(name))
    else {
        return ConsoleResult::error(format!(
            "Unknown reflected state: {name}. Call add_console_state::<S>() to expose it"
        ));
    };

    match operation {
        "get" if args.len() == 2 => match state.current_value(world) {
            Ok(value) => ConsoleResult::info(format!("{} = {value}", state.name)),
            Err(error) => ConsoleResult::error(error),
        },
        "set" if args.len() == 3 => {
            let registry = world.resource::<AppTypeRegistry>().0.clone();
            let value = args.get(2).expect("set arity was checked");
            match state.set_value(world, &registry.read(), value) {
                Ok(value) => ConsoleResult::info(format!("{} = {value} (pending)", state.name)),
                Err(error) => {
                    ConsoleResult::error(format!("Invalid value for {}: {error}", state.name))
                }
            }
        }
        "get" => ConsoleResult::error("Usage: state get <state>"),
        "set" => ConsoleResult::error("Usage: state set <state> <value>"),
        _ => ConsoleResult::error("Usage: state <get|set> <state> [value]"),
    }
}

fn complete_state(
    In(request): ConsoleCompletionRequest,
    states: Res<ConsoleStates>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => static_completion_items([
            ("get", "Shows the current state value"),
            ("set", "Queues a state transition"),
        ]),
        1 => states
            .iter()
            .map(|state| CompletionItem::new(state.name, "reflected state"))
            .collect(),
        2 => {
            if !request
                .argument(0)
                .is_some_and(|operation| operation.eq_ignore_ascii_case("set"))
            {
                return Vec::new();
            }
            let Some(name) = request.argument(1) else {
                return Vec::new();
            };
            let Some(state) = states.search(name) else {
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

    #[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
    enum InspectorState {
        #[default]
        Hidden,
        Visible,
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
                "state set {} Playing",
                state_name()
            )));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Pending(GameState::Playing)
        ));
    }

    #[test]
    fn submitted_state_syntax_is_case_sensitive() {
        let mut app = app();
        for command in [
            format!("state GET {}", state_name()),
            format!("state get {}", state_name().to_ascii_lowercase()),
            format!("state set {} playing", state_name()),
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
                crate::ConsoleLevel::Error
            );
        }
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Unchanged
        ));
    }

    #[test]
    fn only_console_registered_states_are_cached_and_exposed() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<GameState>()
            .add_console_state::<GameState>()
            .init_state::<InspectorState>()
            .register_type_mutable_state::<InspectorState>();

        let states = app.world().resource::<ConsoleStates>();
        assert!(states.get("gamestate").is_none());
        assert!(states.search("gamestate").is_some());
        assert!(states.get("InspectorState").is_none());
    }

    #[test]
    fn state_command_rejects_data_carrying_variants() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(format!(
                "state set {} Loading",
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
    fn state_command_rejects_extra_arguments() {
        let mut app = app();
        for command in [
            format!("state get {} ignored", state_name()),
            format!("state set {} Playing ignored", state_name()),
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
                crate::ConsoleLevel::Error
            );
        }
        assert!(matches!(
            app.world().resource::<NextState<GameState>>(),
            NextState::Unchanged
        ));
    }

    #[test]
    fn duplicate_enum_names_use_full_type_paths() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<first::GameState>()
            .add_console_state::<first::GameState>()
            .init_state::<second::GameState>()
            .add_console_state::<second::GameState>();
        let names = app
            .world()
            .resource::<ConsoleStates>()
            .iter()
            .map(|state| state.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&<first::GameState as bevy::reflect::TypePath>::type_path()));
        assert!(names.contains(&<second::GameState as bevy::reflect::TypePath>::type_path()));
    }
}
