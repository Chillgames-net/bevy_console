use crate::Args;
use crate::config::ConsoleConfig;
use crate::registry::ConsoleRegistry;
use crate::state::ConsoleState;
use crate::ui::{ConsoleAssets, DevConsoleOverlay, spawn_console_ui};
use bevy::ecs::system::SystemId;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};

// ── Run conditions ────────────────────────────────────────────────────────────

pub(crate) fn console_open(state: Option<Res<ConsoleState>>) -> bool {
    state.map_or(false, |s| s.open)
}

pub(crate) fn has_pending_command(state: Option<Res<ConsoleState>>) -> bool {
    state.map_or(false, |s| s.pending_command.is_some())
}

pub(crate) fn console_open_and_changed(state: Option<Res<ConsoleState>>) -> bool {
    state.map_or(false, |s| s.open && s.is_changed())
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
    state: Res<ConsoleState>,
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
        spawn_console_ui(&mut commands, &assets, &config);
    } else {
        for entity in &overlay_q {
            commands.entity(entity).despawn();
        }
    }
}

pub(crate) fn capture_console_input(
    mut key_events: MessageReader<KeyboardInput>,
    mut state: ResMut<ConsoleState>,
    registry: Res<ConsoleRegistry>,
    keys: Res<ButtonInput<KeyCode>>,
    #[cfg(feature = "persistent-history")] config: Res<ConsoleConfig>,
    #[cfg(feature = "persistent-history")] mut persistence: ResMut<
        crate::persistence::PersistenceState,
    >,
) {
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }

        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

        match &ev.logical_key {
            Key::Character(c) => {
                let s = c.as_str();
                if s == "`" || s == "~" {
                    continue;
                }
                // Typing exits history browsing in place (edit the recalled line).
                state.cmd_history_index = None;
                state.cmd_history_draft.clear();
                state.input.push_str(s);
            }
            Key::Space => {
                state.cmd_history_index = None;
                state.cmd_history_draft.clear();
                state.input.push(' ');
            }
            Key::Backspace => {
                if alt {
                    // Alt+Backspace — clear the whole input.
                    state.input.clear();
                    state.cmd_history_index = None;
                    state.cmd_history_draft.clear();
                } else {
                    // Editing exits history browsing in place.
                    state.cmd_history_index = None;
                    state.cmd_history_draft.clear();
                    let mut chars = state.input.chars();
                    chars.next_back();
                    state.input = chars.as_str().to_string();
                }
            }
            Key::Enter => {
                let cmd = state.input.trim().to_string();
                if !cmd.is_empty() {
                    state.pending_command = Some(cmd.clone());
                    // Deduplicate consecutive identical entries.
                    if state.cmd_history.last().map(String::as_str) != Some(cmd.as_str()) {
                        state.cmd_history.push(cmd);
                        #[cfg(feature = "persistent-history")]
                        crate::persistence::on_command_submitted(
                            &mut state,
                            &config,
                            &mut persistence,
                        );
                    }
                }
                state.input.clear();
                state.matches.clear();
                state.match_index = 0;
                state.scroll_follow = true;
                state.cmd_history_index = None;
                state.cmd_history_draft.clear();
                continue;
            }
            Key::Tab => {
                if let Some(name) = state.matches.get(state.match_index).cloned() {
                    state.input = format!("{name} ");
                    state.matches.clear();
                    state.match_index = 0;
                    continue;
                }
            }
            Key::ArrowUp => {
                if !state.matches.is_empty() {
                    // Dropdown navigation takes priority.
                    state.match_index =
                        (state.match_index + state.matches.len() - 1) % state.matches.len();
                    continue;
                }
                // Command history: go to older entry.
                if state.cmd_history.is_empty() {
                    continue;
                }
                match state.cmd_history_index {
                    None => {
                        // Start browsing: save the live input as a draft.
                        state.cmd_history_draft = state.input.clone();
                        let idx = state.cmd_history.len() - 1;
                        state.cmd_history_index = Some(idx);
                        state.input = state.cmd_history[idx].clone();
                    }
                    Some(0) => { /* already at oldest — stay */ }
                    Some(i) => {
                        let idx = i - 1;
                        state.cmd_history_index = Some(idx);
                        state.input = state.cmd_history[idx].clone();
                    }
                }
                continue;
            }
            Key::ArrowDown => {
                if !state.matches.is_empty() {
                    state.match_index = (state.match_index + 1) % state.matches.len();
                    continue;
                }
                // Command history: go to newer entry or restore draft.
                match state.cmd_history_index {
                    None => { /* not browsing — nothing to do */ }
                    Some(i) if i + 1 >= state.cmd_history.len() => {
                        // Past the newest entry: restore the draft.
                        state.cmd_history_index = None;
                        state.input = state.cmd_history_draft.clone();
                        state.cmd_history_draft.clear();
                    }
                    Some(i) => {
                        let idx = i + 1;
                        state.cmd_history_index = Some(idx);
                        state.input = state.cmd_history[idx].clone();
                    }
                }
                continue;
            }
            Key::End => {
                state.scroll_follow = true;
                continue;
            }
            _ => {}
        }

        state.recompute_matches(&registry);
    }
}

pub(crate) fn scroll_console(
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut state: ResMut<ConsoleState>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<crate::ui::ConsoleHistory>>,
) {
    let pixels: f32 = mouse_wheel
        .read()
        .map(|ev| match ev.unit {
            MouseScrollUnit::Line => ev.y * 20.0,
            MouseScrollUnit::Pixel => ev.y,
        })
        .sum();

    if pixels == 0.0 {
        return;
    }

    let Ok((mut scroll_pos, computed)) = history_q.single_mut() else {
        return;
    };

    // y = 0 → top (oldest), y = max → bottom (newest).
    // Wheel up (pixels > 0) → go toward older content → decrease offset.
    //
    // When scroll_follow was true, scroll_pos.y may be f32::MAX because Bevy
    // renders at the clamped bottom but never writes the clamped value back to
    // the component. Clamp against max_scroll first so the delta is applied
    // from the real bottom, not from infinity.
    let max_scroll = (computed.content_size().y - computed.size().y).max(0.0);
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
    let cmd_str = {
        let mut state = world.resource_mut::<ConsoleState>();
        state.pending_command.take()
    };

    let Some(cmd_str) = cmd_str else { return };

    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    let name = parts[0];
    let args = Args(parts[1..].iter().map(|s| s.to_string()).collect());

    let system_id: Option<SystemId<In<Args>, String>> = {
        let registry = world.resource::<ConsoleRegistry>();
        registry.commands.get(name).map(|def| def.system_id)
    };

    // Push the echo before running so commands like `clear` can wipe it.
    world
        .resource_mut::<ConsoleState>()
        .push_line(format!("> {cmd_str}"));

    let result = match system_id {
        Some(id) => match world.run_system_with(id, args) {
            Ok(output) => output,
            Err(err) => format!("System error: {err}"),
        },
        None => format!("Unknown command: {name}"),
    };

    if !result.is_empty() {
        world.resource_mut::<ConsoleState>().push_line(result);
    }
}
