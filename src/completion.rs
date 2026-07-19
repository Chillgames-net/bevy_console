//! Completion generation for command names and command arguments.

use crate::ui::ConsoleInput;
use crate::{
    ArgumentKind, CompletionItem, CompletionRequest, ConsoleAliases, ConsoleConfig,
    ConsoleRegistry, ConsoleState, ParsedInput,
};
use bevy::prelude::*;
use bevy::text::EditableText;

pub(crate) fn has_dirty_completion(state: Option<Res<ConsoleState>>) -> bool {
    state.is_some_and(|state| state.completion_dirty)
}

/// Rebuilds suggestions after input changes. This is exclusive because dynamic
/// completion providers are normal registered Bevy systems and can query any
/// game resource or entity.
pub(crate) fn refresh_completions(world: &mut World) {
    let (input, completion_cursor) = {
        let mut state = world.resource_mut::<ConsoleState>();
        (state.input.clone(), state.completion_cursor.take())
    };
    let cursor = completion_cursor.unwrap_or_else(|| {
        world
            .query_filtered::<&EditableText, With<ConsoleInput>>()
            .iter(world)
            .next()
            .map_or(input.len(), |input| {
                input.editor().raw_selection().focus().index()
            })
    });
    let parsed = ParsedInput::parse_at(input, cursor);
    let items = match parsed.active_argument_index() {
        None => command_completions(
            world.resource::<ConsoleRegistry>(),
            world.resource::<ConsoleAliases>(),
            &parsed,
        ),
        Some(argument_index) => argument_completions(world, &parsed, argument_index),
    };
    let max_suggestions = world.resource::<ConsoleConfig>().max_suggestions;
    let overflow = items.len().saturating_sub(max_suggestions);
    let items = if max_suggestions == 0 {
        Vec::new()
    } else {
        items
    };
    world
        .resource_mut::<ConsoleState>()
        .set_completions(items, overflow);
}

fn command_completions(
    registry: &ConsoleRegistry,
    aliases: &ConsoleAliases,
    parsed: &ParsedInput,
) -> Vec<CompletionItem> {
    rank_completion_items(
        runtime_command_completions(registry, aliases, parsed.replacement_range()),
        parsed.active_fragment(),
    )
}

pub(crate) fn runtime_command_completions(
    registry: &ConsoleRegistry,
    aliases: &ConsoleAliases,
    replace: std::ops::Range<usize>,
) -> Vec<CompletionItem> {
    let mut items = registry
        .commands
        .values()
        .filter(|def| !def.spec.hidden)
        .map(|def| {
            let mut item = CompletionItem::new(def.spec.name.clone(), replace.clone());
            item.detail = def.spec.summary.to_string();
            item
        })
        .collect::<Vec<_>>();
    items.extend(
        registry
            .commands
            .values()
            .filter(|def| !def.spec.hidden)
            .flat_map(|def| {
                def.spec.aliases.iter().map(|alias| {
                    let mut item = CompletionItem::new(*alias, replace.clone());
                    item.detail = format!("alias for {} - {}", def.spec.name, def.spec.summary);
                    item
                })
            }),
    );
    items.extend(aliases.iter().map(|(name, expansion)| {
        let mut item = CompletionItem::new(name, replace.clone());
        item.detail = format!("runtime alias - {expansion}");
        item
    }));
    items
}

/// Builds completion items whose labels and descriptions are both static.
///
/// Built-in command completers use this for their fixed operation lists.
pub(crate) fn static_completion_items(
    request: &CompletionRequest,
    items: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> Vec<CompletionItem> {
    items
        .into_iter()
        .map(|(label, detail)| {
            let mut item = CompletionItem::new(label, request.parsed.replacement_range());
            item.detail = detail.into();
            item
        })
        .collect()
}

fn argument_completions(
    world: &mut World,
    parsed: &ParsedInput,
    argument_index: usize,
) -> Vec<CompletionItem> {
    let Some(command) = parsed.command() else {
        return Vec::new();
    };
    let (argument, completer) = {
        let registry = world.resource::<ConsoleRegistry>();
        let Some(def) = registry.get(command) else {
            return Vec::new();
        };
        (
            def.spec.args.get(argument_index).cloned(),
            def.completers.get(&argument_index).copied(),
        )
    };

    if let Some(completer) = completer {
        let request = CompletionRequest {
            parsed: parsed.clone(),
        };
        return match world.run_system_with(completer, request) {
            Ok(items) => rank_completion_items(items, parsed.active_fragment()),
            Err(error) => {
                warn!("chill_bevy_console: command completer failed: {error}");
                Vec::new()
            }
        };
    }

    let Some(argument) = argument else {
        return Vec::new();
    };
    let choices: Vec<String> = match argument.kind {
        ArgumentKind::Boolean => vec!["true".into(), "false".into()],
        ArgumentKind::Choice => argument
            .choices
            .iter()
            .map(|choice| (*choice).to_string())
            .collect(),
        _ => Vec::new(),
    };
    let detail = if argument.help.is_empty() {
        argument.name.to_string()
    } else {
        format!("{} - {}", argument.name, argument.help)
    };
    rank_completion_items(
        choices
            .into_iter()
            .map(|choice| {
                let mut item = CompletionItem::new(choice, parsed.replacement_range());
                item.detail = detail.clone();
                item
            })
            .collect(),
        parsed.active_fragment(),
    )
}

fn rank_completion_items(items: Vec<CompletionItem>, fragment: &str) -> Vec<CompletionItem> {
    let mut ranked = items
        .into_iter()
        .filter_map(|item| match_rank(&item.label, fragment).map(|rank| (rank, item)))
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_rank, left), (right_rank, right)| {
        left_rank
            .cmp(right_rank)
            .then_with(|| left.label.cmp(&right.label))
    });
    ranked.into_iter().map(|(_, item)| item).collect()
}

/// Prefix matches are ranked first, then case-insensitive substring matches.
fn match_rank(candidate: &str, fragment: &str) -> Option<u8> {
    if fragment.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_ascii_lowercase();
    let fragment = fragment.to_ascii_lowercase();
    if candidate.starts_with(&fragment) {
        Some(0)
    } else if candidate.contains(&fragment) {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{has_dirty_completion, match_rank, refresh_completions};
    use crate::ui::ConsoleInput;
    use crate::{
        ArgumentSpec, BuiltinCommand, CommandArgs, CommandSpec, CompletionItem, CompletionRequest,
        ConsoleAliases, ConsoleAppExt, ConsoleConfig, ConsoleRegistry, ConsoleState,
    };
    use bevy::prelude::*;
    use bevy::text::EditableText;

    #[test]
    fn prefix_beats_substring() {
        assert!(match_rank("map", "ma") < match_rank("r_map", "ma"));
    }

    fn noop(In(_args): CommandArgs) -> String {
        String::new()
    }

    #[derive(Resource)]
    struct Levels(Vec<&'static str>);

    fn level_completer(
        In(request): In<CompletionRequest>,
        levels: Res<Levels>,
    ) -> Vec<CompletionItem> {
        levels
            .0
            .iter()
            .map(|level| CompletionItem::new(*level, request.parsed.replacement_range()))
            .collect()
    }

    #[test]
    fn completes_static_and_dynamic_arguments_through_bevy_systems() {
        let mut app = App::new();
        app.insert_resource(Levels(vec!["forest", "fortress"]));
        app.insert_resource(ConsoleState::default());
        app.insert_resource(ConsoleConfig::default());
        app.init_resource::<ConsoleAliases>();
        app.add_console_command_spec(
            CommandSpec::new("quality")
                .help("quality <level>")
                .args([ArgumentSpec::new("level").choices(["low", "medium", "high"])]),
            noop,
        )
        .add_console_command_spec(
            CommandSpec::new("map")
                .help("map <name>")
                .args([ArgumentSpec::new("name")]),
            noop,
        )
        .add_console_command_spec(
            CommandSpec::new("teleport")
                .help("teleport <target> <mode>")
                .args([
                    ArgumentSpec::new("target"),
                    ArgumentSpec::new("mode").choices(["walk", "snap"]),
                ]),
            noop,
        )
        .add_console_completer("map", 0, level_completer);

        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "quality m".into();
            state.mark_input_changed();
        }
        refresh_completions(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleState>().completion_items[0].label,
            "medium"
        );

        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "map fo".into();
            state.mark_input_changed();
        }
        refresh_completions(app.world_mut());
        let state = app.world().resource::<ConsoleState>();
        assert_eq!(
            state
                .completion_items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["forest", "fortress"]
        );
        assert!(app.world().contains_resource::<ConsoleRegistry>());

        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "teleport player w".into();
            state.mark_input_changed();
        }
        refresh_completions(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleState>().completion_items[0].label,
            "walk"
        );
    }

    #[test]
    fn dynamic_completer_overrides_static_choices() {
        let mut app = App::new();
        app.insert_resource(Levels(vec!["forest", "fortress"]))
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig::default())
            .init_resource::<ConsoleAliases>()
            .add_console_command_spec(
                CommandSpec::new("map")
                    .help("map <name>")
                    .args([ArgumentSpec::new("name").choices(["static_map"])]),
                noop,
            )
            .add_console_completer("map", 0, level_completer);

        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "map ".into();
            state.mark_input_changed();
        }
        refresh_completions(app.world_mut());

        assert_eq!(
            app.world()
                .resource::<ConsoleState>()
                .completion_items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["forest", "fortress"]
        );
    }

    #[test]
    fn accepted_completion_refreshes_for_the_next_argument_before_editor_updates() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig::default())
            .init_resource::<ConsoleAliases>()
            .add_console_command_spec(
                CommandSpec::new("teleport")
                    .help("teleport <target> <mode>")
                    .args([
                        ArgumentSpec::new("target").choices(["player"]),
                        ArgumentSpec::new("mode").choices(["walk", "snap"]),
                    ]),
                noop,
            );
        // Simulate the editor retaining its old cursor until queued edits are
        // applied later in the update cycle.
        app.world_mut()
            .spawn((ConsoleInput, EditableText::new("teleport pla")));
        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "teleport pla".into();
            state.completion_items = vec![CompletionItem::new("player", 9..12)];
            assert_eq!(state.apply_selected_completion(), Some(16));
        }

        refresh_completions(app.world_mut());

        let state = app.world().resource::<ConsoleState>();
        assert_eq!(state.input, "teleport player ");
        assert_eq!(
            state
                .completion_items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["snap", "walk"]
        );
    }

    #[test]
    fn completion_hides_hidden_commands() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig::default())
            .init_resource::<ConsoleAliases>()
            .add_console_command("visible", "visible", noop)
            .add_console_command_spec(CommandSpec::new("hidden").help("hidden").hidden(), noop);
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .mark_input_changed();
        refresh_completions(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleState>()
                .completion_items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["visible"]
        );
    }

    #[test]
    fn retains_completions_beyond_the_first_suggestion_page() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig {
                max_suggestions: 2,
                ..default()
            })
            .init_resource::<ConsoleAliases>()
            .add_console_command("alpha", "alpha", noop)
            .add_console_command("beta", "beta", noop)
            .add_console_command("gamma", "gamma", noop);
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .mark_input_changed();

        refresh_completions(app.world_mut());

        let state = app.world().resource::<ConsoleState>();
        assert_eq!(state.completion_items[0].label, "alpha");
        assert_eq!(state.completion_items[1].label, "beta");
        assert_eq!(state.completion_items[2].label, "gamma");
        assert_eq!(state.completion_overflow, 1);
    }

    #[test]
    fn zero_suggestion_limit_disables_completions() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig {
                max_suggestions: 0,
                ..default()
            })
            .init_resource::<ConsoleAliases>()
            .add_console_command("alpha", "alpha", noop);
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .mark_input_changed();

        refresh_completions(app.world_mut());

        let state = app.world().resource::<ConsoleState>();
        assert!(state.completion_items.is_empty());
        assert_eq!(state.completion_overflow, 1);
    }

    #[test]
    fn alias_completes_registered_commands() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .insert_resource(ConsoleConfig::default())
            .insert_resource(crate::BuiltinCommands::from([BuiltinCommand::Alias]))
            .init_resource::<ConsoleAliases>();
        crate::commands::plugin(&mut app);
        app.add_console_command("save", "save - save the game", noop)
            .add_console_command_spec(CommandSpec::new("hidden").help("hidden").hidden(), noop);
        app.add_systems(Update, refresh_completions.run_if(has_dirty_completion));

        {
            let mut state = app.world_mut().resource_mut::<ConsoleState>();
            state.input = "alias set quick sa".into();
            state.mark_input_changed();
        }
        app.update();

        let state = app.world().resource::<ConsoleState>();
        assert_eq!(
            state
                .completion_items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["save"]
        );
    }
}
