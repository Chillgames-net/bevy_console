//! Optional console transcript persistence.

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
                restore_transcript(&content, &mut state, &mut buffer, config);
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
    buffer: Res<ConsoleBuffer>,
    mut tracker: Local<PersistenceTracker>,
) {
    let path = config.history_file.clone();
    if !tracker.initialized {
        tracker.initialized = true;
        tracker.revision = state.command_history_revision;
        tracker.path = path;
        return;
    }
    if tracker.revision == state.command_history_revision
        && tracker.path == path
        && !buffer.is_changed()
    {
        return;
    }

    tracker.revision = state.command_history_revision;
    tracker.path.clone_from(&path);
    let Some(path) = path else {
        return;
    };
    if let Err(error) = write_history(&path, &buffer) {
        warn!(
            "chill_bevy_console: failed to write history file {:?}: {}",
            path, error
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_transcript(
    content: &str,
    state: &mut ConsoleState,
    buffer: &mut ConsoleBuffer,
    config: &ConsoleConfig,
) {
    let mut commands = Vec::new();
    let mut command_line_ids = Vec::new();

    for line in content.lines() {
        if let Some(command) = line.strip_prefix("> ") {
            commands.push(command.to_owned());
            let name = ParsedInput::parse(command)
                .command()
                .unwrap_or_default()
                .to_string();
            buffer.push(
                ConsoleLevel::Info,
                ConsoleLineSource::Command { name },
                format!("> {command}"),
            );
            command_line_ids.push(buffer.last_line().map(|line| line.id));
        } else {
            match line.strip_prefix("< ") {
                Some(output) => buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, output),
                // Files written by releases before transcript persistence held
                // one command per line, without a marker.
                None => {
                    commands.push(line.to_owned());
                    let name = ParsedInput::parse(line)
                        .command()
                        .unwrap_or_default()
                        .to_string();
                    buffer.push(
                        ConsoleLevel::Info,
                        ConsoleLineSource::Command { name },
                        format!("> {line}"),
                    );
                    command_line_ids.push(buffer.last_line().map(|line| line.id));
                }
            }
        }
    }

    state.restore_command_history(commands, config.max_command_history);
    let retained = state.command_history().len();
    let skip = command_line_ids.len().saturating_sub(retained);
    for (index, line_id) in command_line_ids.into_iter().skip(skip).enumerate() {
        if let Some(line_id) = line_id.filter(|id| buffer.lines().iter().any(|line| line.id == *id))
        {
            state.set_history_line_id(index, line_id);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_history(path: &Path, buffer: &ConsoleBuffer) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    for line in buffer.lines() {
        let (marker, text) = match &line.source {
            ConsoleLineSource::Command { .. } if line.text.starts_with("> ") => {
                ("> ", &line.text[2..])
            }
            _ => ("< ", line.text.as_str()),
        };
        content.push_str(marker);
        content.push_str(text);
        content.push('\n');
    }
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::{load_initial_data, write_history};
    use crate::{ConsoleBuffer, ConsoleConfig, ConsoleLevel, ConsoleLineSource};
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
        let mut saved = ConsoleBuffer::default();
        saved.push(
            ConsoleLevel::Info,
            ConsoleLineSource::Command { name: "map".into() },
            "> map forest",
        );
        saved.push(
            ConsoleLevel::Info,
            ConsoleLineSource::Command { name: "set".into() },
            "> set debug true",
        );
        write_history(&path, &saved).unwrap();
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
    fn transcript_round_trips_interleaved_input_and_output() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "chill_bevy_console_transcript_{}_{}.log",
            std::process::id(),
            unique
        ));
        let mut saved = ConsoleBuffer::default();
        saved.push(
            ConsoleLevel::Info,
            ConsoleLineSource::Command {
                name: "echo".into(),
            },
            "> echo hello",
        );
        saved.push(
            ConsoleLevel::Info,
            ConsoleLineSource::Command {
                name: "echo".into(),
            },
            "hello",
        );
        saved.push(ConsoleLevel::Warn, ConsoleLineSource::System, "low memory");
        write_history(&path, &saved).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "> echo hello\n< hello\n< low memory\n"
        );

        let config = ConsoleConfig {
            history_file: Some(path.clone()),
            ..ConsoleConfig::default()
        };
        let (state, buffer) = load_initial_data(&config);

        assert_eq!(state.command_history(), ["echo hello"]);
        assert_eq!(
            buffer
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["> echo hello", "hello", "low memory"]
        );
        assert_eq!(buffer.lines()[1].source, ConsoleLineSource::System);
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
        let mut saved = ConsoleBuffer::default();
        for command in ["one", "two", "three", "four"] {
            saved.push(
                ConsoleLevel::Info,
                ConsoleLineSource::Command {
                    name: command.into(),
                },
                format!("> {command}"),
            );
        }
        write_history(&path, &saved).unwrap();
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
