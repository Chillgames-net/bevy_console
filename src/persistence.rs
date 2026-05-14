//! Optional command-history persistence — only compiled when the
//! `persistent-history` feature is enabled.

use crate::config::ConsoleConfig;
use crate::state::ConsoleState;
use bevy::prelude::*;
use std::path::Path;

/// Tracks whether `cmd_history` has been mutated since the last disk write.
#[derive(Resource, Default)]
pub(crate) struct PersistenceState {
    pub(crate) dirty: bool,
}

/// Plugin entry point — registers the resource and the save system.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<PersistenceState>()
        .add_systems(Update, persist_cmd_history);
}

/// Builds the initial `ConsoleState`, loading any persisted commands from disk
/// and echoing them into the visible history panel so the user can see what
/// they ran in the previous session.
pub(crate) fn load_initial_state(config: &ConsoleConfig) -> ConsoleState {
    let mut state = ConsoleState::default();
    let Some(path) = &config.history_file else {
        return state;
    };
    state.cmd_history = load_cmd_history(path, config.history_max_entries);
    if !state.cmd_history.is_empty() {
        state.history.push("── previous session ──".to_string());
        for cmd in &state.cmd_history {
            state.history.push(format!("> {cmd}"));
        }
    }
    state
}

/// Called from the input handler whenever a new command is appended to
/// `cmd_history`. Trims to `history_max_entries` and marks state dirty so the
/// save system writes it on the next tick.
pub(crate) fn on_command_submitted(
    state: &mut ConsoleState,
    config: &ConsoleConfig,
    persistence: &mut PersistenceState,
) {
    let max = config.history_max_entries.max(1);
    if state.cmd_history.len() > max {
        let excess = state.cmd_history.len() - max;
        state.cmd_history.drain(0..excess);
    }
    persistence.dirty = true;
}

fn load_cmd_history(path: &Path, max_entries: usize) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(
                "chill_bevy_console: history file {:?} not found yet (will be created on first command)",
                path
            );
            return Vec::new();
        }
        Err(e) => {
            warn!(
                "chill_bevy_console: failed to read history file {:?}: {}",
                path, e
            );
            return Vec::new();
        }
    };
    let mut lines: Vec<String> = content
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let max = max_entries.max(1);
    if lines.len() > max {
        let excess = lines.len() - max;
        lines.drain(0..excess);
    }
    info!(
        "chill_bevy_console: loaded {} history entries from {:?}",
        lines.len(),
        path
    );
    lines
}

fn persist_cmd_history(
    config: Res<ConsoleConfig>,
    state: Res<ConsoleState>,
    mut persistence: ResMut<PersistenceState>,
) {
    if !persistence.dirty {
        return;
    }
    persistence.dirty = false;

    let Some(path) = &config.history_file else {
        return;
    };

    let mut content = state.cmd_history.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    match std::fs::write(path, &content) {
        Ok(()) => debug!(
            "chill_bevy_console: wrote {} history entries to {:?}",
            state.cmd_history.len(),
            path
        ),
        Err(e) => warn!(
            "chill_bevy_console: failed to write history file {:?}: {}",
            path, e
        ),
    }
}
