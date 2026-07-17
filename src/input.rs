use crate::config::{BuiltinCommand, BuiltinCommands, ConsoleConfig};
use crate::state::ConsoleState;
use crate::ui::{ConsoleAssets, ConsoleInput, DevConsoleOverlay, spawn_console_ui};
use crate::{
    Args, CommandExecutor, ConsoleAliases, ConsoleBinds, ConsoleBuffer, ConsoleCommandExecuted,
    ConsoleCommandQueue, ConsoleLevel, ConsoleLineMessage, ConsoleLineSource, ConsoleRegistry,
    ConsoleRequest,
};
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::{ComputedNode, ScrollPosition};

const CONSOLE_SCROLL_SPEED: f32 = 1.25;

#[derive(SystemParam)]
pub(crate) struct ConsoleInputSettings<'w> {
    config: Res<'w, ConsoleConfig>,
    builtin_commands: Res<'w, BuiltinCommands>,
}

// ── Run conditions ────────────────────────────────────────────────────────────

pub(crate) fn console_open(state: Option<Res<ConsoleState>>) -> bool {
    state.is_some_and(|s| s.open)
}

pub(crate) fn has_pending_command(queue: Option<Res<ConsoleCommandQueue>>) -> bool {
    queue.is_some_and(|queue| !queue.is_empty())
}

pub(crate) fn console_open_and_changed(
    state: Option<Res<ConsoleState>>,
    buffer: Option<Res<ConsoleBuffer>>,
) -> bool {
    state.is_some_and(|state| {
        state.open && (state.is_changed() || buffer.is_some_and(|buffer| buffer.is_changed()))
    })
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

/// Spawns or despawns the console UI whenever `state.open` changes.
/// Reacts to changes from any source (key press, external code, etc.).
pub(crate) fn sync_console_ui(
    mut commands: Commands,
    mut state: ResMut<ConsoleState>,
    overlay_q: Query<Entity, With<DevConsoleOverlay>>,
    assets: Res<ConsoleAssets>,
    config: Res<ConsoleConfig>,
    mut prev_open: Local<bool>,
) {
    if *prev_open == state.open {
        return;
    }
    *prev_open = state.open;

    if state.open {
        spawn_console_ui(&mut commands, &assets, &config, &state.input);
        state.mark_input_changed();
    } else {
        for entity in &overlay_q {
            commands.entity(entity).despawn();
        }
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
            state.replace_input(edited);
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
                match state.cmd_history_index {
                    None => {
                        // Start browsing: save the live input as a draft.
                        state.cmd_history_draft = state.input.clone();
                        let idx = state.cmd_history.len() - 1;
                        state.cmd_history_index = Some(idx);
                        let value = state.cmd_history[idx].clone();
                        sync_history_selection(&mut state, &mut input, value);
                    }
                    Some(0) => { /* already at oldest — stay */ }
                    Some(i) => {
                        let idx = i - 1;
                        state.cmd_history_index = Some(idx);
                        let value = state.cmd_history[idx].clone();
                        sync_history_selection(&mut state, &mut input, value);
                    }
                }
                continue;
            }
            Key::ArrowDown => {
                discard_vertical_cursor_moves(&mut input);
                if state.cmd_history_index.is_none() && !state.completion_items.is_empty() {
                    state.select_next_completion();
                    continue;
                }
                // Command history: go to newer entry or restore draft.
                match state.cmd_history_index {
                    None => { /* not browsing — nothing to do */ }
                    Some(i) if i + 1 >= state.cmd_history.len() => {
                        // Past the newest entry: restore the draft.
                        state.cmd_history_index = None;
                        let value = state.cmd_history_draft.clone();
                        sync_history_selection(&mut state, &mut input, value);
                        state.cmd_history_draft.clear();
                    }
                    Some(i) => {
                        let idx = i + 1;
                        state.cmd_history_index = Some(idx);
                        let value = state.cmd_history[idx].clone();
                        sync_history_selection(&mut state, &mut input, value);
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
    state.replace_input(value);
}

fn discard_vertical_cursor_moves(input: &mut EditableText) {
    input
        .pending_edits
        .retain(|edit| !matches!(edit, TextEdit::Up(_) | TextEdit::Down(_)));
}

pub(crate) fn set_editable_text(input: &mut EditableText, value: &str, cursor: usize) {
    input.clear();
    input.editor_mut().set_text(value);
    let mut cursor = cursor.min(value.len());
    while !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    if cursor == value.len() {
        input.queue_edit(TextEdit::TextEnd(false));
        return;
    }
    input.queue_edit(TextEdit::TextStart(false));
    for _ in value[..cursor].chars() {
        input.queue_edit(TextEdit::Right(false));
    }
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

/// Adds command requests sent by game systems to the same FIFO queue as local
/// keyboard input. Applications can therefore execute a command with
/// `MessageWriter<ConsoleRequest>` without opening the UI.
pub(crate) fn queue_console_requests(
    mut requests: MessageReader<ConsoleRequest>,
    mut queue: ResMut<ConsoleCommandQueue>,
) {
    for request in requests.read() {
        queue.push(request.clone());
    }
}

/// Receives structured lines emitted by game systems.
pub(crate) fn collect_console_lines(
    mut messages: MessageReader<ConsoleLineMessage>,
    mut buffer: ResMut<ConsoleBuffer>,
) {
    for line in messages.read() {
        buffer.push(line.level, line.source.clone(), &line.text);
    }
}

pub(crate) fn scroll_console(
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut state: ResMut<ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<crate::ui::ConsoleHistory>>,
) {
    let wheel_pixels: f32 = mouse_wheel
        .read()
        .map(|ev| match ev.unit {
            MouseScrollUnit::Line => ev.y * MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
            MouseScrollUnit::Pixel => ev.y,
        })
        .sum();

    // Drain wheel messages while closed so a pre-open scroll cannot apply to
    // the newly spawned history panel.
    if !state.open {
        return;
    }

    let key_pixels = if keys.just_pressed(KeyCode::PageUp) {
        240.0
    } else if keys.just_pressed(KeyCode::PageDown) {
        -240.0
    } else {
        0.0
    };
    if wheel_pixels == 0.0 && key_pixels == 0.0 {
        return;
    }

    let Ok((mut scroll_pos, computed)) = history_q.single_mut() else {
        return;
    };

    // `MouseWheel` and computed UI sizes are physical pixels, while
    // `ScrollPosition` is logical pixels. Convert both so scrolling feels
    // consistent on high-DPI displays.
    let pixels = (wheel_pixels * computed.inverse_scale_factor + key_pixels) * CONSOLE_SCROLL_SPEED;

    // y = 0 → top (oldest), y = max → bottom (newest).
    // Wheel up (pixels > 0) → go toward older content → decrease offset.
    //
    // When scroll_follow was true, scroll_pos.y may be f32::MAX because Bevy
    // renders at the clamped bottom but never writes the clamped value back to
    // the component. Clamp against max_scroll first so the delta is applied
    // from the real bottom, not from infinity.
    let max_scroll =
        (computed.content_size().y - computed.size().y).max(0.0) * computed.inverse_scale_factor;
    let current = scroll_pos.y.min(max_scroll);
    let new_y = (current - pixels).clamp(0.0, max_scroll);
    scroll_pos.y = new_y;

    if pixels > 0.0 {
        // Scrolling up — stop following tail.
        if state.scroll_follow {
            state.scroll_follow = false;
        }
    } else if new_y >= max_scroll - 1.0 {
        // Scrolled back to the bottom — re-enable tail follow.
        if !state.scroll_follow {
            state.scroll_follow = true;
        }
    }
}

pub(crate) fn execute_pending_commands(world: &mut World) {
    let Some(queued) = world.resource_mut::<ConsoleCommandQueue>().pop_front() else {
        return;
    };
    let request = queued.request;
    let cmd_str = request.input;

    let parsed = crate::ParsedInput::parse(&cmd_str);
    if let Some(error) = parsed.error {
        write_line(
            world,
            ConsoleLevel::Error,
            ConsoleLineSource::System,
            format!("Parse error: {}", error.message),
        );
        world.write_message(ConsoleCommandExecuted {
            input: cmd_str,
            command: None,
            origin: request.origin,
            succeeded: false,
        });
        return;
    }
    let Some(name) = parsed.command() else { return };
    let args = Args::from_parsed(&parsed);

    let command = {
        let registry = world.resource::<ConsoleRegistry>();
        registry
            .get(name)
            .map(|def| (def.spec.name.clone(), def.executor))
    };

    let Some((command_name, executor)) = command else {
        if let Some(expansion) = world
            .resource::<ConsoleAliases>()
            .get(name)
            .map(str::to_owned)
        {
            if queued.alias_depth >= 16 {
                write_line(
                    world,
                    ConsoleLevel::Error,
                    ConsoleLineSource::System,
                    format!("Alias expansion limit exceeded while resolving `{name}`"),
                );
                return;
            }
            let suffix = &cmd_str[parsed.tokens[0].range.end..];
            world
                .resource_mut::<ConsoleCommandQueue>()
                .push_alias_expansion(
                    ConsoleRequest {
                        input: format!("{expansion}{suffix}"),
                        origin: request.origin,
                    },
                    queued.alias_depth + 1,
                );
            return;
        }
        write_line(
            world,
            ConsoleLevel::Error,
            ConsoleLineSource::System,
            format!("Unknown command: {name}"),
        );
        world.write_message(ConsoleCommandExecuted {
            input: cmd_str,
            command: Some(name.to_string()),
            origin: request.origin,
            succeeded: false,
        });
        return;
    };

    let source = ConsoleLineSource::Command {
        name: command_name.clone(),
    };
    write_line(
        world,
        ConsoleLevel::Info,
        source.clone(),
        format!("> {cmd_str}"),
    );

    let result = match executor {
        CommandExecutor::Structured(id) => match world.run_system_with(id, args) {
            Ok(output) => output.lines,
            Err(error) => vec![(ConsoleLevel::Error, format!("System error: {error}"))],
        },
        #[cfg(feature = "resource-properties")]
        CommandExecutor::Exclusive(command) => command(world, args).lines,
    };

    let succeeded = !result
        .iter()
        .any(|(level, _)| *level == ConsoleLevel::Error);
    for (level, text) in result {
        write_line(world, level, source.clone(), text);
    }
    world.write_message(ConsoleCommandExecuted {
        input: cmd_str,
        command: Some(command_name),
        origin: request.origin,
        succeeded,
    });
}

fn write_line(
    world: &mut World,
    level: ConsoleLevel,
    source: ConsoleLineSource,
    text: impl AsRef<str>,
) {
    let text = text.as_ref();
    world
        .resource_mut::<ConsoleBuffer>()
        .push(level, source, text);
}

#[cfg(test)]
mod tests {
    use super::{
        capture_console_input, execute_pending_commands, queue_bound_commands, sync_console_ui,
    };
    use crate::ui::{ConsoleAssets, ConsoleHistory, ConsoleInput};
    use crate::{
        BuiltinCommand, BuiltinCommands, CommandArgs, ConsoleAliases, ConsoleAppExt, ConsoleBinds,
        ConsoleBuffer, ConsoleCommandQueue, ConsoleConfig, ConsoleKeyBinding, ConsoleKeyModifiers,
        ConsoleLevel, ConsoleRequest, ConsoleResult, ConsoleState,
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
            .add_console_command("echo", "echo <text>", echo)
            .add_console_command("status", "status", structured)
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
                    crate::CompletionItem::new("alpha", 0..0),
                    crate::CompletionItem::new("beta", 0..0),
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
    fn completion_navigation_does_not_move_the_text_cursor() {
        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(BuiltinCommands::default())
            .insert_resource(ConsoleState {
                open: true,
                input: "ec".into(),
                completion_items: vec![
                    crate::CompletionItem::new("echo", 0..2),
                    crate::CompletionItem::new("exit", 0..2),
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
                completion_items: vec![crate::CompletionItem::new("echo", 0..2)],
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
