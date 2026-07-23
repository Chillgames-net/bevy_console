use crate::{
    ArgumentSpec, BuiltinCommand, CommandArgs, CompletionItem, CompletionRequest, ConsoleAliases,
    ConsoleAppExt, ConsoleBinds, ConsoleBuffer, ConsoleCommand, ConsoleCompletionRequest,
    ConsoleKeyBinding, ConsoleKeyModifiers, ConsoleRegistry, ConsoleResult,
    completion::{runtime_command_completions, static_completion_items},
};
use bevy::prelude::*;
use bevy::reflect::{FromReflect, Typed, enums::DynamicEnum};

pub(crate) fn plugin(app: &mut App) {
    let enabled = app.world().resource::<crate::BuiltinCommands>().clone();
    if enabled.contains(&BuiltinCommand::Clear) {
        app.add_console_command(
            ConsoleCommand::new("clear", "clear - clear the console output", clear_cmd)
                .with_summary("Clear the console output"),
        );
    }
    if enabled.contains(&BuiltinCommand::Help) {
        app.add_console_command(
            ConsoleCommand::new(
                "help",
                "help [command] - show available commands or command help",
                help_cmd,
            )
            .with_summary("Show command help")
            .with_args([ArgumentSpec::new("command").help("Command to describe")]),
        );
    }
    if enabled.contains(&BuiltinCommand::Alias) {
        app.add_console_command(
            ConsoleCommand::new(
                "alias",
                "alias <list|get|set|remove> [name] [command...] - manage runtime aliases",
                alias_cmd,
            )
            .with_summary("Manage runtime command aliases")
            .with_args([
                ArgumentSpec::new("operation"),
                ArgumentSpec::new("name").help("Alias name"),
                ArgumentSpec::new("command").help("Command expansion"),
            ])
            .with_completions(complete_alias),
        );
    }
    if enabled.contains(&BuiltinCommand::Bind) {
        app.add_console_command(
            ConsoleCommand::new(
                "bind",
                "bind <list|get|set|remove> [key] [command...] - manage key bindings",
                bind_cmd,
            )
            .with_summary("Manage runtime key bindings")
            .with_args([
                ArgumentSpec::new("operation"),
                ArgumentSpec::new("key").help("Key binding, e.g. meta+KeyW or F1"),
                ArgumentSpec::new("command").help("Command to run"),
            ])
            .with_completions(complete_bind),
        );
    }
}

fn clear_cmd(
    In(args): CommandArgs,
    mut buffer: ResMut<ConsoleBuffer>,
    #[cfg(feature = "persistent-history")] mut state: ResMut<crate::ConsoleState>,
) -> ConsoleResult {
    buffer.clear();
    #[cfg(feature = "persistent-history")]
    if args.len() == 1
        && args
            .get(0)
            .is_some_and(|arg| arg.eq_ignore_ascii_case("--history"))
    {
        state.clear_command_history();
    }
    #[cfg(not(feature = "persistent-history"))]
    let _ = args;
    ConsoleResult::default()
}

fn help_cmd(In(args): CommandArgs, registry: Res<ConsoleRegistry>) -> ConsoleResult {
    if let Some(name) = args.get(0) {
        return registry.get(name).map_or_else(
            || ConsoleResult::error(format!("Unknown command: {name}")),
            |def| {
                let help = def.spec.long_help.unwrap_or(def.spec.summary);
                let mut lines = vec![def.spec.usage.to_string()];
                if !help.is_empty() && help != def.spec.usage {
                    lines.push(help.to_string());
                }
                if !def.spec.aliases.is_empty() {
                    lines.push(format!("Aliases: {}", def.spec.aliases.join(", ")));
                }
                ConsoleResult::info(lines.join("\n"))
            },
        );
    }

    ConsoleResult::info(
        registry
            .commands
            .values()
            .filter(|def| !def.spec.hidden)
            .map(|def| def.spec.usage)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn alias_cmd(
    In(args): CommandArgs,
    registry: Res<ConsoleRegistry>,
    mut aliases: ResMut<ConsoleAliases>,
) -> ConsoleResult {
    let Some(operation) = args.get(0) else {
        return ConsoleResult::error("Usage: alias <list|get|set|remove> [name] [command...]");
    };
    match operation.to_ascii_lowercase().as_str() {
        "list" if args.len() == 1 => {
            let aliases = aliases
                .iter()
                .map(|(name, expansion)| format!("{name} = {expansion}"))
                .collect::<Vec<_>>();
            if aliases.is_empty() {
                ConsoleResult::info("No runtime aliases defined")
            } else {
                ConsoleResult::info(aliases.join("\n"))
            }
        }
        "get" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: alias get <name>");
            };
            if args.len() != 2 {
                return ConsoleResult::error("Usage: alias get <name>");
            }
            aliases.get(name).map_or_else(
                || ConsoleResult::error(format!("Unknown alias: {name}")),
                |expansion| ConsoleResult::info(format!("{name} = {expansion}")),
            )
        }
        "set" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: alias set <name> <command...>");
            };
            if args.len() < 3 {
                return ConsoleResult::error("Usage: alias set <name> <command...>");
            }
            if registry.contains(name) {
                return ConsoleResult::error(format!(
                    "Cannot create alias `{}`: it is already a registered command",
                    name
                ));
            }
            let expansion = args.raw_rest(2);
            aliases.set(name, &expansion);
            ConsoleResult::info(format!("{name} = {expansion}"))
        }
        "remove" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: alias remove <name>");
            };
            if args.len() != 2 {
                return ConsoleResult::error("Usage: alias remove <name>");
            }
            if aliases.remove(name).is_some() {
                ConsoleResult::info(format!("Removed alias: {name}"))
            } else {
                ConsoleResult::error(format!("Unknown alias: {name}"))
            }
        }
        _ => ConsoleResult::error("Usage: alias <list|get|set|remove> [name] [command...]"),
    }
}

fn complete_alias(
    In(request): ConsoleCompletionRequest,
    aliases: Res<ConsoleAliases>,
    registry: Res<ConsoleRegistry>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => complete_alias_operations(),
        1 => complete_alias_names(&request, &aliases),
        2 => complete_commands_after_set(&request, &registry, &aliases),
        _ => Vec::new(),
    }
}

fn complete_alias_operations() -> Vec<CompletionItem> {
    static_completion_items([
        ("list", "Lists runtime aliases"),
        ("get", "Shows an alias"),
        ("set", "Creates or updates an alias"),
        ("remove", "Removes an alias"),
    ])
}

fn complete_alias_names(
    request: &CompletionRequest,
    aliases: &ConsoleAliases,
) -> Vec<CompletionItem> {
    if !request.argument(0).is_some_and(|operation| {
        operation.eq_ignore_ascii_case("get") || operation.eq_ignore_ascii_case("remove")
    }) {
        return Vec::new();
    }
    aliases
        .iter()
        .map(|(name, expansion)| CompletionItem::new(name, expansion))
        .collect()
}

fn bind_cmd(In(args): CommandArgs, mut binds: ResMut<ConsoleBinds>) -> ConsoleResult {
    let Some(operation) = args.get(0) else {
        return ConsoleResult::error("Usage: bind <list|get|set|remove> [key] [command...]");
    };
    match operation.to_ascii_lowercase().as_str() {
        "list" if args.len() == 1 => {
            let binds = binds
                .iter()
                .map(|(binding, command)| format!("{binding} = {command}"))
                .collect::<Vec<_>>();
            if binds.is_empty() {
                ConsoleResult::info("No runtime key bindings defined")
            } else {
                ConsoleResult::info(binds.join("\n"))
            }
        }
        "get" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: bind get <key>");
            };
            if args.len() != 2 {
                return ConsoleResult::error("Usage: bind get <key>");
            }
            let Some(binding) = parse_key_binding(name) else {
                return ConsoleResult::error(format!("Unknown key: {name}"));
            };
            binds.get_binding(binding).map_or_else(
                || ConsoleResult::error(format!("No binding for {binding}")),
                |command| ConsoleResult::info(format!("{binding} = {command}")),
            )
        }
        "set" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: bind set <key> <command...>");
            };
            if args.len() < 3 {
                return ConsoleResult::error("Usage: bind set <key> <command...>");
            }
            let Some(binding) = parse_key_binding(name) else {
                return ConsoleResult::error(format!("Unknown key: {name}"));
            };
            let command = args.raw_rest(2);
            binds.set_binding(binding, &command);
            ConsoleResult::info(format!("{binding} = {command}"))
        }
        "remove" => {
            let Some(name) = args.get(1) else {
                return ConsoleResult::error("Usage: bind remove <key>");
            };
            if args.len() != 2 {
                return ConsoleResult::error("Usage: bind remove <key>");
            }
            let Some(binding) = parse_key_binding(name) else {
                return ConsoleResult::error(format!("Unknown key: {name}"));
            };
            if binds.remove_binding(binding).is_some() {
                ConsoleResult::info(format!("Removed binding: {binding}"))
            } else {
                ConsoleResult::error(format!("No binding for {binding}"))
            }
        }
        _ => ConsoleResult::error("Usage: bind <list|get|set|remove> [key] [command...]"),
    }
}

fn complete_bind(
    In(request): ConsoleCompletionRequest,
    registry: Res<ConsoleRegistry>,
    aliases: Res<ConsoleAliases>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => complete_bind_operations(),
        2 => complete_commands_after_set(&request, &registry, &aliases),
        _ => Vec::new(),
    }
}

fn complete_bind_operations() -> Vec<CompletionItem> {
    static_completion_items([
        ("list", "Lists runtime key bindings"),
        ("get", "Shows a key binding"),
        ("set", "Creates or updates a key binding"),
        ("remove", "Removes a key binding"),
    ])
}

fn complete_commands_after_set(
    request: &CompletionRequest,
    registry: &ConsoleRegistry,
    aliases: &ConsoleAliases,
) -> Vec<CompletionItem> {
    if !request
        .argument(0)
        .is_some_and(|operation| operation.eq_ignore_ascii_case("set"))
    {
        return Vec::new();
    }
    runtime_command_completions(registry, aliases)
}

fn parse_key_binding(input: &str) -> Option<ConsoleKeyBinding> {
    let mut modifiers = ConsoleKeyModifiers::default();
    let mut key = None;
    for part in input.split('+') {
        if part.is_empty() {
            return None;
        }
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" if !modifiers.ctrl => modifiers.ctrl = true,
            "meta" | "cmd" | "command" if !modifiers.meta => modifiers.meta = true,
            "shift" if !modifiers.shift => modifiers.shift = true,
            "alt" if !modifiers.alt => modifiers.alt = true,
            _ => {
                let parsed = parse_keycode(part)?;
                if key.replace(parsed).is_some() {
                    return None;
                }
            }
        }
    }
    key.map(|key| ConsoleKeyBinding { key, modifiers })
}

fn parse_keycode(name: &str) -> Option<KeyCode> {
    let shorthand = match name {
        name if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", name.to_ascii_uppercase())
        }
        name if name.len() == 1 && name.as_bytes()[0].is_ascii_digit() => {
            format!("Digit{name}")
        }
        _ => name.to_string(),
    };
    let variant = KeyCode::type_info()
        .as_enum()
        .expect("KeyCode must remain an enum")
        .iter()
        .find(|variant| variant.name().eq_ignore_ascii_case(&shorthand))
        .filter(|variant| variant.as_unit_variant().is_ok())?;
    KeyCode::from_reflect(&DynamicEnum::new(variant.name(), ()))
}

#[cfg(test)]
mod tests {
    use super::{help_cmd, parse_key_binding, plugin};
    use crate::model::CommandSpec;
    use crate::{
        Args, BuiltinCommand, BuiltinCommands, CommandArgs, ConsoleAliases, ConsoleBinds,
        ConsoleBuffer, ConsoleCommandExecuted, ConsoleCommandQueue, ConsoleConfig,
        ConsoleKeyBinding, ConsoleKeyModifiers, ConsoleLevel, ConsoleRegistry, ConsoleRequest,
        ConsoleResult, ConsoleState,
    };
    use bevy::prelude::*;

    fn noop(In(_args): CommandArgs) -> ConsoleResult {
        ConsoleResult::default()
    }

    #[test]
    fn help_omits_duplicated_usage_but_shows_an_explicit_summary() {
        let mut world = World::new();
        world.init_resource::<ConsoleRegistry>();
        let echo = world.register_system(noop);
        let described_echo = world.register_system(noop);
        let help = world.register_system(help_cmd);
        {
            let mut registry = world.resource_mut::<ConsoleRegistry>();
            registry.register(CommandSpec::new("echo", "echo <text>"), echo, None);
            registry.register(
                CommandSpec {
                    summary: "Echo text to the console",
                    ..CommandSpec::new("described-echo", "described-echo <text>")
                },
                described_echo,
                None,
            );
        }

        assert_eq!(
            world
                .run_system_with(help, Args::from(vec!["echo".to_string()]))
                .unwrap()
                .lines,
            vec![(ConsoleLevel::Info, "echo <text>".to_string())]
        );
        assert_eq!(
            world
                .run_system_with(help, Args::from(vec!["described-echo".to_string()]))
                .unwrap()
                .lines,
            vec![(
                ConsoleLevel::Info,
                "described-echo <text>\nEcho text to the console".to_string(),
            )]
        );
    }

    #[test]
    fn runtime_aliases_cannot_shadow_registered_commands() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::from([
                BuiltinCommand::Alias,
                BuiltinCommand::Clear,
            ]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(plugin);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("alias set clear echo ignored"));
        crate::execution::execute_pending_commands(app.world_mut());

        assert!(
            app.world()
                .resource::<ConsoleAliases>()
                .get("clear")
                .is_none()
        );
        assert_eq!(
            app.world().resource::<ConsoleBuffer>().lines()[1].text,
            "Cannot create alias `clear`: it is already a registered command"
        );
        assert_eq!(
            app.world().resource::<ConsoleBuffer>().lines()[1].level,
            ConsoleLevel::Error
        );
        let messages = app.world().resource::<Messages<ConsoleCommandExecuted>>();
        let mut cursor = messages.get_cursor();
        assert!(!cursor.read(messages).next().unwrap().succeeded);
    }

    #[test]
    fn alias_operations_manage_runtime_aliases() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::from([BuiltinCommand::Alias]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(plugin);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("alias set quicksave save slot_1"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleAliases>().get("quicksave"),
            Some("save slot_1")
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("alias get quicksave"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .text,
            "quicksave = save slot_1"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("alias remove quicksave"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(
            app.world()
                .resource::<ConsoleAliases>()
                .get("quicksave")
                .is_none()
        );
    }

    #[test]
    fn builtins_can_be_selected_individually() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::from([BuiltinCommand::Help]))
            .add_plugins(plugin);

        let registry = app.world().resource::<ConsoleRegistry>();
        assert!(registry.get("help").is_some());
        assert!(registry.get("clear").is_none());
        assert!(registry.get("get").is_none());
    }

    #[test]
    fn default_builtins_are_help_and_clear() {
        let commands = BuiltinCommands::default();

        assert!(commands.contains(&BuiltinCommand::Help));
        assert!(commands.contains(&BuiltinCommand::Clear));
        assert!(!commands.contains(&BuiltinCommand::Alias));
        assert!(!commands.contains(&BuiltinCommand::Bind));
    }

    #[test]
    fn builtins_can_be_created_from_an_iterator() {
        let commands = BuiltinCommands::from(
            [BuiltinCommand::Help, BuiltinCommand::Alias]
                .into_iter()
                .filter(|command| *command != BuiltinCommand::Help),
        );

        assert!(!commands.contains(&BuiltinCommand::Help));
        assert!(commands.contains(&BuiltinCommand::Alias));
    }

    #[cfg(feature = "persistent-history")]
    #[test]
    fn clear_history_flag_clears_command_recall() {
        let mut world = World::new();
        world.insert_resource(ConsoleBuffer::default());
        world.insert_resource(ConsoleState {
            cmd_history: vec!["map forest".into()],
            cmd_history_index: Some(0),
            cmd_history_draft: "draft".into(),
            command_history_revision: 7,
            ..default()
        });
        let clear = world.register_system(super::clear_cmd);

        world
            .run_system_with(clear, Args::from(vec!["--history".into()]))
            .unwrap();

        let state = world.resource::<ConsoleState>();
        assert!(state.cmd_history.is_empty());
        assert_eq!(state.cmd_history_index, None);
        assert!(state.cmd_history_draft.is_empty());
        assert_eq!(state.command_history_revision, 8);
    }

    #[test]
    fn bind_operations_manage_runtime_key_bindings() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::from([BuiltinCommand::Bind]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(plugin);

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("bind set f1 echo hello"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleBinds>().get(KeyCode::F1),
            Some("echo hello")
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("bind set ctrl+w echo save"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBinds>()
                .get_binding(ConsoleKeyBinding {
                    key: KeyCode::KeyW,
                    modifiers: ConsoleKeyModifiers {
                        ctrl: true,
                        ..default()
                    },
                }),
            Some("echo save")
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("bind get ctrl+w"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .text,
            "ctrl+KeyW = echo save"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("bind remove F1"));
        crate::execution::execute_pending_commands(app.world_mut());
        assert!(
            app.world()
                .resource::<ConsoleBinds>()
                .get(KeyCode::F1)
                .is_none()
        );
    }

    #[test]
    fn binding_parser_supports_combined_modifiers_and_rejects_invalid_chords() {
        assert_eq!(
            parse_key_binding("meta+shift+w"),
            Some(ConsoleKeyBinding {
                key: KeyCode::KeyW,
                modifiers: ConsoleKeyModifiers {
                    meta: true,
                    shift: true,
                    ..default()
                },
            })
        );
        assert_eq!(parse_key_binding("cmd+w"), parse_key_binding("meta+w"));
        assert!(parse_key_binding("ctrl+shift").is_none());
        assert!(parse_key_binding("meta+meta+w").is_none());
        assert!(parse_key_binding("ctrl+ctrl+w").is_none());
        assert!(parse_key_binding("w+e").is_none());
        assert!(parse_key_binding("ctrl++w").is_none());
    }
}
