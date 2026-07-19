//! Optional console transcript persistence.

use crate::{ConsoleBuffer, ConsoleConfig, ConsoleState};
#[cfg(not(target_arch = "wasm32"))]
use crate::{ConsoleLevel, ConsoleLineSource, ParsedInput};
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
/// Coalesce frequent console output while still saving during a continuous stream.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(250);

/// Settings for optional console transcript persistence.
#[derive(Resource, Clone)]
pub struct ConsolePersistence {
    /// Path to the plain-text transcript and command-recall history.
    pub history_file: PathBuf,
    /// Maximum transcript rows written to disk.
    pub max_saved_lines: usize,
    /// Save command recall without writing any transcript rows.
    pub recall_only: bool,
    /// Maximum character length of each saved transcript row. `None` preserves
    /// the entire row.
    pub max_line_length: Option<usize>,
}

impl ConsolePersistence {
    pub fn new(history_file: PathBuf) -> Self {
        Self {
            history_file,
            ..default()
        }
    }
}

impl Default for ConsolePersistence {
    fn default() -> Self {
        Self {
            history_file: PathBuf::from("console_history.txt"),
            max_saved_lines: 256,
            recall_only: false,
            max_line_length: None,
        }
    }
}

pub(crate) fn plugin(_app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    _app.add_systems(Update, persist_history);
}

pub(crate) fn load_initial_data(
    config: &ConsoleConfig,
    persistence: &ConsolePersistence,
) -> (ConsoleState, ConsoleBuffer) {
    #[cfg(not(target_arch = "wasm32"))]
    let mut state = ConsoleState::default();
    #[cfg(target_arch = "wasm32")]
    let state = ConsoleState::default();
    #[cfg(not(target_arch = "wasm32"))]
    let mut buffer = ConsoleBuffer::new(config.max_history_lines);
    #[cfg(target_arch = "wasm32")]
    let buffer = ConsoleBuffer::new(config.max_history_lines);

    #[cfg(not(target_arch = "wasm32"))]
    match std::fs::read_to_string(&persistence.history_file) {
        Ok(content) => {
            if restore_transcript(&content, &mut state, &mut buffer, config).is_err() {
                warn!(
                    "chill_bevy_console: discarding invalid history file {:?}",
                    persistence.history_file
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => warn!(
            "chill_bevy_console: failed to read history file {:?}: {}",
            persistence.history_file, error
        ),
    }

    (state, buffer)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PersistenceTracker {
    initialized: bool,
    revision: u64,
    path: PathBuf,
    max_saved_lines: usize,
    recall_only: bool,
    max_line_length: Option<usize>,
    dirty: bool,
    save_at: Duration,
    task: Option<Task<(PathBuf, std::io::Result<()>)>>,
}

#[cfg(not(target_arch = "wasm32"))]
fn persist_history(
    persistence: Res<ConsolePersistence>,
    state: Res<ConsoleState>,
    buffer: Res<ConsoleBuffer>,
    time: Res<Time<Real>>,
    mut tracker: Local<PersistenceTracker>,
) {
    if let Some(task) = tracker.task.as_mut()
        && let Some((path, result)) = block_on(future::poll_once(task))
    {
        tracker.task = None;
        if let Err(error) = result {
            warn!(
                "chill_bevy_console: failed to write history file {:?}: {}",
                path, error
            );
        }
    }

    let path = persistence.history_file.clone();
    if !tracker.initialized {
        tracker.initialized = true;
        tracker.revision = state.command_history_revision;
        tracker.path = path;
        tracker.max_saved_lines = persistence.max_saved_lines;
        tracker.recall_only = persistence.recall_only;
        tracker.max_line_length = persistence.max_line_length;
        return;
    }
    let settings_changed = tracker.path != path
        || tracker.max_saved_lines != persistence.max_saved_lines
        || tracker.recall_only != persistence.recall_only
        || tracker.max_line_length != persistence.max_line_length;
    let transcript_changed = !persistence.recall_only && buffer.is_changed();
    if tracker.revision == state.command_history_revision
        && !settings_changed
        && !transcript_changed
    {
        if tracker.dirty && tracker.task.is_none() && time.elapsed() >= tracker.save_at {
            start_history_write(&mut tracker, &state, &buffer, &persistence);
        }
        return;
    }

    tracker.revision = state.command_history_revision;
    tracker.path.clone_from(&path);
    tracker.max_saved_lines = persistence.max_saved_lines;
    tracker.recall_only = persistence.recall_only;
    tracker.max_line_length = persistence.max_line_length;
    // Do not push the deadline forward for every line: a busy console should
    // save at most once per interval instead of waiting forever for quiet.
    if !tracker.dirty {
        tracker.save_at = time.elapsed() + SAVE_DEBOUNCE;
    }
    tracker.dirty = true;
}

#[cfg(not(target_arch = "wasm32"))]
fn start_history_write(
    tracker: &mut PersistenceTracker,
    state: &ConsoleState,
    buffer: &ConsoleBuffer,
    persistence: &ConsolePersistence,
) {
    let path = persistence.history_file.clone();
    let content = history_content(state, buffer, persistence);
    tracker.dirty = false;
    tracker.task = Some(IoTaskPool::get().spawn(async move {
        let result = write_history_content(&path, &content);
        (path, result)
    }));
}

#[cfg(not(target_arch = "wasm32"))]
fn write_history_content(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

#[cfg(not(target_arch = "wasm32"))]
fn history_content(
    state: &ConsoleState,
    buffer: &ConsoleBuffer,
    persistence: &ConsolePersistence,
) -> String {
    let mut content = String::new();
    for command in state.command_history() {
        content.push_str("H\t");
        content.push_str(&encode_text(command));
        content.push('\n');
    }
    if persistence.recall_only {
        return content;
    }

    let first_saved_line = buffer
        .lines()
        .len()
        .saturating_sub(persistence.max_saved_lines);
    for line in buffer.lines().iter().skip(first_saved_line) {
        let (kind, text) = match &line.source {
            ConsoleLineSource::CommandEcho { .. } => (
                "C",
                line.text.strip_prefix("> ").unwrap_or(line.text.as_str()),
            ),
            _ => (output_kind(line.level), line.text.as_str()),
        };
        content.push_str(kind);
        content.push('\t');
        content.push_str(&encode_text(truncate_text(
            text,
            persistence.max_line_length,
        )));
        content.push('\n');
    }
    content
}

#[cfg(not(target_arch = "wasm32"))]
const fn output_kind(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Trace => "T",
        ConsoleLevel::Debug => "D",
        ConsoleLevel::Info => "I",
        ConsoleLevel::Warn => "W",
        ConsoleLevel::Error => "E",
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
fn write_history_with_settings(
    path: &Path,
    state: &ConsoleState,
    buffer: &ConsoleBuffer,
    persistence: &ConsolePersistence,
) -> std::io::Result<()> {
    write_history_content(path, &history_content(state, buffer, persistence))
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_transcript(
    content: &str,
    state: &mut ConsoleState,
    buffer: &mut ConsoleBuffer,
    config: &ConsoleConfig,
) -> Result<(), ()> {
    let mut restored_state = ConsoleState::default();
    let mut restored_buffer = ConsoleBuffer::new(buffer.max_lines());
    restore_history(content, &mut restored_state, &mut restored_buffer, config)?;
    *state = restored_state;
    *buffer = restored_buffer;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn restore_history(
    content: &str,
    state: &mut ConsoleState,
    buffer: &mut ConsoleBuffer,
    config: &ConsoleConfig,
) -> Result<(), ()> {
    let mut commands = Vec::new();
    let mut command_echoes = Vec::new();

    for record in content.lines() {
        let Some((kind, encoded)) = record.split_once('\t') else {
            return Err(());
        };
        if encoded.contains('\t') {
            return Err(());
        }
        let level = match kind {
            "C" | "H" => ConsoleLevel::Info,
            "T" => ConsoleLevel::Trace,
            "D" => ConsoleLevel::Debug,
            "I" => ConsoleLevel::Info,
            "W" => ConsoleLevel::Warn,
            "E" => ConsoleLevel::Error,
            _ => return Err(()),
        };
        let Some(text) = decode_text(encoded) else {
            return Err(());
        };
        match kind {
            "H" => commands.push(text),
            "C" => {
                let name = ParsedInput::parse(&text)
                    .command()
                    .unwrap_or_default()
                    .to_string();
                buffer.push(
                    level,
                    ConsoleLineSource::CommandEcho { name },
                    format!("> {text}"),
                );
                command_echoes.push((text, buffer.last_line().map(|line| line.id)));
            }
            "T" | "D" | "I" | "W" | "E" => buffer.push(level, ConsoleLineSource::System, text),
            _ => return Err(()),
        }
    }

    state.restore_command_history(commands, config.max_command_history);
    link_restored_echoes(state, buffer, command_echoes);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn link_restored_echoes(
    state: &mut ConsoleState,
    buffer: &ConsoleBuffer,
    echoes: Vec<(String, Option<u64>)>,
) {
    let mut end = state.command_history().len();
    for (command, line_id) in echoes.into_iter().rev() {
        let Some(index) = state.command_history()[..end]
            .iter()
            .rposition(|candidate| candidate == &command)
        else {
            continue;
        };
        end = index;
        if let Some(line_id) = line_id.filter(|id| buffer.lines().iter().any(|line| line.id == *id))
        {
            state.set_history_line_id(index, line_id);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
fn write_history(path: &Path, state: &ConsoleState, buffer: &ConsoleBuffer) -> std::io::Result<()> {
    write_history_with_settings(
        path,
        state,
        buffer,
        &ConsolePersistence {
            max_saved_lines: usize::MAX,
            ..ConsolePersistence::default()
        },
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn truncate_text(text: &str, max_line_length: Option<usize>) -> &str {
    let Some(max_line_length) = max_line_length else {
        return text;
    };
    text.char_indices()
        .nth(max_line_length)
        .map_or(text, |(index, _)| &text[..index])
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_text(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            _ => encoded.push(character),
        }
    }
    encoded
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_text(encoded: &str) -> Option<String> {
    let mut decoded = String::with_capacity(encoded.len());
    let mut characters = encoded.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => decoded.push('\\'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            _ => return None,
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::{load_initial_data, write_history, write_history_with_settings};
    use crate::{
        ConsoleBuffer, ConsoleConfig, ConsoleLevel, ConsoleLineSource, ConsolePersistence,
        ConsoleState,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn state_with_history(commands: &[&str]) -> ConsoleState {
        let mut state = ConsoleState::default();
        for command in commands {
            state.record_command((*command).to_owned(), usize::MAX);
        }
        state
    }

    fn load(persistence: &ConsolePersistence) -> (ConsoleState, ConsoleBuffer) {
        load_initial_data(&ConsoleConfig::default(), persistence)
    }

    fn persistence(path: std::path::PathBuf) -> ConsolePersistence {
        ConsolePersistence {
            history_file: path,
            ..ConsolePersistence::default()
        }
    }

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
            ConsoleLineSource::CommandEcho { name: "map".into() },
            "> map forest",
        );
        saved.push(
            ConsoleLevel::Info,
            ConsoleLineSource::CommandEcho { name: "set".into() },
            "> set debug true",
        );
        let state = state_with_history(&["map forest", "set debug true"]);
        write_history(&path, &state, &saved).unwrap();
        let (state, buffer) = load(&persistence(path.clone()));

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
            ConsoleLineSource::CommandEcho { name: "map".into() }
        );
        assert_eq!(
            buffer.lines()[1].source,
            ConsoleLineSource::CommandEcho { name: "set".into() }
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
            ConsoleLineSource::CommandEcho {
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
        let state = state_with_history(&["echo hello"]);
        write_history(&path, &state, &saved).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "H\techo hello\nC\techo hello\nI\thello\nW\tlow memory\n"
        );

        let (state, buffer) = load(&persistence(path.clone()));

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
        assert_eq!(buffer.lines()[2].level, ConsoleLevel::Warn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn transcript_round_trips_each_log_level() {
        let path = unique_history_path("levels");
        let state = ConsoleState::default();
        let mut buffer = ConsoleBuffer::default();
        for level in ConsoleLevel::ALL {
            buffer.push(level, ConsoleLineSource::System, level.as_str());
        }

        write_history(&path, &state, &buffer).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "T\ttrace\nD\tdebug\nI\tinfo\nW\twarn\nE\terror\n"
        );
        let (_, restored_buffer) = load(&persistence(path.clone()));

        assert_eq!(
            restored_buffer
                .lines()
                .iter()
                .map(|line| line.level)
                .collect::<Vec<_>>(),
            ConsoleLevel::ALL
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn old_record_formats_are_discarded() {
        let path = unique_history_path("old_format");
        std::fs::write(&path, "H\tstatus\nO\twarn\tready\n").unwrap();

        let (state, buffer) = load(&persistence(path.clone()));

        assert!(state.command_history().is_empty());
        assert!(buffer.lines().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saved_transcript_rows_are_limited_independently_of_live_history() {
        let path = unique_history_path("saved_limit");
        let state = state_with_history(&["one", "two"]);
        let mut buffer = ConsoleBuffer::default();
        for text in ["first", "second", "third"] {
            buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, text);
        }

        write_history_with_settings(
            &path,
            &state,
            &buffer,
            &ConsolePersistence {
                max_saved_lines: 2,
                ..ConsolePersistence::default()
            },
        )
        .unwrap();
        let (restored, restored_buffer) = load(&persistence(path.clone()));

        assert_eq!(restored.command_history(), ["one", "two"]);
        assert_eq!(
            restored_buffer
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["second", "third"]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn recall_only_omits_transcript_rows() {
        let path = unique_history_path("recall_only");
        let state = state_with_history(&["status"]);
        let mut buffer = ConsoleBuffer::default();
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "ready");
        let persistence = ConsolePersistence {
            history_file: path.clone(),
            recall_only: true,
            ..ConsolePersistence::default()
        };

        write_history_with_settings(&path, &state, &buffer, &persistence).unwrap();
        let (restored, restored_buffer) = load(&persistence);

        assert_eq!(restored.command_history(), ["status"]);
        assert!(restored_buffer.lines().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn saved_rows_respect_the_character_length_limit() {
        let path = unique_history_path("line_length");
        let state = ConsoleState::default();
        let mut buffer = ConsoleBuffer::default();
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "héllo");
        let persistence = ConsolePersistence {
            history_file: path.clone(),
            max_line_length: Some(2),
            ..ConsolePersistence::default()
        };

        write_history_with_settings(&path, &state, &buffer, &persistence).unwrap();
        let (_, restored_buffer) = load(&persistence);

        assert_eq!(restored_buffer.lines()[0].text, "hé");
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
                ConsoleLineSource::CommandEcho {
                    name: command.into(),
                },
                format!("> {command}"),
            );
        }
        let state = state_with_history(&["one", "two", "three", "four"]);
        write_history(&path, &state, &saved).unwrap();
        let config = ConsoleConfig {
            max_command_history: 3,
            max_history_lines: 2,
            ..ConsoleConfig::default()
        };

        let (state, buffer) = load_initial_data(&config, &persistence(path.clone()));

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

    #[test]
    fn clearing_display_preserves_command_recall_on_restart() {
        let path = unique_history_path("clear");
        let state = state_with_history(&["map forest", "set debug true"]);
        let mut buffer = ConsoleBuffer::default();
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "old output");
        buffer.clear();
        write_history(&path, &state, &buffer).unwrap();

        let (restored, restored_buffer) = load(&persistence(path.clone()));

        assert_eq!(restored.command_history(), ["map forest", "set debug true"]);
        assert!(restored_buffer.lines().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn command_recall_is_not_limited_by_visual_buffer() {
        let path = unique_history_path("buffer_limit");
        let state = state_with_history(&["one", "two", "three", "four"]);
        let mut buffer = ConsoleBuffer::new(2);
        for command in ["one", "two", "three", "four"] {
            buffer.push(
                ConsoleLevel::Info,
                ConsoleLineSource::CommandEcho {
                    name: command.into(),
                },
                format!("> {command}"),
            );
        }
        write_history(&path, &state, &buffer).unwrap();

        let (restored, restored_buffer) = load_initial_data(
            &ConsoleConfig {
                max_command_history: 4,
                max_history_lines: 2,
                ..ConsoleConfig::default()
            },
            &persistence(path.clone()),
        );

        assert_eq!(restored.command_history(), ["one", "two", "three", "four"]);
        assert_eq!(
            restored_buffer
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["> three", "> four"]
        );
        assert_eq!(
            restored.cmd_history_line_ids,
            [None, None, Some(1), Some(2)]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn output_starting_with_a_prompt_is_not_restored_as_recall() {
        let path = unique_history_path("prompt_output");
        let state = state_with_history(&["status"]);
        let mut buffer = ConsoleBuffer::default();
        buffer.push(
            ConsoleLevel::Info,
            ConsoleLineSource::CommandEcho {
                name: "status".into(),
            },
            "> status",
        );
        buffer.push(
            ConsoleLevel::Info,
            ConsoleLineSource::Command {
                name: "status".into(),
            },
            "> ready",
        );
        write_history(&path, &state, &buffer).unwrap();

        let (restored, restored_buffer) = load(&persistence(path.clone()));

        assert_eq!(restored.command_history(), ["status"]);
        assert_eq!(restored_buffer.lines()[1].text, "> ready");
        assert_eq!(restored_buffer.lines()[1].source, ConsoleLineSource::System);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_transcripts_are_discarded() {
        let path = unique_history_path("legacy");
        std::fs::write(&path, "> map forest\n< loaded\nset debug true\n").unwrap();

        let (state, buffer) = load(&persistence(path.clone()));

        assert!(state.command_history().is_empty());
        assert!(buffer.lines().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_transcripts_are_discarded_without_partial_restore() {
        let path = unique_history_path("malformed");
        std::fs::write(&path, "H\tstatus\nE\tstatus\nnot a record\n").unwrap();

        let (state, buffer) = load(&persistence(path.clone()));

        assert!(state.command_history().is_empty());
        assert!(buffer.lines().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_escapes_record_separators_without_losing_text() {
        let path = unique_history_path("escaped");
        let state = state_with_history(&["say \\ hello\tworld"]);
        let mut buffer = ConsoleBuffer::default();
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "a\tb\\c");
        write_history(&path, &state, &buffer).unwrap();

        let (restored, restored_buffer) = load(&persistence(path.clone()));

        assert_eq!(restored.command_history(), ["say \\ hello\tworld"]);
        assert_eq!(restored_buffer.lines()[0].text, "a\tb\\c");
        std::fs::remove_file(path).unwrap();
    }

    fn unique_history_path(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "chill_bevy_console_{label}_{}_{}.txt",
            std::process::id(),
            unique
        ))
    }
}
