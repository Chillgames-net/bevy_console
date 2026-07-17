use crate::CompletionItem;
use bevy::prelude::*;

#[derive(Resource)]
pub struct ConsoleState {
    /// Whether the console can be opened. Set to `false` to disable the toggle
    /// key and force-close the console if it is currently open.
    pub enabled: bool,
    pub open: bool,
    pub input: String,
    /// All ranked completion candidates for the current command word or argument.
    pub completion_items: Vec<CompletionItem>,
    /// Number of completion candidates beyond the first visible suggestion page.
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
    /// The echoed output line for each recalled command, when it is still in
    /// the output buffer. Kept alongside `cmd_history` so selection can be
    /// rendered without guessing from duplicate command text.
    pub(crate) cmd_history_line_ids: Vec<Option<u64>>,
    /// `Some(i)` while the user is browsing `cmd_history`; `None` otherwise.
    pub(crate) cmd_history_index: Option<usize>,
    /// The input that was live when the user started browsing history,
    /// restored when they navigate back past the newest entry.
    pub(crate) cmd_history_draft: String,
    /// The history item submitted from the input bar that is waiting for its
    /// command echo to be written.
    pub(crate) pending_history_index: Option<usize>,
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
            self.pending_history_index = self.cmd_history.len().checked_sub(1);
            return;
        }
        self.cmd_history.push(command);
        self.cmd_history_line_ids.push(None);
        let excess = self.cmd_history.len().saturating_sub(limit);
        if excess > 0 {
            self.cmd_history.drain(..excess);
            self.cmd_history_line_ids.drain(..excess);
        }
        self.pending_history_index = self.cmd_history.len().checked_sub(1);
        self.command_history_revision = self.command_history_revision.wrapping_add(1);
    }

    pub(crate) fn take_pending_history_index(&mut self, command: &str) -> Option<usize> {
        let index = self.pending_history_index.take()?;
        (self.cmd_history.get(index).map(String::as_str) == Some(command)).then_some(index)
    }

    pub(crate) fn set_history_line_id(&mut self, index: usize, line_id: u64) {
        if let Some(id) = self.cmd_history_line_ids.get_mut(index) {
            *id = Some(line_id);
        }
    }

    pub(crate) fn selected_history_line_id(&self) -> Option<u64> {
        self.cmd_history_index
            .and_then(|index| self.cmd_history_line_ids.get(index).copied().flatten())
    }

    /// Clears command recall and marks persisted history stale.
    #[cfg(feature = "persistent-history")]
    pub(crate) fn clear_command_history(&mut self) {
        self.cmd_history.clear();
        self.cmd_history_line_ids.clear();
        self.cmd_history_index = None;
        self.cmd_history_draft.clear();
        self.pending_history_index = None;
        self.command_history_revision = self.command_history_revision.wrapping_add(1);
    }

    #[cfg(all(feature = "persistent-history", not(target_arch = "wasm32")))]
    pub(crate) fn restore_command_history(&mut self, commands: Vec<String>, limit: usize) {
        let keep_from = commands.len().saturating_sub(limit);
        self.cmd_history = commands.into_iter().skip(keep_from).collect();
        self.cmd_history_line_ids = vec![None; self.cmd_history.len()];
        self.command_history_revision = 0;
    }

    #[cfg(all(feature = "persistent-history", not(target_arch = "wasm32")))]
    pub(crate) fn command_history(&self) -> &[String] {
        &self.cmd_history
    }

    pub(crate) fn set_completions(&mut self, items: Vec<CompletionItem>, overflow: usize) {
        self.completion_items = items;
        self.completion_overflow = overflow;
        self.match_index = 0;
        self.completion_dirty = false;
    }

    pub(crate) fn select_previous_completion(&mut self) {
        if !self.completion_items.is_empty() {
            self.match_index =
                (self.match_index + self.completion_items.len() - 1) % self.completion_items.len();
        }
    }

    pub(crate) fn select_next_completion(&mut self) {
        if !self.completion_items.is_empty() {
            self.match_index = (self.match_index + 1) % self.completion_items.len();
        }
    }

    pub(crate) fn completion_page_range(&self, page_size: usize) -> std::ops::Range<usize> {
        if page_size == 0 || self.completion_items.is_empty() {
            return 0..0;
        }
        let selected = self.match_index.min(self.completion_items.len() - 1);
        let start = selected / page_size * page_size;
        start..(start + page_size).min(self.completion_items.len())
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
            cmd_history_line_ids: Vec::new(),
            cmd_history_index: None,
            cmd_history_draft: String::new(),
            pending_history_index: None,
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
    fn completion_navigation_crosses_pages_and_wraps() {
        let mut state = ConsoleState {
            completion_items: (0..7)
                .map(|index| crate::CompletionItem::new(format!("item-{index}"), 0..0))
                .collect(),
            ..ConsoleState::default()
        };

        assert_eq!(state.completion_page_range(3), 0..3);
        state.match_index = 2;
        state.select_next_completion();
        assert_eq!(state.match_index, 3);
        assert_eq!(state.completion_page_range(3), 3..6);

        state.match_index = 6;
        assert_eq!(state.completion_page_range(3), 6..7);
        state.select_next_completion();
        assert_eq!(state.match_index, 0);
        assert_eq!(state.completion_page_range(3), 0..3);

        state.select_previous_completion();
        assert_eq!(state.match_index, 6);
        assert_eq!(state.completion_page_range(3), 6..7);
    }

    #[test]
    fn completion_page_range_is_empty_when_page_size_is_zero() {
        let state = ConsoleState {
            completion_items: vec![crate::CompletionItem::new("item", 0..0)],
            ..ConsoleState::default()
        };

        assert_eq!(state.completion_page_range(0), 0..0);
    }

    #[test]
    fn applies_a_completion_selected_from_a_later_page() {
        let mut state = ConsoleState::default();
        state.replace_input("ma".into());
        state.completion_items = ["map", "marker", "material"]
            .into_iter()
            .map(|label| crate::CompletionItem::new(label, 0..2))
            .collect();
        state.match_index = 2;

        assert_eq!(state.apply_selected_completion(), Some(9));
        assert_eq!(state.input, "material ");
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

    #[test]
    fn recorded_command_keeps_its_echo_line_for_recall_highlighting() {
        let mut state = ConsoleState::default();
        state.record_command("map forest".into(), 10);
        let index = state.take_pending_history_index("map forest").unwrap();
        state.set_history_line_id(index, 42);
        state.cmd_history_index = Some(index);

        assert_eq!(state.selected_history_line_id(), Some(42));
    }
}
