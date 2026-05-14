//! Optional display-history persistence — only compiled when the
//! `persistent-history` feature is enabled.
//!
//! The whole console display (commands and their outputs) is mirrored to a
//! plain-text file. On startup the file is read back into `ConsoleState` and
//! Up/Down recall is rebuilt by extracting the `> command` echo lines.

use crate::config::ConsoleConfig;
use crate::state::ConsoleState;
use bevy::prelude::*;

const ECHO_PREFIX: &str = "> ";
const SESSION_SEPARATOR: &str = "── previous session ──";

/// Plugin entry point — registers the save system.
pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, persist_history);
}

/// Builds the initial `ConsoleState` from the persisted file (if any). The
/// loaded display lines are restored verbatim, Up/Down recall is rebuilt from
/// the `> ` echo lines, and a session separator is appended below the loaded
/// content so the user can tell the boundary between past and current runs.
pub(crate) fn load_initial_state(config: &ConsoleConfig) -> ConsoleState {
    let mut state = ConsoleState::default();
    let Some(path) = &config.history_file else {
        return state;
    };

    let lines = match std::fs::read_to_string(path) {
        Ok(c) => c
            .lines()
            .map(str::trim_end)
            .map(String::from)
            .collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(
                "chill_bevy_console: history file {:?} not found yet (will be created on first command)",
                path
            );
            return state;
        }
        Err(e) => {
            warn!(
                "chill_bevy_console: failed to read history file {:?}: {}",
                path, e
            );
            return state;
        }
    };

    state.cmd_history = lines
        .iter()
        .filter_map(|l| l.strip_prefix(ECHO_PREFIX).map(String::from))
        .collect();
    state.history = lines;
    if !state.history.is_empty() {
        state.history.push(SESSION_SEPARATOR.to_string());
    }

    info!(
        "chill_bevy_console: loaded {} history lines ({} recall entries) from {:?}",
        state.history.len(),
        state.cmd_history.len(),
        path
    );
    state
}

fn persist_history(
    config: Res<ConsoleConfig>,
    state: Res<ConsoleState>,
    mut last_seen: Local<u64>,
) {
    if state.history_mutation_count == *last_seen {
        return;
    }
    *last_seen = state.history_mutation_count;

    let Some(path) = &config.history_file else {
        return;
    };

    let mut content = state.history.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    match std::fs::write(path, &content) {
        Ok(()) => debug!(
            "chill_bevy_console: wrote {} history lines to {:?}",
            state.history.len(),
            path
        ),
        Err(e) => warn!(
            "chill_bevy_console: failed to write history file {:?}: {}",
            path, e
        ),
    }
}
