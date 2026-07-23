use crate::{Args, CompletionItem, ConsoleCompletionRequest, ConsoleResult, model::CommandSpec};
use bevy::ecs::system::SystemId;
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandExecutor {
    Structured(SystemId<In<Args>, ConsoleResult>),
    Exclusive(fn(&mut World, Args) -> ConsoleResult),
}

pub(crate) struct CommandDef {
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
    pub(crate) commands: BTreeMap<String, CommandDef>,
}

impl ConsoleRegistry {
    pub(crate) fn register(
        &mut self,
        spec: CommandSpec,
        system_id: SystemId<In<Args>, ConsoleResult>,
        completer: Option<SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>>,
    ) {
        self.insert_with_completer(spec, CommandExecutor::Structured(system_id), completer);
    }

    pub(crate) fn register_exclusive_spec_with_completer(
        &mut self,
        spec: CommandSpec,
        command: fn(&mut World, Args) -> ConsoleResult,
        completer: SystemId<ConsoleCompletionRequest, Vec<CompletionItem>>,
    ) {
        self.insert_with_completer(spec, CommandExecutor::Exclusive(command), Some(completer));
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

    /// Finds a command using its exact registered name or alias.
    pub(crate) fn get(&self, name: &str) -> Option<&CommandDef> {
        self.resolve_name(name)
            .and_then(|name| self.commands.get(name))
    }

    /// Finds a command for case-insensitive completion search.
    pub(crate) fn search(&self, name: &str) -> Option<&CommandDef> {
        if let Some(command) = self.get(name) {
            return Some(command);
        }
        let mut matches = self.commands.values().filter(|definition| {
            definition.spec.name.eq_ignore_ascii_case(name)
                || definition
                    .spec
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(name))
        });
        let command = matches.next()?;
        matches.next().is_none().then_some(command)
    }

    /// Returns whether an exact command or registered alias exists.
    pub fn contains(&self, name: &str) -> bool {
        self.resolve_name(name).is_some()
    }

    /// Iterates over canonical command names in sorted order.
    pub fn command_names(&self) -> impl Iterator<Item = &str> {
        self.commands.keys().map(String::as_str)
    }

    fn prepare_registration(&self, spec: &CommandSpec) -> String {
        let name = spec.name.clone();
        if let Some(canonical) = self.resolve_alias(&name) {
            assert_eq!(
                canonical, name,
                "command `{}` collides with alias for `{canonical}`",
                spec.name
            );
        }

        let mut seen = BTreeSet::new();
        for alias in &spec.aliases {
            assert_ne!(*alias, name, "command `{name}` cannot alias itself");
            assert!(
                !self.commands.contains_key(*alias),
                "alias `{alias}` for `{name}` collides with registered command"
            );
            if let Some(canonical) = self.resolve_alias(alias) {
                assert_eq!(
                    canonical, name,
                    "alias `{alias}` for `{name}` collides with alias for `{canonical}`"
                );
            }
            assert!(
                seen.insert(*alias),
                "alias `{alias}` is repeated for command `{name}`"
            );
        }
        name
    }

    pub fn resolve_name(&self, name: &str) -> Option<&str> {
        if self.commands.contains_key(name) {
            return self
                .commands
                .get_key_value(name)
                .map(|(name, _)| name.as_str());
        }
        self.resolve_alias(name)
    }

    fn resolve_alias(&self, alias: &str) -> Option<&str> {
        self.commands
            .iter()
            .find_map(|(name, def)| def.spec.aliases.contains(&alias).then_some(name.as_str()))
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
        registry.register(
            CommandSpec {
                aliases: vec!["ChangeLevel"],
                ..CommandSpec::new("map", "map")
            },
            first,
            None,
        );
        registry.register(
            CommandSpec {
                aliases: vec!["LoadMap"],
                ..CommandSpec::new("map", "map")
            },
            second,
            None,
        );

        assert!(registry.get("changelevel").is_none());
        assert!(registry.get("LoadMap").is_some());
        assert!(registry.get("LOADMAP").is_none());
        assert!(registry.search("LOADMAP").is_some());
        assert!(!registry.contains("changelevel"));
        assert!(registry.contains("LoadMap"));
        assert!(!registry.contains("LOADMAP"));
        assert_eq!(registry.command_names().collect::<Vec<_>>(), ["map"]);
    }

    #[test]
    #[should_panic(expected = "collides with registered command")]
    fn alias_cannot_shadow_a_registered_command() {
        let mut world = World::new();
        let first = world.register_system(noop);
        let second = world.register_system(noop);
        let mut registry = ConsoleRegistry::default();
        registry.register(CommandSpec::new("foo", "foo"), first, None);
        registry.register(
            CommandSpec {
                aliases: vec!["foo"],
                ..CommandSpec::new("bar", "bar")
            },
            second,
            None,
        );
    }

    #[test]
    #[should_panic(expected = "collides with alias")]
    fn command_cannot_shadow_a_registered_alias() {
        let mut world = World::new();
        let first = world.register_system(noop);
        let second = world.register_system(noop);
        let mut registry = ConsoleRegistry::default();
        registry.register(
            CommandSpec {
                aliases: vec!["foo"],
                ..CommandSpec::new("bar", "bar")
            },
            first,
            None,
        );
        registry.register(CommandSpec::new("foo", "foo"), second, None);
    }
}
