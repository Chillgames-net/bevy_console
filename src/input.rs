use crate::config::ConsoleConfig;
use crate::registry::{CommandFn, ConsoleRegistry};
use crate::state::ConsoleState;
use crate::ui::{ConsoleAssets, DevConsoleOverlay, spawn_console_ui};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use std::sync::Arc;

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

pub(crate) fn toggle_console(
    mut commands: Commands,
    config: Res<ConsoleConfig>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ConsoleState>,
    overlay_q: Query<Entity, With<DevConsoleOverlay>>,
    assets: Res<ConsoleAssets>,
) {
    if !keys.just_pressed(config.toggle_key) {
        return;
    }

    if state.open {
        state.open = false;
        for entity in &overlay_q {
            commands.entity(entity).despawn();
        }
    } else {
        state.open = true;
        spawn_console_ui(&mut commands, &assets, &config);
    }
}

pub(crate) fn capture_console_input(
    mut key_events: MessageReader<KeyboardInput>,
    mut state: ResMut<ConsoleState>,
    registry: Res<ConsoleRegistry>,
) {
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }

        match &ev.logical_key {
            Key::Character(c) => {
                if c.as_str() != "`" && c.as_str() != "~" {
                    state.input.push_str(c.as_str());
                }
            }
            Key::Space => {
                state.input.push(' ');
            }
            Key::Backspace => {
                let mut chars = state.input.chars();
                chars.next_back();
                state.input = chars.as_str().to_string();
            }
            Key::Enter => {
                let cmd = state.input.trim().to_string();
                if !cmd.is_empty() {
                    state.pending_command = Some(cmd);
                }
                state.input.clear();
                state.matches.clear();
                state.match_index = 0;
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
            Key::ArrowDown => {
                if !state.matches.is_empty() {
                    state.match_index = (state.match_index + 1) % state.matches.len();
                }
                continue;
            }
            Key::ArrowUp => {
                if !state.matches.is_empty() {
                    state.match_index =
                        (state.match_index + state.matches.len() - 1) % state.matches.len();
                }
                continue;
            }
            _ => {}
        }

        state.recompute_matches(&registry);
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
    let args = &parts[1..];

    let func: Option<CommandFn> = {
        let registry = world.resource::<ConsoleRegistry>();
        registry.commands.get(name).map(|def| Arc::clone(&def.func))
    };

    // Push the echo before running so commands like `clear` can wipe it.
    world
        .resource_mut::<ConsoleState>()
        .push_line(format!("> {cmd_str}"));

    let result = match func {
        Some(f) => f(args, world),
        None => format!("Unknown command: {name}"),
    };

    if !result.is_empty() {
        world.resource_mut::<ConsoleState>().push_line(result);
    }
}
