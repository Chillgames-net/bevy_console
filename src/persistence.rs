//! Optional command-history persistence.

use crate::{ConsoleBuffer, ConsoleConfig, ConsoleState};
#[cfg(not(target_arch = "wasm32"))]
use crate::{ConsoleLevel, ConsoleLineSource, ParsedInput};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

pub(crate) fn plugin(_app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    _app.add_systems(Update, persist_history);
}

pub(crate) fn load_initial_data(config: &ConsoleConfig) -> (ConsoleState, ConsoleBuffer) {
    #[cfg(not(target_arch = "wasm32"))]
    let mut state = ConsoleState::default();
    #[cfg(target_arch = "wasm32")]
    let state = ConsoleState::default();
    #[cfg(not(target_arch = "wasm32"))]
    let mut buffer = ConsoleBuffer::new(config.max_history_lines);
    #[cfg(target_arch = "wasm32")]
    let buffer = ConsoleBuffer::new(config.max_history_lines);

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &config.history_file {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                state.restore_command_history(
                    content.lines().map(str::to_owned).collect(),
                    config.max_command_history,
                );
                for command in state.command_history() {
                    let name = ParsedInput::parse(command)
                        .command()
                        .unwrap_or_default()
                        .to_string();
                    buffer.push(
                        ConsoleLevel::Info,
                        ConsoleLineSource::Command { name },
                        format!("> {command}"),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warn!(
                "chill_bevy_console: failed to read history file {:?}: {}",
                path, error
            ),
        }
    }

    (state, buffer)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PersistenceTracker {
    initialized: bool,
    revision: u64,
    path: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
fn persist_history(
    config: Res<ConsoleConfig>,
    state: Res<ConsoleState>,
    mut tracker: Local<PersistenceTracker>,
) {
    let path = config.history_file.clone();
    if !tracker.initialized {
        tracker.initialized = true;
        tracker.revision = state.command_history_revision;
        tracker.path = path;
        return;
    }
    if tracker.revision == state.command_history_revision && tracker.path == path {
        return;
    }

    tracker.revision = state.command_history_revision;
    tracker.path.clone_from(&path);
    let Some(path) = path else {
        return;
    };
    if let Err(error) = write_history(&path, state.command_history()) {
        warn!(
            "chill_bevy_console: failed to write history file {:?}: {}",
            path, error
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_history(path: &Path, commands: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = commands.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::{load_initial_data, write_history};
    use crate::{ConsoleConfig, ConsoleLineSource};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn command_history_round_trips() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "chill_bevy_console_history_{}_{}.txt",
            std::process::id(),
            unique
        ));
        write_history(&path, &["map forest".into(), "set debug true".into()]).unwrap();
        let config = ConsoleConfig {
            history_file: Some(path.clone()),
            ..ConsoleConfig::default()
        };

        let (state, buffer) = load_initial_data(&config);

        assert_eq!(state.command_history(), ["map forest", "set debug true"]);
        assert_eq!(
            buffer
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["> map forest", "> set debug true"]
        );
        assert_eq!(
            buffer.lines()[0].source,
            ConsoleLineSource::Command { name: "map".into() }
        );
        assert_eq!(
            buffer.lines()[1].source,
            ConsoleLineSource::Command { name: "set".into() }
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn restored_visual_history_respects_both_limits() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "chill_bevy_console_history_limits_{}_{}.txt",
            std::process::id(),
            unique
        ));
        write_history(
            &path,
            &["one".into(), "two".into(), "three".into(), "four".into()],
        )
        .unwrap();
        let config = ConsoleConfig {
            max_command_history: 3,
            max_history_lines: 2,
            history_file: Some(path.clone()),
            ..ConsoleConfig::default()
        };

        let (state, buffer) = load_initial_data(&config);

        assert_eq!(state.command_history(), ["two", "three", "four"]);
        assert_eq!(
            buffer
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["> three", "> four"]
        );
        std::fs::remove_file(path).unwrap();
    }
}
