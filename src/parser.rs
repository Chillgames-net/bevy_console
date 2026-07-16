//! Shell-like parsing for console input.
//!
//! The parser deliberately has no Bevy dependency, which keeps command
//! execution and completion easy to test. It supports quoted arguments and
//! backslash escaping, and retains source ranges so completion can replace
//! only the word currently being edited.

use std::ops::Range;

/// A quote character surrounding a parsed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    Single,
    Double,
}

/// One token read from console input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedToken {
    /// Decoded text passed to the command.
    pub value: String,
    /// The complete token range in the source, including surrounding quotes.
    pub range: Range<usize>,
    /// The part of `range` containing text that can be replaced by completion.
    pub value_range: Range<usize>,
    /// The surrounding quote style, if the token started quoted.
    pub quote: Option<QuoteStyle>,
}

/// A non-fatal parsing problem. The parsed tokens before the problem remain
/// available so completion can still help finish the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: &'static str,
    pub range: Range<usize>,
}

/// Parsed console input together with the token active at the requested cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInput {
    pub source: String,
    pub cursor: usize,
    pub tokens: Vec<ParsedToken>,
    pub active_token: Option<usize>,
    pub error: Option<ParseError>,
}

impl ParsedInput {
    /// Parses an input line with the cursor at its end.
    pub fn parse(source: impl Into<String>) -> Self {
        let source = source.into();
        Self::parse_at(source.clone(), source.len())
    }

    /// Parses an input line and identifies the token containing `cursor`.
    pub fn parse_at(source: impl Into<String>, cursor: usize) -> Self {
        let source = source.into();
        let cursor = source.floor_char_boundary(cursor.min(source.len()));
        let mut tokens = Vec::new();
        let mut error = None;
        let mut index = 0;

        while index < source.len() {
            index = skip_whitespace(&source, index);
            if index == source.len() {
                break;
            }

            let token_start = index;
            let first = source[index..].chars().next().expect("index is in bounds");
            let (quote, value_start, mut value_end, mut scan) = match first {
                '\'' => (
                    Some(QuoteStyle::Single),
                    index + first.len_utf8(),
                    None,
                    index + first.len_utf8(),
                ),
                '"' => (
                    Some(QuoteStyle::Double),
                    index + first.len_utf8(),
                    None,
                    index + first.len_utf8(),
                ),
                _ => (None, index, None, index),
            };

            let mut value = String::new();
            let mut escaped = false;
            let mut closed = quote.is_none();
            while scan < source.len() {
                let ch = source[scan..].chars().next().expect("scan is in bounds");
                let next = scan + ch.len_utf8();

                if escaped {
                    value.push(ch);
                    escaped = false;
                    scan = next;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    scan = next;
                    continue;
                }
                if let Some(style) = quote {
                    let closes = matches!(
                        (style, ch),
                        (QuoteStyle::Single, '\'') | (QuoteStyle::Double, '"')
                    );
                    if closes {
                        value_end = Some(scan);
                        scan = next;
                        closed = true;
                        break;
                    }
                } else if ch.is_whitespace() {
                    value_end = Some(scan);
                    break;
                }
                value.push(ch);
                scan = next;
            }

            if escaped {
                error.get_or_insert(ParseError {
                    message: "input ends with an escape character",
                    range: scan.saturating_sub(1)..scan,
                });
            }
            if quote.is_some() && !closed {
                error.get_or_insert(ParseError {
                    message: "unterminated quoted argument",
                    range: token_start..source.len(),
                });
            }

            let token_end = scan;
            let value_end = value_end.unwrap_or(token_end);
            tokens.push(ParsedToken {
                value,
                range: token_start..token_end,
                value_range: value_start..value_end,
                quote,
            });
            index = token_end;
        }

        let active_token = tokens
            .iter()
            .position(|token| cursor >= token.value_range.start && cursor <= token.value_range.end);
        Self {
            source,
            cursor,
            tokens,
            active_token,
            error,
        }
    }

    /// The command word, if present.
    pub fn command(&self) -> Option<&str> {
        self.tokens.first().map(|token| token.value.as_str())
    }

    /// The zero-based argument position being edited. `None` means the command
    /// name itself is active; `Some(0)` is the first command argument.
    pub fn active_argument_index(&self) -> Option<usize> {
        match self.active_token {
            Some(0) => None,
            Some(index) => Some(index - 1),
            None => self
                .tokens
                .iter()
                .take_while(|token| token.range.end <= self.cursor)
                .count()
                .checked_sub(1),
        }
    }

    /// The raw fragment at the cursor, decoded as an argument value.
    pub fn active_fragment(&self) -> &str {
        self.active_token
            .and_then(|index| self.tokens.get(index))
            .map(|token| token.value.as_str())
            .unwrap_or("")
    }

    /// The source range completion should replace. A zero-width range at the
    /// cursor is returned when the user is beginning a new argument.
    pub fn replacement_range(&self) -> Range<usize> {
        self.active_token
            .and_then(|index| self.tokens.get(index))
            .map(|token| token.value_range.clone())
            .unwrap_or(self.cursor..self.cursor)
    }
}

fn skip_whitespace(source: &str, mut index: usize) -> usize {
    while index < source.len() {
        let ch = source[index..].chars().next().expect("index is in bounds");
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_words() {
        let parsed = ParsedInput::parse("teleport 12 -4");
        assert_eq!(parsed.command(), Some("teleport"));
        assert_eq!(
            parsed.tokens.iter().map(|t| &t.value).collect::<Vec<_>>(),
            ["teleport", "12", "-4"]
        );
        assert_eq!(parsed.active_argument_index(), Some(1));
    }

    #[test]
    fn parses_quotes_and_escapes() {
        let parsed = ParsedInput::parse(r#"say "hello world" two\ words"#);
        assert_eq!(
            parsed.tokens.iter().map(|t| &t.value).collect::<Vec<_>>(),
            ["say", "hello world", "two words"]
        );
        assert_eq!(parsed.tokens[1].quote, Some(QuoteStyle::Double));
    }

    #[test]
    fn preserves_a_replacement_range() {
        let parsed = ParsedInput::parse_at("map de", 6);
        assert_eq!(parsed.active_argument_index(), Some(0));
        assert_eq!(parsed.active_fragment(), "de");
        assert_eq!(parsed.replacement_range(), 4..6);
    }

    #[test]
    fn uses_the_cursor_position_between_tokens_for_argument_completion() {
        let source = "set  render.max_fps 60";
        let cursor = source.find("render").unwrap() - 1;
        let parsed = ParsedInput::parse_at(source, cursor);

        assert_eq!(parsed.active_argument_index(), Some(0));
        assert_eq!(parsed.replacement_range(), cursor..cursor);
    }

    #[test]
    fn reports_unterminated_quote_without_discarding_token() {
        let parsed = ParsedInput::parse("say 'hello");
        assert_eq!(parsed.tokens[1].value, "hello");
        assert_eq!(
            parsed.error.unwrap().message,
            "unterminated quoted argument"
        );
    }
}
