//! Shared helpers for Bevy's editable console text widget.

use bevy::text::{EditableText, TextEdit};

/// Replaces editor text and moves the cursor to a valid UTF-8 boundary.
pub(crate) fn set_editable_text(input: &mut EditableText, value: &str, cursor: usize) {
    input.clear();
    input.editor_mut().set_text(value);
    let mut cursor = cursor.min(value.len());
    while !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    if cursor == value.len() {
        input.queue_edit(TextEdit::TextEnd(false));
        return;
    }
    input.queue_edit(TextEdit::TextStart(false));
    for _ in value[..cursor].chars() {
        input.queue_edit(TextEdit::Right(false));
    }
}
