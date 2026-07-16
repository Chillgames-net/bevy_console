use crate::CompletionItem;
use bevy::prelude::*;

#[derive(Resource)]
pub struct ConsoleState {
    /// Whether the console can be opened. Set to `false` to disable the toggle
    /// key and force-close the console if it is currently open.
    pub enabled: bool,
    pub open: bool,
    pub input: String,
    /// Rich completion candidates for the current command word or argument.
    pub completion_items: Vec<CompletionItem>,
    /// Number of completion candidates omitted because they exceed the
    /// configured suggestion limit.
    pub completion_overflow: usize,
    /// Index into `completion_items` that is currently highlighted.
    pub match_index: usize,
    /// Set whenever the input changes. The completion system consumes this after
    /// keyboard input so dynamic completers can safely run as Bevy systems.
    pub(crate) completion_dirty: bool,
    /// Cursor position to use for the next completion refresh after accepting
    /// a suggestion. The editable text cursor update is queued and therefore
    /// is not visible until a later system runs.
    pub(crate) completion_cursor: Option<usize>,
    /// When true the history panel auto-scrolls to the newest line.
    pub scroll_follow: bool,
    /// Previously submitted commands, for Up/Down recall.
    pub(crate) cmd_history: Vec<String>,
    /// `Some(i)` while the user is browsing `cmd_history`; `None` otherwise.
    pub(crate) cmd_history_index: Option<usize>,
    /// The input that was live when the user started browsing history,
    /// restored when they navigate back past the newest entry.
    pub(crate) cmd_history_draft: String,
    pub(crate) command_history_revision: u64,
}

impl ConsoleState {
    pub(crate) fn mark_input_changed(&mut self) {
        self.completion_dirty = true;
    }

    pub(crate) fn replace_input(&mut self, input: String) {
        self.input = input;
        self.completion_cursor = None;
        self.mark_input_changed();
    }

    pub(crate) fn clear_input(&mut self) {
        self.input.clear();
        self.completion_cursor = None;
        self.mark_input_changed();
    }

    pub(crate) fn recall_history_matching_input(&mut self) {
        let needle = self.input.to_ascii_lowercase();
        let Some(command) = self
            .cmd_history
            .iter()
            .rev()
            .find(|command| command.to_ascii_lowercase().contains(&needle))
            .cloned()
        else {
            return;
        };
        self.replace_input(command);
    }

    pub(crate) fn record_command(&mut self, command: String, limit: usize) {
        if self.cmd_history.last() == Some(&command) {
            return;
        }
        self.cmd_history.push(command);
        let excess = self.cmd_history.len().saturating_sub(limit);
        if excess > 0 {
            self.cmd_history.drain(..excess);
        }
        self.command_history_revision = self.command_history_revision.wrapping_add(1);
    }

    #[cfg(all(feature = "persistent-history", not(target_arch = "wasm32")))]
    pub(crate) fn restore_command_history(&mut self, commands: Vec<String>, limit: usize) {
        let keep_from = commands.len().saturating_sub(limit);
        self.cmd_history = commands.into_iter().skip(keep_from).collect();
        self.command_history_revision = 0;
    }

    #[cfg(all(feature = "persistent-history", not(target_arch = "wasm32")))]
    pub(crate) fn command_history(&self) -> &[String] {
        &self.cmd_history
    }

    pub(crate) fn set_completions(&mut self, items: Vec<CompletionItem>, overflow: usize) {
        self.completion_items = items;
        self.completion_overflow = overflow;
        if self.match_index >= self.completion_items.len() {
            self.match_index = 0;
        }
        self.completion_dirty = false;
    }

    pub(crate) fn clear_completions(&mut self) {
        self.completion_items.clear();
        self.completion_overflow = 0;
        self.match_index = 0;
        self.completion_cursor = None;
        self.completion_dirty = false;
    }

    /// Applies the selected completion and returns the desired cursor byte offset.
    pub(crate) fn apply_selected_completion(&mut self) -> Option<usize> {
        let item = self.completion_items.get(self.match_index).cloned()?;
        if item.replace.start > item.replace.end
            || item.replace.end > self.input.len()
            || !self.input.is_char_boundary(item.replace.start)
            || !self.input.is_char_boundary(item.replace.end)
        {
            return None;
        }
        let insert_start = item.replace.start;
        let opening_quote = self.input[..insert_start]
            .chars()
            .next_back()
            .filter(|ch| matches!(ch, '\'' | '"'));
        let default_insert = item.insert_text == item.label;
        let insert_text = if default_insert {
            format_completion_text(&item.insert_text, opening_quote)
        } else {
            item.insert_text.clone()
        };
        self.input.replace_range(item.replace, &insert_text);
        let insert_end = insert_start + insert_text.len();
        let mut cursor = insert_end;
        if let Some(opening_quote) = opening_quote {
            if self.input[insert_end..].starts_with(opening_quote) {
                cursor += opening_quote.len_utf8();
            } else if default_insert {
                self.input.insert(cursor, opening_quote);
                cursor += opening_quote.len_utf8();
            }
        }
        if item.append_space
            && !self.input[cursor..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            self.input.insert(cursor, ' ');
            cursor += 1;
        }
        self.completion_cursor = Some(cursor);
        self.mark_input_changed();
        Some(cursor)
    }
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self {
            enabled: true,
            open: false,
            input: String::new(),
            completion_items: Vec::new(),
            completion_overflow: 0,
            match_index: 0,
            completion_dirty: false,
            completion_cursor: None,
            scroll_follow: true,
            cmd_history: Vec::new(),
            cmd_history_index: None,
            cmd_history_draft: String::new(),
            command_history_revision: 0,
        }
    }
}

fn format_completion_text(text: &str, opening_quote: Option<char>) -> String {
    if let Some(quote) = opening_quote {
        return escape_for_quote(text, quote);
    }
    if text
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '\\' | '\'' | '"'))
    {
        format!("\"{}\"", escape_for_quote(text, '"'))
    } else {
        text.to_string()
    }
}

fn escape_for_quote(text: &str, quote: char) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\\' || character == quote {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::ConsoleState;

    #[test]
    fn completion_in_the_middle_preserves_following_input_and_cursor_position() {
        let mut state = ConsoleState::default();
        state.replace_input("map fo now".into());
        state.completion_items = vec![crate::CompletionItem::new("forest", 4..6)];

        assert_eq!(state.apply_selected_completion(), Some(10));
        assert_eq!(state.input, "map forest now");
    }

    #[test]
    fn completion_places_the_cursor_after_a_closing_quote() {
        let mut state = ConsoleState::default();
        state.replace_input("map \"fo\" now".into());
        state.completion_items = vec![crate::CompletionItem::new("forest", 5..7)];

        assert_eq!(state.apply_selected_completion(), Some(12));
        assert_eq!(state.input, "map \"forest\" now");
    }

    #[test]
    fn completion_quotes_argument_values_containing_spaces() {
        let mut state = ConsoleState::default();
        state.replace_input("map fo".into());
        state.completion_items = vec![crate::CompletionItem::new("forest path", 4..6)];

        assert_eq!(state.apply_selected_completion(), Some(18));
        assert_eq!(state.input, "map \"forest path\" ");
    }

    #[test]
    fn completion_closes_an_unterminated_quote() {
        let mut state = ConsoleState::default();
        state.replace_input("map \"fo".into());
        state.completion_items = vec![crate::CompletionItem::new("forest path", 5..7)];

        assert!(state.apply_selected_completion().is_some());
        assert_eq!(state.input, "map \"forest path\" ");
    }

    #[test]
    fn invalid_completion_ranges_are_ignored_without_panicking() {
        let mut state = ConsoleState::default();
        state.replace_input("map fo".into());
        let invalid_start = state.input.len();
        state.completion_items = vec![crate::CompletionItem::new("forest", invalid_start..4)];

        assert_eq!(state.apply_selected_completion(), None);
        assert_eq!(state.input, "map fo");
    }

    #[test]
    fn recalls_matching_history() {
        let mut state = ConsoleState {
            cmd_history: vec!["map forest".into(), "set debug true".into()],
            ..ConsoleState::default()
        };
        state.replace_input("for".into());
        state.recall_history_matching_input();
        assert_eq!(state.input, "map forest");
    }
}
