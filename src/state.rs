use crate::registry::ConsoleRegistry;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct ConsoleState {
    pub open: bool,
    pub input: String,
    pub history: Vec<String>,
    /// Command names that contain the current first typed word.
    pub matches: Vec<String>,
    /// Index into `matches` that is currently highlighted.
    pub match_index: usize,
    pub pending_command: Option<String>,
}

impl ConsoleState {
    pub fn recompute_matches(&mut self, registry: &ConsoleRegistry) {
        let first_word = self.input.split_whitespace().next().unwrap_or("");
        if first_word.is_empty() || self.input.contains(' ') {
            self.matches.clear();
            return;
        }
        self.matches = registry
            .commands
            .keys()
            .filter(|k| k.contains(first_word))
            .cloned()
            .collect();
        // BTreeMap is sorted, so matches come out alphabetically already.
        if self.match_index >= self.matches.len() {
            self.match_index = 0;
        }
    }

    pub fn push_line(&mut self, line: String) {
        self.history.push(line);
        if self.history.len() > 200 {
            self.history.remove(0);
        }
    }
}
