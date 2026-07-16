use crate::{Args, CommandSpec, CompletionItem, CompletionRequest, ConsoleResult};
use bevy::ecs::system::SystemId;
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub enum CommandExecutor {
    Structured(SystemId<In<Args>, ConsoleResult>),
    #[cfg(feature = "resource-properties")]
    Exclusive(fn(&mut World, Args) -> ConsoleResult),
}

pub struct CommandDef {
    /// Structured metadata used by help and completion.
    pub spec: CommandSpec,
    pub executor: CommandExecutor,
    pub completers: BTreeMap<usize, SystemId<In<CompletionRequest>, Vec<CompletionItem>>>,
}

/// Registry of all commands available in the console.
///
/// You can inject this as a resource and call `register_result_spec` directly,
/// but the preferred way is `app.add_console_command(name, usage, my_system)`.
#[derive(Resource, Default)]
pub struct ConsoleRegistry {
    pub commands: BTreeMap<String, CommandDef>,
}

impl ConsoleRegistry {
    /// Registers a command that returns structured lines with severity levels.
    pub fn register_result_spec(
        &mut self,
        spec: CommandSpec,
        system_id: SystemId<In<Args>, ConsoleResult>,
    ) {
        self.insert(spec, CommandExecutor::Structured(system_id));
    }

    /// Registers a command implemented directly against the [`World`].
    ///
    /// This is used internally for commands that must dynamically select a
    /// resource at runtime. Public commands should normally remain Bevy
    /// systems and use [`Self::register_result_spec`].
    #[cfg(feature = "resource-properties")]
    pub(crate) fn register_exclusive_spec(
        &mut self,
        spec: CommandSpec,
        command: fn(&mut World, Args) -> ConsoleResult,
    ) {
        self.insert(spec, CommandExecutor::Exclusive(command));
    }

    fn insert(&mut self, spec: CommandSpec, executor: CommandExecutor) {
        let name = self.prepare_registration(&spec);
        self.commands.insert(
            name,
            CommandDef {
                spec,
                executor,
                completers: BTreeMap::new(),
            },
        );
    }

    /// Associates a dynamic completion system with a command argument.
    pub fn register_completer(
        &mut self,
        command: &str,
        argument_index: usize,
        system_id: SystemId<In<CompletionRequest>, Vec<CompletionItem>>,
    ) -> bool {
        let Some(command) = self.resolve_name(command).map(str::to_owned) else {
            return false;
        };
        let Some(def) = self.commands.get_mut(&command) else {
            return false;
        };
        def.completers.insert(argument_index, system_id);
        true
    }

    /// Finds a command using its name or an alias, case-insensitively.
    pub fn get(&self, name: &str) -> Option<&CommandDef> {
        self.resolve_name(name)
            .and_then(|name| self.commands.get(name))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut CommandDef> {
        let canonical = self.resolve_name(name)?.to_owned();
        self.commands.get_mut(&canonical)
    }

    fn prepare_registration(&self, spec: &CommandSpec) -> String {
        let name = spec.name.to_ascii_lowercase();
        if let Some(canonical) = self.resolve_alias(&name) {
            assert_eq!(
                canonical, name,
                "command `{}` collides with alias for `{canonical}`",
                spec.name
            );
        }

        let mut seen = BTreeSet::new();
        for alias in &spec.aliases {
            let alias = alias.to_ascii_lowercase();
            assert_ne!(alias, name, "command `{name}` cannot alias itself");
            assert!(
                !self.commands.contains_key(&alias),
                "alias `{alias}` for `{name}` collides with registered command"
            );
            if let Some(canonical) = self.resolve_alias(&alias) {
                assert_eq!(
                    canonical, name,
                    "alias `{alias}` for `{name}` collides with alias for `{canonical}`"
                );
            }
            assert!(
                seen.insert(alias.clone()),
                "alias `{alias}` is repeated for command `{name}`"
            );
        }
        name
    }

    pub fn resolve_name(&self, name: &str) -> Option<&str> {
        let normalized = name.to_ascii_lowercase();
        if self.commands.contains_key(&normalized) {
            return self
                .commands
                .get_key_value(&normalized)
                .map(|(name, _)| name.as_str());
        }
        self.resolve_alias(&normalized)
    }

    fn resolve_alias(&self, alias: &str) -> Option<&str> {
        self.commands.iter().find_map(|(name, def)| {
            def.spec
                .aliases
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(alias))
                .then_some(name.as_str())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ConsoleRegistry;
    use crate::{Args, CommandSpec, ConsoleResult};
    use bevy::prelude::*;

    fn noop(In(_args): In<Args>) -> ConsoleResult {
        ConsoleResult::default()
    }

    #[test]
    fn replacing_a_command_removes_its_stale_aliases() {
        let mut world = World::new();
        let first = world.register_system(noop);
        let second = world.register_system(noop);
        let mut registry = ConsoleRegistry::default();
        registry.register_result_spec(
            CommandSpec::new("map").help("map").alias("ChangeLevel"),
            first,
        );
        registry.register_result_spec(CommandSpec::new("map").help("map").alias("LoadMap"), second);

        assert!(registry.get("changelevel").is_none());
        assert!(registry.get("LOADMAP").is_some());
    }

    #[test]
    #[should_panic(expected = "collides with registered command")]
    fn alias_cannot_shadow_a_registered_command() {
        let mut world = World::new();
        let first = world.register_system(noop);
        let second = world.register_system(noop);
        let mut registry = ConsoleRegistry::default();
        registry.register_result_spec(CommandSpec::new("foo"), first);
        registry.register_result_spec(CommandSpec::new("bar").alias("foo"), second);
    }

    #[test]
    #[should_panic(expected = "collides with alias")]
    fn command_cannot_shadow_a_registered_alias() {
        let mut world = World::new();
        let first = world.register_system(noop);
        let second = world.register_system(noop);
        let mut registry = ConsoleRegistry::default();
        registry.register_result_spec(CommandSpec::new("bar").alias("foo"), first);
        registry.register_result_spec(CommandSpec::new("foo"), second);
    }
}
