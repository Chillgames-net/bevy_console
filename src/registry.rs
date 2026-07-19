use crate::{Args, CompletionItem, ConsoleCompletionRequest, ConsoleResult, model::CommandSpec};
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
    pub(crate) spec: CommandSpec,
    pub(crate) executor: CommandExecutor,
    pub(crate) completer: Option<SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>>,
}

/// Registry of all commands available in the console.
///
/// The registry is available as a resource for command lookup. Register
/// commands through [`crate::ConsoleAppExt`].
#[derive(Resource, Default)]
pub struct ConsoleRegistry {
    pub commands: BTreeMap<String, CommandDef>,
}

impl ConsoleRegistry {
    /// Registers a command that returns structured lines with severity levels.
    pub(crate) fn register_result_spec(
        &mut self,
        spec: CommandSpec,
        system_id: SystemId<In<Args>, ConsoleResult>,
    ) {
        self.insert(spec, CommandExecutor::Structured(system_id));
    }

    pub(crate) fn register_result_spec_with_completer(
        &mut self,
        spec: CommandSpec,
        system_id: SystemId<In<Args>, ConsoleResult>,
        completer: SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>,
    ) {
        self.insert_with_completer(
            spec,
            CommandExecutor::Structured(system_id),
            Some(completer),
        );
    }

    #[cfg(feature = "resource-properties")]
    pub(crate) fn register_exclusive_spec_with_completer(
        &mut self,
        spec: CommandSpec,
        command: fn(&mut World, Args) -> ConsoleResult,
        completer: SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>,
    ) {
        self.insert_with_completer(spec, CommandExecutor::Exclusive(command), Some(completer));
    }

    fn insert(&mut self, spec: CommandSpec, executor: CommandExecutor) {
        self.insert_with_completer(spec, executor, None);
    }

    fn insert_with_completer(
        &mut self,
        spec: CommandSpec,
        executor: CommandExecutor,
        completer: Option<SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>>,
    ) {
        let name = self.prepare_registration(&spec);
        self.commands.insert(
            name,
            CommandDef {
                spec,
                executor,
                completer,
            },
        );
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
    use crate::{Args, ConsoleResult, model::CommandSpec};
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
