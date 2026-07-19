use crate::config::{BuiltinCommand, BuiltinCommands, ConsoleConfig};
use crate::editor::set_editable_text;
use crate::state::ConsoleState;
use crate::ui::ConsoleInput;
use crate::{ConsoleBinds, ConsoleCommandQueue, ConsoleRequest};
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::ScrollPosition;

#[derive(SystemParam)]
pub(crate) struct ConsoleInputSettings<'w> {
    config: Res<'w, ConsoleConfig>,
    builtin_commands: Res<'w, BuiltinCommands>,
}

// ── Run conditions ────────────────────────────────────────────────────────────

pub(crate) fn console_open(state: Option<Res<ConsoleState>>) -> bool {
    state.is_some_and(|s| s.open)
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Handles the toggle key and the force-close-when-disabled case.
/// Only mutates `state.open` — UI sync is handled by [`sync_console_ui`].
pub(crate) fn handle_toggle_key(
    config: Res<ConsoleConfig>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ConsoleState>,
) {
    if !state.enabled {
        if state.open {
            state.open = false;
        }
        return;
    }

    if keys.just_pressed(config.toggle_key) {
        state.open = !state.open;
    }
}

/// Keeps the public console input string and Bevy's text editor in sync.
pub(crate) fn sync_console_input(
    mut state: ResMut<ConsoleState>,
    mut input_q: Query<&mut EditableText, With<ConsoleInput>>,
    mut last_synced: Local<Option<String>>,
) {
    let Ok(mut input) = input_q.single_mut() else {
        *last_synced = None;
        return;
    };
    let edited = input.value().to_string();
    if edited != state.input {
        if last_synced.as_ref() != Some(&state.input) {
            set_editable_text(&mut input, &state.input, state.input.len());
        } else {
            state.set_input(edited);
            state.cmd_history_index = None;
            state.cmd_history_draft.clear();
        }
    }
    if input.is_changed() {
        state.set_changed();
    }
    *last_synced = Some(state.input.clone());
}

pub(crate) fn capture_console_input(
    mut key_events: MessageReader<KeyboardInput>,
    mut state: ResMut<ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: ConsoleInputSettings,
    mut queue: ResMut<ConsoleCommandQueue>,
    mut input_q: Query<&mut EditableText, With<ConsoleInput>>,
    mut history_q: Query<&mut ScrollPosition, With<crate::ui::ConsoleHistory>>,
) {
    if !state.open {
        // This system runs while closed so its reader stays current. Otherwise
        // an Escape, Enter, or shortcut from before opening could replay.
        key_events.read().for_each(drop);
        return;
    }

    let Ok(mut input) = input_q.single_mut() else {
        key_events.read().for_each(drop);
        return;
    };
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }

        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let meta = if cfg!(target_os = "macos") {
            keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)
        } else {
            control
        };
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

        if ev.key_code == settings.config.toggle_key {
            continue;
        }
        if input.is_composing() {
            continue;
        }

        if meta && ev.key_code == KeyCode::Backspace {
            state.clear_input();
            state.clear_completions();
            set_editable_text(&mut input, "", 0);
            continue;
        }

        match &ev.logical_key {
            Key::Character(c) if control && c.eq_ignore_ascii_case("r") => {
                state.recall_history_matching_input();
                set_editable_text(&mut input, &state.input, state.input.len());
                continue;
            }
            Key::Character(c) if control && c.eq_ignore_ascii_case("u") => {
                state.clear_input();
                set_editable_text(&mut input, "", 0);
                continue;
            }
            Key::Character(c)
                if control
                    && c.eq_ignore_ascii_case("l")
                    && settings.builtin_commands.contains(&BuiltinCommand::Clear) =>
            {
                queue.push(ConsoleRequest {
                    input: "clear".into(),
                    origin: crate::CommandOrigin::LocalUi,
                });
                state.clear_input();
                state.clear_completions();
                set_editable_text(&mut input, "", 0);
                continue;
            }
            Key::Enter => {
                submit_console_input(
                    &mut state,
                    &settings.config,
                    &mut queue,
                    &mut input,
                    &mut history_q,
                );
                continue;
            }
            // iOS emits Return from the software keyboard as a character
            // insertion ("\\n") instead of `Key::Enter`.
            Key::Character(c) if c == "\n" || c == "\r" => {
                submit_console_input(
                    &mut state,
                    &settings.config,
                    &mut queue,
                    &mut input,
                    &mut history_q,
                );
                continue;
            }
            Key::Tab => {
                if shift && !state.completion_items.is_empty() {
                    state.select_previous_completion();
                    continue;
                }
                if let Some(cursor) = state.apply_selected_completion() {
                    state.cmd_history_index = None;
                    state.cmd_history_draft.clear();
                    set_editable_text(&mut input, &state.input, cursor);
                    continue;
                }
            }
            Key::ArrowUp => {
                // The focused text widget queues its own vertical cursor move
                // before this system receives the key. Up/Down belong to the
                // console's completion and history navigation instead.
                discard_vertical_cursor_moves(&mut input);
                if state.cmd_history_index.is_none()
                    && !state.completion_items.is_empty()
                    && state.match_index > 0
                {
                    state.select_previous_completion();
                    continue;
                }
                // At the first suggestion, Up enters history browsing. Once
                // browsing, history keeps priority over completion results for
                // recalled commands.
                if state.cmd_history.is_empty() {
                    continue;
                }
                let search_end = state.cmd_history_index.unwrap_or(state.cmd_history.len());
                let previous = state.cmd_history[..search_end]
                    .iter()
                    .rposition(|command| command != &state.input);
                if let Some(idx) = previous {
                    if state.cmd_history_index.is_none() {
                        // Start browsing: save the live input as a draft.
                        state.cmd_history_draft = state.input.clone();
                    }
                    state.cmd_history_index = Some(idx);
                    let value = state.cmd_history[idx].clone();
                    sync_history_selection(&mut state, &mut input, value);
                }
                continue;
            }
            Key::ArrowDown => {
                discard_vertical_cursor_moves(&mut input);
                if state.cmd_history_index.is_none() && !state.completion_items.is_empty() {
                    state.select_next_completion();
                    continue;
                }
                // Command history: go to the next distinct entry or restore
                // the draft after the newest distinct entry.
                match state.cmd_history_index {
                    None => { /* not browsing — nothing to do */ }
                    Some(i) => {
                        let next = state.cmd_history[i + 1..]
                            .iter()
                            .position(|command| command != &state.input)
                            .map(|offset| i + 1 + offset);
                        if let Some(idx) = next {
                            state.cmd_history_index = Some(idx);
                            let value = state.cmd_history[idx].clone();
                            sync_history_selection(&mut state, &mut input, value);
                        } else {
                            // Past the newest distinct entry: restore the draft.
                            state.cmd_history_index = None;
                            let value = state.cmd_history_draft.clone();
                            sync_history_selection(&mut state, &mut input, value);
                            state.cmd_history_draft.clear();
                        }
                    }
                }
                continue;
            }
            Key::End if control => {
                state.scroll_follow = true;
                continue;
            }
            Key::Escape => {
                if state.completion_items.is_empty() {
                    state.open = false;
                } else {
                    state.clear_completions();
                }
                continue;
            }
            _ => {}
        }
    }
}

fn submit_console_input(
    state: &mut ConsoleState,
    config: &ConsoleConfig,
    queue: &mut ConsoleCommandQueue,
    input: &mut EditableText,
    history_q: &mut Query<&mut ScrollPosition, With<crate::ui::ConsoleHistory>>,
) {
    let cmd = state.input.trim().to_string();
    if !cmd.is_empty() {
        queue.push(ConsoleRequest {
            input: cmd.clone(),
            origin: crate::CommandOrigin::LocalUi,
        });
        state.record_command(cmd, config.max_command_history);
    } else if config.close_on_empty_submit {
        state.open = false;
    }
    state.clear_input();
    set_editable_text(input, "", 0);
    state.clear_completions();
    state.scroll_follow = true;
    if let Ok(mut scroll_pos) = history_q.single_mut() {
        // Reset immediately, including when the command itself produces no
        // output and therefore does not trigger a later history UI refresh.
        scroll_pos.y = f32::MAX;
    }
    state.cmd_history_index = None;
    state.cmd_history_draft.clear();
}

fn sync_history_selection(state: &mut ConsoleState, input: &mut EditableText, value: String) {
    set_editable_text(input, &value, value.len());
    state.set_input(value);
}

fn discard_vertical_cursor_moves(input: &mut EditableText) {
    input
        .pending_edits
        .retain(|edit| !matches!(edit, TextEdit::Up(_) | TextEdit::Down(_)));
}

/// Queues commands assigned with `bind` while the console is closed.
pub(crate) fn queue_bound_commands(
    state: Res<ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    binds: Res<ConsoleBinds>,
    mut queue: ResMut<ConsoleCommandQueue>,
) {
    if state.open {
        return;
    }

    for (binding, command) in binds.iter() {
        if keys.just_pressed(binding.key) && binding.modifiers.matches(&keys) {
            queue.push(ConsoleRequest {
                input: command.into(),
                origin: crate::CommandOrigin::LocalUi,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{capture_console_input, queue_bound_commands};
    use crate::execution::execute_pending_commands;
    use crate::ui::{ConsoleAssets, ConsoleHistory, ConsoleInput, sync_console_ui};
    use crate::{
        BuiltinCommand, BuiltinCommands, CommandArgs, ConsoleAliases, ConsoleAppExt, ConsoleBinds,
        ConsoleBuffer, ConsoleCommandQueue, ConsoleConfig, ConsoleKeyBinding, ConsoleKeyModifiers,
        ConsoleLevel, ConsoleLineSource, ConsoleRequest, ConsoleResult, ConsoleState,
    };
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::prelude::*;
    use bevy::text::{EditableText, TextEdit};

    fn echo(In(args): CommandArgs) -> String {
        args.join("|")
    }

    fn structured(In(_args): CommandArgs) -> ConsoleResult {
        ConsoleResult::info("all good").line(ConsoleLevel::Warn, "watch out")
    }

    fn command_test_app(builtins: impl Into<BuiltinCommands>) -> App {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(builtins.into())
            .insert_resource(ConsoleState::default())
            .insert_resource(ConsoleBuffer::default())
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<crate::ConsoleCommandExecuted>()
            .add_console_command(crate::ConsoleCommand::new("echo", "echo <text>", echo))
            .add_console_command(crate::ConsoleCommand::new("status", "status", structured))
            .add_plugins(crate::commands::plugin);
        app
    }

    #[test]
    fn opening_console_requests_completions_for_empty_input() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(ConsoleState {
                open: true,
                ..default()
            })
            .insert_resource(ConsoleAssets {
                font: Handle::default(),
            })
            .add_systems(Update, sync_console_ui);

        app.update();

        assert!(app.world().resource::<ConsoleState>().completion_dirty);
    }

    #[test]
    fn up_from_first_suggestion_browses_history_until_draft_is_restored() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                completion_items: vec![
                    crate::CompletionItem::from("alpha").with_replace(0..0),
                    crate::CompletionItem::from("beta").with_replace(0..0),
                ],
                cmd_history: vec!["echo older".into(), "echo newer".into()],
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut().spawn((ConsoleInput, EditableText::new("")));

        for (key, expected) in [
            (Key::ArrowUp, "echo newer"),
            (Key::ArrowUp, "echo older"),
            (Key::ArrowDown, "echo newer"),
            (Key::ArrowDown, ""),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code: match &key {
                    Key::ArrowUp => KeyCode::ArrowUp,
                    Key::ArrowDown => KeyCode::ArrowDown,
                    _ => unreachable!(),
                },
                logical_key: key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
            assert_eq!(app.world().resource::<ConsoleState>().input, expected);
        }

        assert_eq!(
            app.world().resource::<ConsoleState>().cmd_history_index,
            None
        );
    }

    #[test]
    fn history_navigation_skips_duplicate_entries_in_both_directions() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                cmd_history: vec!["status".into(), "help".into(), "help".into()],
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut().spawn((ConsoleInput, EditableText::new("")));

        for (key, expected) in [
            (Key::ArrowUp, "help"),
            (Key::ArrowUp, "status"),
            (Key::ArrowDown, "help"),
            (Key::ArrowDown, ""),
        ] {
            app.world_mut().write_message(KeyboardInput {
                key_code: match key {
                    Key::ArrowUp => KeyCode::ArrowUp,
                    Key::ArrowDown => KeyCode::ArrowDown,
                    _ => unreachable!(),
                },
                logical_key: key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
            assert_eq!(app.world().resource::<ConsoleState>().input, expected);
        }
    }

    #[test]
    fn completion_navigation_does_not_move_the_text_cursor() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "ec".into(),
                completion_items: vec![
                    crate::CompletionItem::from("echo").with_replace(0..2),
                    crate::CompletionItem::from("exit").with_replace(0..2),
                ],
                match_index: 1,
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        let input = app
            .world_mut()
            .spawn((ConsoleInput, EditableText::new("ec")))
            .id();
        {
            let mut entity = app.world_mut().entity_mut(input);
            let mut editable = entity.get_mut::<EditableText>().unwrap();
            editable.pending_edits.clear();
            editable.queue_edit(TextEdit::Up(false));
        }
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::ArrowUp,
            logical_key: Key::ArrowUp,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();

        assert_eq!(app.world().resource::<ConsoleState>().match_index, 0);
        assert!(
            app.world()
                .entity(input)
                .get::<EditableText>()
                .unwrap()
                .pending_edits
                .is_empty()
        );
    }

    #[test]
    fn accepting_completion_exits_history_browsing() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "ec".into(),
                completion_items: vec![crate::CompletionItem::from("echo").with_replace(0..2)],
                cmd_history: vec!["ec".into()],
                cmd_history_index: Some(0),
                cmd_history_draft: "draft".into(),
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut()
            .spawn((ConsoleInput, EditableText::new("ec")));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Tab,
            logical_key: Key::Tab,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();

        let state = app.world().resource::<ConsoleState>();
        assert_eq!(state.input, "echo ");
        assert_eq!(state.cmd_history_index, None);
        assert!(state.cmd_history_draft.is_empty());
    }

    #[test]
    fn queued_commands_parse_quotes_and_write_structured_output() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(r#"echo "hello world" two"#));
        execute_pending_commands(app.world_mut());

        let lines = app.world().resource::<ConsoleBuffer>().lines();
        assert_eq!(lines[0].text, r#"> echo "hello world" two"#);
        assert_eq!(lines[1].text, "hello world|two");

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("status"));
        execute_pending_commands(app.world_mut());
        let lines = app.world().resource::<ConsoleBuffer>().lines();
        assert_eq!(lines[3].level, ConsoleLevel::Info);
        assert_eq!(lines[4].level, ConsoleLevel::Warn);
    }

    #[test]
    fn unknown_commands_echo_the_submitted_input_before_the_error() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .record_command("missing arg".into(), 10);
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("missing arg"));

        execute_pending_commands(app.world_mut());

        let lines = app.world().resource::<ConsoleBuffer>().lines();
        assert_eq!(lines[0].text, "> missing arg");
        assert!(matches!(
            lines[0].source,
            ConsoleLineSource::CommandEcho { ref name } if name == "missing"
        ));
        assert_eq!(lines[1].text, "Unknown command: missing");
        assert_eq!(
            app.world().resource::<ConsoleState>().cmd_history_line_ids,
            [Some(lines[0].id)]
        );
    }

    #[test]
    fn executing_a_submitted_command_links_its_recall_entry_to_the_echo_row() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .record_command("echo linked".into(), 10);
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("echo linked"));

        execute_pending_commands(app.world_mut());

        let echo_id = app.world().resource::<ConsoleBuffer>().lines()[0].id;
        let state = app.world().resource::<ConsoleState>();
        assert_eq!(state.cmd_history_line_ids, [Some(echo_id)]);
    }

    #[test]
    fn earlier_queued_command_does_not_consume_a_pending_history_link() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("echo earlier"));
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .record_command("echo recalled".into(), 10);
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest {
                input: "echo recalled".into(),
                origin: crate::CommandOrigin::LocalUi,
            });

        execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleState>().pending_history_index,
            Some(0)
        );

        execute_pending_commands(app.world_mut());
        let echo_id = app.world().resource::<ConsoleBuffer>().lines()[2].id;
        assert_eq!(
            app.world().resource::<ConsoleState>().cmd_history_line_ids,
            [Some(echo_id)]
        );
    }

    #[test]
    fn runtime_aliases_expand_before_command_execution() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleAliases>()
            .set("say_hi", "echo hello");
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("say_hi world"));
        execute_pending_commands(app.world_mut());
        execute_pending_commands(app.world_mut());

        let lines = app.world().resource::<ConsoleBuffer>().lines();
        assert_eq!(lines[0].text, "> echo hello world");
        assert_eq!(lines[1].text, "hello|world");
    }

    #[test]
    fn alias_expansion_limit_emits_a_failed_execution_message() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleAliases>()
            .set("loop", "loop");
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("loop"));

        for _ in 0..=16 {
            execute_pending_commands(app.world_mut());
        }

        let messages = app
            .world()
            .resource::<Messages<crate::ConsoleCommandExecuted>>();
        let mut cursor = messages.get_cursor();
        let message = cursor.read(messages).next().unwrap();
        assert_eq!(message.input, "loop");
        assert_eq!(message.command.as_deref(), Some("loop"));
        assert!(!message.succeeded);
    }

    #[test]
    fn recalled_runtime_alias_links_to_its_expanded_echo_row() {
        let mut app = command_test_app(BuiltinCommands::default());
        app.world_mut()
            .resource_mut::<ConsoleAliases>()
            .set("say_hi", "echo hello");
        app.world_mut()
            .resource_mut::<ConsoleState>()
            .record_command("say_hi world".into(), 10);
        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("say_hi world"));

        execute_pending_commands(app.world_mut());
        execute_pending_commands(app.world_mut());

        let echo_id = app.world().resource::<ConsoleBuffer>().lines()[0].id;
        let state = app.world().resource::<ConsoleState>();
        assert_eq!(state.cmd_history_line_ids, [Some(echo_id)]);
    }

    #[test]
    fn alias_and_bind_set_preserve_quoted_command_arguments() {
        let mut app = command_test_app(BuiltinCommand::all());

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(
                r#"alias set greeting echo "hello world""#,
            ));
        execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleAliases>().get("greeting"),
            Some(r#"echo "hello world""#)
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new("greeting"));
        execute_pending_commands(app.world_mut());
        execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world()
                .resource::<ConsoleBuffer>()
                .last_line()
                .unwrap()
                .text,
            "hello world"
        );

        app.world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .push(ConsoleRequest::new(r#"bind set F1 echo "hello world""#));
        execute_pending_commands(app.world_mut());
        assert_eq!(
            app.world().resource::<ConsoleBinds>().get(KeyCode::F1),
            Some(r#"echo "hello world""#)
        );
    }

    #[test]
    fn key_bindings_queue_commands_only_while_the_console_is_closed() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, queue_bound_commands);
        app.world_mut()
            .resource_mut::<ConsoleBinds>()
            .set(KeyCode::F1, "echo hello");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F1);

        app.update();
        assert_eq!(app.world().resource::<ConsoleCommandQueue>().len(), 1);

        app.world_mut().resource_mut::<ConsoleState>().open = true;
        app.update();
        assert_eq!(app.world().resource::<ConsoleCommandQueue>().len(), 1);
    }

    #[test]
    fn key_bindings_require_an_exact_modifier_chord() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, queue_bound_commands);
        app.world_mut().resource_mut::<ConsoleBinds>().set_binding(
            ConsoleKeyBinding {
                key: KeyCode::KeyW,
                modifiers: ConsoleKeyModifiers {
                    shift: true,
                    ..default()
                },
            },
            "echo sprint",
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::KeyW);
        }

        app.update();
        assert_eq!(app.world().resource::<ConsoleCommandQueue>().len(), 1);
    }

    #[test]
    fn key_bindings_support_the_platform_meta_modifier() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, queue_bound_commands);
        app.world_mut().resource_mut::<ConsoleBinds>().set_binding(
            ConsoleKeyBinding {
                key: KeyCode::KeyW,
                modifiers: ConsoleKeyModifiers {
                    meta: true,
                    ..default()
                },
            },
            "echo save",
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(if cfg!(target_os = "macos") {
                KeyCode::SuperLeft
            } else {
                KeyCode::ControlLeft
            });
            keys.press(KeyCode::KeyW);
        }

        app.update();
        assert_eq!(app.world().resource::<ConsoleCommandQueue>().len(), 1);
    }

    #[test]
    fn key_bindings_do_not_match_the_wrong_modifier_chord() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, queue_bound_commands);
        app.world_mut().resource_mut::<ConsoleBinds>().set_binding(
            ConsoleKeyBinding {
                key: KeyCode::KeyW,
                modifiers: ConsoleKeyModifiers {
                    shift: true,
                    ..default()
                },
            },
            "echo sprint",
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyW);
        }

        app.update();
        assert!(app.world().resource::<ConsoleCommandQueue>().is_empty());
    }

    #[test]
    fn key_bindings_do_not_fire_while_super_is_held() {
        let mut app = App::new();
        app.insert_resource(ConsoleState::default())
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, queue_bound_commands);
        app.world_mut()
            .resource_mut::<ConsoleBinds>()
            .set(KeyCode::KeyW, "echo walk");
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::SuperLeft);
            keys.press(KeyCode::KeyW);
        }

        app.update();
        assert!(app.world().resource::<ConsoleCommandQueue>().is_empty());
    }

    #[test]
    fn enter_submits_the_editable_text_value() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "echo hello".into(),
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut()
            .spawn((ConsoleInput, EditableText::new("echo hello")));
        app.world_mut()
            .spawn((ConsoleHistory, ScrollPosition(Vec2::new(0.0, 80.0))));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();
        let request = app
            .world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .pop_front()
            .unwrap();
        assert_eq!(request.request.input, "echo hello");
        assert!(app.world().resource::<ConsoleState>().input.is_empty());
        let scroll_position = app
            .world_mut()
            .query_filtered::<&ScrollPosition, With<ConsoleHistory>>()
            .single(app.world())
            .expect("history panel should exist");
        assert_eq!(scroll_position.y, f32::MAX);
    }

    #[test]
    fn empty_input_closes_the_console_when_close_is_enabled() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig {
            close_on_empty_submit: true,
            ..default()
        })
        .insert_resource(BuiltinCommands::default())
        .insert_resource(ConsoleState {
            open: true,
            ..default()
        })
        .insert_resource(ButtonInput::<KeyCode>::default())
        .init_resource::<ConsoleCommandQueue>()
        .add_message::<KeyboardInput>()
        .add_systems(Update, capture_console_input);
        app.world_mut().spawn((ConsoleInput, EditableText::new("")));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();

        assert!(!app.world().resource::<ConsoleState>().open);
        assert!(app.world().resource::<ConsoleCommandQueue>().is_empty());
    }

    #[test]
    fn ios_return_character_submits_the_editable_text_value() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "echo hello".into(),
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut()
            .spawn((ConsoleInput, EditableText::new("echo hello")));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Character("\n".into()),
            state: ButtonState::Pressed,
            text: Some("\n".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();
        let request = app
            .world_mut()
            .resource_mut::<ConsoleCommandQueue>()
            .pop_front()
            .unwrap();
        assert_eq!(request.request.input, "echo hello");
        assert!(app.world().resource::<ConsoleState>().input.is_empty());
    }

    #[test]
    fn meta_backspace_clears_the_console_input() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "echo hello".into(),
                ..default()
            })
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut()
            .spawn((ConsoleInput, EditableText::new("echo hello")));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(if cfg!(target_os = "macos") {
                KeyCode::SuperLeft
            } else {
                KeyCode::ControlLeft
            });
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Backspace,
            logical_key: Key::Backspace,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();

        assert!(app.world().resource::<ConsoleState>().input.is_empty());
        let editable = app
            .world_mut()
            .query_filtered::<&EditableText, With<ConsoleInput>>()
            .single(app.world())
            .expect("console input should exist")
            .value()
            .to_string();
        assert!(editable.is_empty());
    }

    #[test]
    fn closed_console_drains_keyboard_input_before_opening() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_resource::<ConsoleCommandQueue>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture_console_input);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });

        app.update();
        app.world_mut().resource_mut::<ConsoleState>().open = true;
        app.world_mut().spawn((ConsoleInput, EditableText::new("")));
        app.update();

        assert!(app.world().resource::<ConsoleState>().open);
    }
}
