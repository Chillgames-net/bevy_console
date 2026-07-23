use bevy::prelude::*;
use std::collections::BTreeSet;

/// A command provided by the console plugin rather than the host application.
///
/// [`crate::ChillConsole::builtin_commands`] controls which of these commands
/// are registered. By default, only [`Self::Help`] and [`Self::Clear`] are
/// enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltinCommand {
    Clear,
    Help,
    Alias,
    Bind,
    State,
    #[cfg(feature = "resource-properties")]
    Res,
}

/// The set of built-in console commands to register.
///
/// [`Self::default`] enables `help` and `clear`. Use
/// [`BuiltinCommand::all`] to opt in to every built-in command.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct BuiltinCommands(BTreeSet<BuiltinCommand>);

impl Default for BuiltinCommands {
    fn default() -> Self {
        Self::from([BuiltinCommand::Help, BuiltinCommand::Clear])
    }
}

impl BuiltinCommands {
    pub fn all() -> Self {
        Self::from(BuiltinCommand::all())
    }
}

impl<T: IntoIterator<Item = BuiltinCommand>> From<T> for BuiltinCommands {
    fn from(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl FromIterator<BuiltinCommand> for BuiltinCommands {
    fn from_iter<T: IntoIterator<Item = BuiltinCommand>>(iter: T) -> Self {
        Self::from(iter)
    }
}

impl std::ops::Deref for BuiltinCommands {
    type Target = BTreeSet<BuiltinCommand>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BuiltinCommand {
    /// Returns every built-in command.
    #[cfg(feature = "resource-properties")]
    pub const fn all() -> [Self; 6] {
        [
            Self::Clear,
            Self::Help,
            Self::Alias,
            Self::Bind,
            Self::State,
            Self::Res,
        ]
    }

    /// Returns every built-in command.
    #[cfg(not(feature = "resource-properties"))]
    pub const fn all() -> [Self; 5] {
        [
            Self::Clear,
            Self::Help,
            Self::Alias,
            Self::Bind,
            Self::State,
        ]
    }
}

/// Visual and interaction settings for the developer console.
///
/// Use `ConsoleConfig::default()` for the built-in dark/gold look, or set any
/// field to customise every visual element before passing it to the plugin:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use chill_bevy_console::{ChillConsole, ConsoleConfig};
/// # let mut app = App::new();
/// app.add_plugins(ChillConsole {
///     config: ConsoleConfig {
///         input_border_color: Color::srgb(0.2, 0.8, 0.4),
///         toggle_key: KeyCode::F1,
///         ..default()
///     },
///     ..default()
/// });
/// ```
#[derive(Resource, Clone)]
pub struct ConsoleConfig {
    // ── Font ─────────────────────────────────────────────────────────────────
    /// Path to a font asset (e.g. `"fonts/UbuntuMono-R.ttf"`).
    /// `None` uses Bevy's built-in default font.
    pub font_path: Option<String>,
    /// Font size for the input bar text.
    pub font_size: f32,
    /// Font size for history lines.
    pub history_font_size: f32,
    /// Font size for dropdown suggestion items.
    pub dropdown_font_size: f32,

    // ── History panel ─────────────────────────────────────────────────────────
    /// Height of the history panel as a percentage of viewport height.
    pub history_height_vh: f32,
    /// Background color of the history panel.
    pub history_bg: Color,
    /// Padding (px) inside the history panel on all sides.
    pub history_padding: f32,
    /// Text color for history lines.
    pub history_text_color: Color,
    /// Text color for trace/debug output lines.
    pub history_debug_color: Color,
    /// Text color for warning output lines.
    pub history_warn_color: Color,
    /// Text color for error output lines.
    pub history_error_color: Color,
    /// Background color for the output row selected through Up/Down recall.
    pub history_highlight_bg: Color,

    // ── Input bar ─────────────────────────────────────────────────────────────
    /// Background color of the input bar.
    pub input_bg: Color,
    /// Horizontal padding (px) inside the input bar.
    pub input_padding_h: f32,
    /// Vertical padding (px) inside the input bar.
    pub input_padding_v: f32,
    /// Width (px) of the divider border drawn above the input bar.
    pub input_border_width: f32,
    /// Color of the divider line above the input bar.
    pub input_border_color: Color,
    /// Color of the main input text.
    pub input_text_color: Color,
    /// Color of the ghost / autocomplete hint suffix.
    pub input_ghost_color: Color,
    /// Prefix shown before the cursor (e.g. `"> "`).
    pub input_prefix: String,

    // ── Dropdown ──────────────────────────────────────────────────────────────
    /// Background color of the autocomplete dropdown.
    pub dropdown_bg: Color,
    /// Color of the bottom border of the dropdown container.
    pub dropdown_border_color: Color,
    /// Color of the hairline dividers between dropdown items.
    pub dropdown_item_divider_color: Color,
    /// Horizontal padding (px) inside each dropdown item.
    pub dropdown_padding_h: f32,
    /// Vertical padding (px) inside each dropdown item.
    pub dropdown_padding_v: f32,
    /// Maximum number of wrapped lines in each dropdown item. Set to `0` for
    /// no limit. Defaults to `2`.
    pub dropdown_item_max_lines: usize,
    /// Text color for unselected dropdown items.
    pub dropdown_text_color: Color,
    /// Background color for the currently highlighted dropdown item.
    pub dropdown_highlight_bg: Color,
    /// Text color for the currently highlighted dropdown item.
    pub dropdown_highlight_text_color: Color,

    // ── Behavior ──────────────────────────────────────────────────────────────
    /// The key that toggles the console open and closed. Defaults to backtick.
    pub toggle_key: KeyCode,
    /// Close the console when Enter is submitted with no input. Defaults to
    /// `false`.
    pub close_on_empty_submit: bool,
    /// Maximum structured output lines kept in the in-game console buffer.
    pub max_history_lines: usize,
    /// Maximum submitted commands kept for Up/Down recall.
    pub max_command_history: usize,
    /// Maximum completion rows presented on each suggestion page.
    pub max_suggestions: usize,
    /// Z-index applied to the console overlay.
    pub z_index: i32,
}

impl ConsoleConfig {
    /// Icy blue — the Chillgames look.
    pub fn chillgames() -> Self {
        Self {
            history_bg: Color::srgba(0.07, 0.07, 0.07, 0.96),
            history_text_color: Color::srgb(0.85, 0.85, 0.85),
            input_bg: Color::srgba(0.0, 0.0, 0.0, 0.98),
            input_border_color: Color::srgb(0.35, 0.75, 1.0),
            input_text_color: Color::WHITE,
            input_ghost_color: Color::srgba(1.0, 1.0, 1.0, 0.30),
            dropdown_bg: Color::srgba(0.05, 0.08, 0.12, 0.97),
            dropdown_border_color: Color::srgb(0.15, 0.40, 0.60),
            dropdown_item_divider_color: Color::srgba(0.35, 0.75, 1.0, 0.08),
            dropdown_text_color: Color::srgb(0.60, 0.70, 0.75),
            dropdown_highlight_bg: Color::srgba(0.35, 0.75, 1.0, 0.12),
            dropdown_highlight_text_color: Color::srgb(0.75, 0.92, 1.0),
            ..Self::default()
        }
    }

    /// Black background with green phosphor text.
    pub fn matrix() -> Self {
        Self {
            history_bg: Color::srgba(0.0, 0.04, 0.0, 0.97),
            history_text_color: Color::srgb(0.15, 0.85, 0.25),
            input_bg: Color::srgba(0.0, 0.02, 0.0, 0.99),
            input_border_color: Color::srgb(0.0, 0.90, 0.20),
            input_text_color: Color::srgb(0.20, 1.0, 0.30),
            input_ghost_color: Color::srgba(0.0, 0.90, 0.20, 0.30),
            dropdown_bg: Color::srgba(0.0, 0.06, 0.0, 0.97),
            dropdown_border_color: Color::srgb(0.0, 0.45, 0.10),
            dropdown_item_divider_color: Color::srgba(0.0, 0.90, 0.20, 0.08),
            dropdown_text_color: Color::srgb(0.10, 0.60, 0.18),
            dropdown_highlight_bg: Color::srgba(0.0, 0.90, 0.20, 0.12),
            dropdown_highlight_text_color: Color::srgb(0.30, 1.0, 0.45),
            ..Self::default()
        }
    }

    /// Muted blue-gray inspired by the Source engine developer console.
    pub fn source() -> Self {
        Self {
            history_bg: Color::srgba(0.10, 0.11, 0.13, 0.96),
            history_text_color: Color::srgb(0.82, 0.83, 0.85),
            input_bg: Color::srgba(0.07, 0.08, 0.10, 0.98),
            input_border_color: Color::srgb(0.40, 0.43, 0.50),
            input_text_color: Color::srgb(0.90, 0.91, 0.93),
            input_ghost_color: Color::srgba(0.90, 0.91, 0.93, 0.25),
            dropdown_bg: Color::srgba(0.12, 0.13, 0.16, 0.97),
            dropdown_border_color: Color::srgb(0.25, 0.27, 0.32),
            dropdown_item_divider_color: Color::srgba(1.0, 1.0, 1.0, 0.05),
            dropdown_text_color: Color::srgb(0.55, 0.58, 0.65),
            dropdown_highlight_bg: Color::srgba(0.40, 0.43, 0.50, 0.15),
            dropdown_highlight_text_color: Color::srgb(0.90, 0.91, 0.93),
            ..Self::default()
        }
    }
}

impl Default for ConsoleConfig {
    fn default() -> Self {
        Self {
            font_path: None,
            font_size: 18.0,
            history_font_size: 16.0,
            dropdown_font_size: 16.0,

            history_height_vh: 38.0,
            history_bg: Color::srgba(0.05, 0.05, 0.05, 0.96),
            history_padding: 8.0,
            history_text_color: Color::srgb(0.90, 0.90, 0.90),
            history_debug_color: Color::srgb(0.55, 0.55, 0.55),
            history_warn_color: Color::srgb(1.0, 0.78, 0.25),
            history_error_color: Color::srgb(1.0, 0.35, 0.35),
            history_highlight_bg: Color::srgba(1.0, 1.0, 1.0, 0.10),

            input_bg: Color::srgba(0.0, 0.0, 0.0, 0.98),
            input_padding_h: 10.0,
            input_padding_v: 7.0,
            input_border_width: 2.0,
            input_border_color: Color::WHITE,
            input_text_color: Color::WHITE,
            input_ghost_color: Color::srgba(1.0, 1.0, 1.0, 0.25),
            input_prefix: "> ".to_string(),

            dropdown_bg: Color::srgba(0.08, 0.08, 0.08, 0.97),
            dropdown_border_color: Color::srgb(0.35, 0.35, 0.35),
            dropdown_item_divider_color: Color::srgba(1.0, 1.0, 1.0, 0.06),
            dropdown_padding_h: 10.0,
            dropdown_padding_v: 5.0,
            dropdown_item_max_lines: 2,
            dropdown_text_color: Color::srgb(0.60, 0.60, 0.60),
            dropdown_highlight_bg: Color::srgba(1.0, 1.0, 1.0, 0.10),
            dropdown_highlight_text_color: Color::WHITE,

            toggle_key: KeyCode::Backquote,
            close_on_empty_submit: false,
            max_history_lines: 256,
            max_command_history: 500,
            max_suggestions: 5,
            z_index: i32::MAX,
        }
    }
}
