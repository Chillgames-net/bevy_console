/// Parsed arguments passed to a console command system.
///
/// Dereferences to `[String]`, so slice methods (`len`, `is_empty`, `iter`, `join`, …)
/// work directly. Additional helpers cover the most common patterns:
///
/// ```no_run
/// # use chill_bevy_console::{Args, CommandArgs};
/// # use bevy::prelude::*;
/// fn greet_cmd(In(args): CommandArgs) -> String {
///     let name = args.get(0).unwrap_or("world");
///     let count: usize = args.parse(1).unwrap_or(1);
///     let tail = args.rest(2); // everything after the second arg
///     format!("Hello, {name}! (×{count}) {tail}")
/// }
/// ```
pub struct Args(pub(crate) Vec<String>);

impl Args {
    /// Returns the argument at `index` as a `&str`, or `None` if out of bounds.
    ///
    /// ```
    /// # use chill_bevy_console::Args;
    /// let args = Args::from(vec!["hello".to_string(), "world".to_string()]);
    /// assert_eq!(args.get(0), Some("hello"));
    /// assert_eq!(args.get(5), None);
    /// ```
    pub fn get(&self, index: usize) -> Option<&str> {
        self.0.get(index).map(String::as_str)
    }

    /// Parses the argument at `index` as `T`.
    ///
    /// Returns `None` if the index is out of bounds or the value fails to parse.
    ///
    /// ```
    /// # use chill_bevy_console::Args;
    /// let args = Args::from(vec!["42".to_string(), "bad".to_string()]);
    /// assert_eq!(args.parse::<i32>(0), Some(42));
    /// assert_eq!(args.parse::<i32>(1), None); // parse failed
    /// assert_eq!(args.parse::<i32>(2), None); // out of bounds
    /// ```
    pub fn parse<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.0.get(index)?.parse().ok()
    }

    /// Joins all arguments from `start` onwards with spaces.
    ///
    /// Returns an empty string when `start` is past the end.
    ///
    /// ```
    /// # use chill_bevy_console::Args;
    /// let args = Args::from(vec!["cmd".to_string(), "foo".to_string(), "bar".to_string()]);
    /// assert_eq!(args.rest(1), "foo bar");
    /// assert_eq!(args.rest(99), "");
    /// ```
    pub fn rest(&self, start: usize) -> String {
        self.0[start.min(self.0.len())..].join(" ")
    }
}

impl From<Vec<String>> for Args {
    fn from(v: Vec<String>) -> Self {
        Args(v)
    }
}

impl std::ops::Deref for Args {
    type Target = [String];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Args;

    fn args(s: &str) -> Args {
        Args(s.split_whitespace().map(str::to_string).collect())
    }

    // ── get ──────────────────────────────────────────────────────────────────

    #[test]
    fn get_returns_str_at_index() {
        let a = args("foo bar baz");
        assert_eq!(a.get(0), Some("foo"));
        assert_eq!(a.get(2), Some("baz"));
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let a = args("foo");
        assert_eq!(a.get(1), None);
    }

    #[test]
    fn get_on_empty_args_returns_none() {
        let a = Args(vec![]);
        assert_eq!(a.get(0), None);
    }

    // ── parse ─────────────────────────────────────────────────────────────────

    #[test]
    fn parse_valid_integer() {
        let a = args("42");
        assert_eq!(a.parse::<i32>(0), Some(42));
    }

    #[test]
    fn parse_valid_float() {
        let a = args("3.14");
        assert!(matches!(a.parse::<f32>(0), Some(v) if (v - 3.14_f32).abs() < 1e-5));
    }

    #[test]
    fn parse_invalid_returns_none() {
        let a = args("notanumber");
        assert_eq!(a.parse::<i32>(0), None);
    }

    #[test]
    fn parse_out_of_bounds_returns_none() {
        let a = args("42");
        assert_eq!(a.parse::<i32>(1), None);
    }

    // ── rest ──────────────────────────────────────────────────────────────────

    #[test]
    fn rest_from_zero_joins_all() {
        let a = args("hello world");
        assert_eq!(a.rest(0), "hello world");
    }

    #[test]
    fn rest_skips_leading_args() {
        let a = args("cmd foo bar baz");
        assert_eq!(a.rest(1), "foo bar baz");
    }

    #[test]
    fn rest_past_end_returns_empty() {
        let a = args("foo");
        assert_eq!(a.rest(5), "");
    }

    #[test]
    fn rest_on_empty_args_returns_empty() {
        let a = Args(vec![]);
        assert_eq!(a.rest(0), "");
    }

    // ── deref / slice methods ─────────────────────────────────────────────────

    #[test]
    fn len_and_is_empty() {
        let empty = Args(vec![]);
        let one = args("x");
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(!one.is_empty());
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn iter_via_deref() {
        let a = args("a b c");
        let collected: Vec<&str> = a.iter().map(String::as_str).collect();
        assert_eq!(collected, ["a", "b", "c"]);
    }

    #[test]
    fn join_via_deref() {
        let a = args("hello world");
        assert_eq!(a.join("-"), "hello-world");
    }
}
