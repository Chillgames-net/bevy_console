//! Public data models shared by command registration, completion, execution,
//! logging, and the console UI.

use crate::parser::ParsedInput;
use bevy::ecs::system::{BoxedSystem, IntoSystem};
use bevy::prelude::{ButtonInput, KeyCode, Message, Resource};
use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

/// Describes the expected shape of an argument for help and completion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArgumentKind {
    #[default]
    String,
    Boolean,
    /// One of the values supplied on [`ArgumentSpec`].
    Choice,
}

/// Metadata for one command argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentSpec {
    pub name: &'static str,
    pub help: &'static str,
    pub kind: ArgumentKind,
    pub choices: Vec<&'static str>,
}

impl ArgumentSpec {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            help: "",
            kind: ArgumentKind::String,
            choices: Vec::new(),
        }
    }

    pub fn help(mut self, help: &'static str) -> Self {
        self.help = help;
        self
    }

    pub fn kind(mut self, kind: ArgumentKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn choices(mut self, choices: impl IntoIterator<Item = &'static str>) -> Self {
        self.kind = ArgumentKind::Choice;
        self.choices = choices.into_iter().collect();
        self
    }
}

/// Structured metadata retained by the registry after a command is registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandSpec {
    pub(crate) name: String,
    pub(crate) usage: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) long_help: Option<&'static str>,
    pub(crate) aliases: Vec<&'static str>,
    pub(crate) args: Vec<ArgumentSpec>,
    pub(crate) hidden: bool,
}

impl CommandSpec {
    pub(crate) fn new(name: impl Into<String>, usage: &'static str) -> Self {
        Self {
            name: name.into(),
            usage,
            summary: usage,
            long_help: None,
            aliases: Vec::new(),
            args: Vec::new(),
            hidden: false,
        }
    }
}

/// Builder for a console command and its optional completion system.
///
/// Register the completed builder through
/// [`crate::ConsoleAppExt::add_console_command`].
pub struct ConsoleCommand {
    pub(crate) spec: CommandSpec,
    pub(crate) system: BoxedSystem<bevy::prelude::In<crate::Args>, crate::ConsoleResult>,
    pub(crate) completer: Option<BoxedSystem<crate::ConsoleCompletionRequest, Vec<CompletionItem>>>,
}

impl ConsoleCommand {
    /// Creates a command builder with no dynamic completion system.
    pub fn new<S, M, O>(name: impl Into<String>, help: &'static str, system: S) -> Self
    where
        S: IntoSystem<bevy::prelude::In<crate::Args>, O, M> + 'static,
        O: Into<crate::ConsoleResult> + 'static,
    {
        Self {
            spec: CommandSpec::new(name, help),
            system: Box::new(IntoSystem::into_system(
                system.map(into_console_result::<O>),
            )),
            completer: None,
        }
    }

    /// Attaches the command's dynamic completion system.
    pub fn with_completions<C, M, O>(mut self, completer: C) -> Self
    where
        C: IntoSystem<crate::ConsoleCompletionRequest, O, M> + 'static,
        O: IntoIterator + 'static,
        O::Item: Into<CompletionItem>,
    {
        self.completer = Some(Box::new(IntoSystem::into_system(
            completer.map(into_completion_items::<O>),
        )));
        self
    }

    /// Sets the short description shown alongside command completion.
    pub fn with_summary(mut self, summary: &'static str) -> Self {
        self.spec.summary = summary;
        self
    }

    /// Sets extended text displayed by the built-in help command.
    pub fn with_long_help(mut self, help: &'static str) -> Self {
        self.spec.long_help = Some(help);
        self
    }

    /// Adds an alternate name for this command.
    pub fn with_alias(mut self, alias: &'static str) -> Self {
        self.spec.aliases.push(alias);
        self
    }

    /// Sets structured metadata for the command arguments.
    pub fn with_args(mut self, args: impl IntoIterator<Item = ArgumentSpec>) -> Self {
        self.spec.args = args.into_iter().collect();
        self
    }

    /// Hides this command from command completion.
    pub fn hidden(mut self) -> Self {
        self.spec.hidden = true;
        self
    }
}

fn into_console_result<O: Into<crate::ConsoleResult>>(output: O) -> crate::ConsoleResult {
    output.into()
}

fn into_completion_items<O>(items: O) -> Vec<CompletionItem>
where
    O: IntoIterator,
    O::Item: Into<CompletionItem>,
{
    items.into_iter().map(Into::into).collect()
}

/// The source that requested a command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandOrigin {
    #[default]
    LocalUi,
    Application,
}

/// A command queued by the console UI or game code.
#[derive(Debug, Clone, Message, PartialEq, Eq)]
pub struct ConsoleRequest {
    pub input: String,
    pub origin: CommandOrigin,
}

/// FIFO queue shared by local input and programmatic [`ConsoleRequest`]
/// messages. Keeping this separate from UI state allows commands to execute
/// without pretending to type into the console.
#[derive(Debug, Default, Resource)]
pub(crate) struct ConsoleCommandQueue {
    requests: VecDeque<QueuedConsoleRequest>,
}

#[derive(Debug)]
pub(crate) struct QueuedConsoleRequest {
    pub(crate) request: ConsoleRequest,
    pub(crate) alias_depth: u8,
    /// The recalled history entry whose echo should be linked after alias expansion.
    pub(crate) history_index: Option<usize>,
}

/// User-defined command expansions. Unlike aliases declared during command
/// registration, these can be created and removed at runtime through the
/// `alias` commands.
#[derive(Debug, Default, Resource)]
pub struct ConsoleAliases {
    aliases: BTreeMap<String, String>,
}

/// Modifier keys required for a [`ConsoleKeyBinding`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConsoleKeyModifiers {
    pub ctrl: bool,
    /// The platform shortcut modifier: Command on macOS and Control elsewhere.
    pub meta: bool,
    pub shift: bool,
    pub alt: bool,
}

impl ConsoleKeyModifiers {
    pub(crate) fn matches(self, keys: &ButtonInput<KeyCode>) -> bool {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let super_key = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);
        let primary_modifiers_match = if cfg!(target_os = "macos") {
            self.ctrl == ctrl && self.meta == super_key
        } else {
            // On non-macOS platforms, Meta is an alias for Control. Both bindings may
            // intentionally refer to the same shortcut.
            (self.ctrl || self.meta) == ctrl && !super_key
        };

        primary_modifiers_match
            && self.shift == (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight))
            && self.alt == (keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight))
    }
}

/// A physical key and its required modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConsoleKeyBinding {
    pub key: KeyCode,
    pub modifiers: ConsoleKeyModifiers,
}

impl ConsoleKeyBinding {
    pub const fn new(key: KeyCode) -> Self {
        Self {
            key,
            modifiers: ConsoleKeyModifiers {
                ctrl: false,
                meta: false,
                shift: false,
                alt: false,
            },
        }
    }
}

impl std::fmt::Display for ConsoleKeyBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.modifiers.ctrl {
            write!(formatter, "ctrl+")?;
        }
        if self.modifiers.meta {
            write!(formatter, "meta+")?;
        }
        if self.modifiers.shift {
            write!(formatter, "shift+")?;
        }
        if self.modifiers.alt {
            write!(formatter, "alt+")?;
        }
        write!(formatter, "{:?}", self.key)
    }
}

/// User-defined key bindings created through the `bind` command.
///
/// Bindings run their command when the assigned physical key is pressed while
/// the console is closed. They are not persisted between runs.
#[derive(Debug, Default, Resource)]
pub struct ConsoleBinds {
    binds: BTreeMap<ConsoleKeyBinding, String>,
}

impl ConsoleBinds {
    /// Assign a command to a key without modifiers.
    pub fn set(&mut self, key: KeyCode, command: impl Into<String>) -> Option<String> {
        self.set_binding(ConsoleKeyBinding::new(key), command)
    }

    /// Assign a command to a key and modifier combination.
    pub fn set_binding(
        &mut self,
        binding: ConsoleKeyBinding,
        command: impl Into<String>,
    ) -> Option<String> {
        self.binds.insert(binding, command.into())
    }

    /// Get the command assigned to a key without modifiers.
    pub fn get(&self, key: KeyCode) -> Option<&str> {
        self.get_binding(ConsoleKeyBinding::new(key))
    }

    /// Get the command assigned to a key and modifier combination.
    pub fn get_binding(&self, binding: ConsoleKeyBinding) -> Option<&str> {
        self.binds.get(&binding).map(String::as_str)
    }

    /// Remove a binding without modifiers.
    pub fn remove(&mut self, key: KeyCode) -> Option<String> {
        self.remove_binding(ConsoleKeyBinding::new(key))
    }

    /// Remove a key and modifier combination.
    pub fn remove_binding(&mut self, binding: ConsoleKeyBinding) -> Option<String> {
        self.binds.remove(&binding)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ConsoleKeyBinding, &str)> + '_ {
        self.binds
            .iter()
            .map(|(binding, command)| (*binding, command.as_str()))
    }
}

impl ConsoleAliases {
    pub fn set(&mut self, name: impl Into<String>, expansion: impl Into<String>) {
        self.aliases
            .insert(name.into().to_ascii_lowercase(), expansion.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.aliases
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.aliases.remove(&name.to_ascii_lowercase())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(|(name, expansion)| (name.as_str(), expansion.as_str()))
    }
}

impl ConsoleCommandQueue {
    pub(crate) fn push(&mut self, request: ConsoleRequest) {
        self.requests.push_back(QueuedConsoleRequest {
            request,
            alias_depth: 0,
            history_index: None,
        });
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.requests.len()
    }

    pub(crate) fn pop_front(&mut self) -> Option<QueuedConsoleRequest> {
        self.requests.pop_front()
    }

    pub(crate) fn push_alias_expansion(
        &mut self,
        request: ConsoleRequest,
        alias_depth: u8,
        history_index: Option<usize>,
    ) {
        self.requests.push_front(QueuedConsoleRequest {
            request,
            alias_depth,
            history_index,
        });
    }
}

/// Notification emitted after a command has been parsed and executed.
#[derive(Debug, Clone, Message, PartialEq, Eq)]
pub struct ConsoleCommandExecuted {
    pub input: String,
    pub command: Option<String>,
    pub origin: CommandOrigin,
    pub succeeded: bool,
}

impl ConsoleRequest {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            origin: CommandOrigin::Application,
        }
    }
}

/// Input supplied to a command completer.
///
/// A request is only created while completing an argument of a registered
/// command, so [`Self::command`] and [`Self::argument_index`] are always
/// available. The parser result remains public for advanced completion logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    pub parsed: ParsedInput,
    command: String,
    argument_index: usize,
}

impl CompletionRequest {
    pub(crate) fn new(parsed: ParsedInput, command: String, argument_index: usize) -> Self {
        Self {
            parsed,
            command,
            argument_index,
        }
    }

    /// The command being completed, as written in the input.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The zero-based index of the argument being completed.
    pub fn argument_index(&self) -> usize {
        self.argument_index
    }

    /// Returns a decoded argument value by zero-based index.
    pub fn argument(&self, index: usize) -> Option<&str> {
        self.parsed
            .tokens
            .get(index.checked_add(1)?)
            .map(|token| token.value.as_str())
    }

    /// The decoded text of the argument currently being completed.
    pub fn active_fragment(&self) -> &str {
        self.parsed.active_fragment()
    }

    /// The source range that completion should replace.
    pub fn replacement_range(&self) -> Range<usize> {
        self.parsed.replacement_range()
    }
}

/// A completion candidate returned by a command completer.
///
/// The default `insert_text` is quoted and escaped automatically when needed.
/// Use [`Self::insert_text`] when the completion intentionally inserts command
/// syntax rather than one argument value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub(crate) label: String,
    pub(crate) insert_text: String,
    pub(crate) detail: String,
    pub(crate) replace: Range<usize>,
    pub(crate) append_space: bool,
}

impl CompletionItem {
    /// Creates a candidate with its display label and detail text.
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            insert_text: label.clone(),
            label,
            detail: detail.into(),
            replace: 0..0,
            append_space: true,
        }
    }

    /// Sets text inserted in place of the candidate label.
    pub fn insert_text(mut self, text: impl Into<String>) -> Self {
        self.insert_text = text.into();
        self
    }

    /// Controls whether accepting this candidate appends a space.
    pub fn append_space(mut self, append_space: bool) -> Self {
        self.append_space = append_space;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_replace(mut self, replace: Range<usize>) -> Self {
        self.replace = replace;
        self
    }

    pub(crate) fn set_replace(&mut self, replace: Range<usize>) {
        self.replace = replace;
    }
}

impl From<String> for CompletionItem {
    fn from(label: String) -> Self {
        Self::new(label, "")
    }
}

impl From<&str> for CompletionItem {
    fn from(label: &str) -> Self {
        Self::new(label, "")
    }
}

/// Severity for a line in the console output buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsoleLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl ConsoleLevel {
    pub const ALL: [Self; 5] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Warn,
        Self::Error,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl std::str::FromStr for ConsoleLevel {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(()),
        }
    }
}

/// Why a line was written to the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleLineSource {
    /// The prompt line echoed when a command is submitted.
    CommandEcho {
        name: String,
    },
    /// Output produced while executing a command.
    Command {
        name: String,
    },
    Log {
        target: String,
    },
    System,
}

/// One structured line in the output buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    pub id: u64,
    pub level: ConsoleLevel,
    pub source: ConsoleLineSource,
    pub text: String,
}

/// A line submitted by game systems or a log integration. The console consumes
/// these messages into [`ConsoleBuffer`] once per frame.
#[derive(Debug, Clone, Message, PartialEq, Eq)]
pub struct ConsoleLineMessage {
    pub level: ConsoleLevel,
    pub source: ConsoleLineSource,
    pub text: String,
}

impl ConsoleLineMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            level: ConsoleLevel::Info,
            source: ConsoleLineSource::System,
            text: text.into(),
        }
    }
}

/// Bounded structured output retained by the console.
#[derive(Debug, Clone, Resource)]
pub struct ConsoleBuffer {
    lines: VecDeque<ConsoleLine>,
    max_lines: usize,
    next_id: u64,
}

impl Default for ConsoleBuffer {
    fn default() -> Self {
        Self::new(256)
    }
}

impl ConsoleBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            max_lines,
            next_id: 0,
        }
    }

    pub fn max_lines(&self) -> usize {
        self.max_lines
    }
    pub fn set_max_lines(&mut self, max_lines: usize) {
        self.max_lines = max_lines;
        self.trim();
    }
    pub fn lines(&self) -> &VecDeque<ConsoleLine> {
        &self.lines
    }

    /// Returns the most recently appended output line, if any.
    pub fn last_line(&self) -> Option<&ConsoleLine> {
        self.lines.back()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn push(&mut self, level: ConsoleLevel, source: ConsoleLineSource, text: impl AsRef<str>) {
        for text in text.as_ref().lines() {
            self.next_id = self.next_id.wrapping_add(1);
            self.lines.push_back(ConsoleLine {
                id: self.next_id,
                level,
                source: source.clone(),
                text: text.to_string(),
            });
        }
        self.trim();
    }

    fn trim(&mut self) {
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }
}

/// Structured output returned by advanced commands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleResult {
    pub lines: Vec<(ConsoleLevel, String)>,
}

impl ConsoleResult {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            lines: vec![(ConsoleLevel::Info, text.into())],
        }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            lines: vec![(ConsoleLevel::Error, text.into())],
        }
    }
    pub fn line(mut self, level: ConsoleLevel, text: impl Into<String>) -> Self {
        self.lines.push((level, text.into()));
        self
    }
}

impl From<String> for ConsoleResult {
    fn from(text: String) -> Self {
        Self::info(text)
    }
}
