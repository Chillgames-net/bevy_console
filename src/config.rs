use bevy::prelude::*;

/// All visual and behavioral settings for the developer console.
///
/// Use `ConsoleConfig::default()` for the built-in dark/gold look, or set any
/// field to customise every visual element before passing it to the plugin:
///
/// ```rust,ignore
/// app.add_plugins(ChillgamesConsolePlugin {
///     config: ConsoleConfig {
///         input_border_color: Color::srgb(0.2, 0.8, 0.4),
///         toggle_key: KeyCode::F1,
///         ..default()
///     },
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
    /// Prefix symbol shown before the cursor (e.g. `"▶ "`).
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
    /// Text color for unselected dropdown items.
    pub dropdown_text_color: Color,
    /// Background color for the currently highlighted dropdown item.
    pub dropdown_highlight_bg: Color,
    /// Text color for the currently highlighted dropdown item.
    pub dropdown_highlight_text_color: Color,

    // ── Behavior ──────────────────────────────────────────────────────────────
    /// The key that toggles the console open and closed. Defaults to backtick.
    pub toggle_key: KeyCode,
}

impl Default for ConsoleConfig {
    fn default() -> Self {
        Self {
            font_path: None,
            font_size: 18.0,
            history_font_size: 16.0,
            dropdown_font_size: 16.0,

            history_height_vh: 38.0,
            history_bg: Color::srgba(0.07, 0.07, 0.07, 0.96),
            history_padding: 8.0,
            history_text_color: Color::srgb(0.85, 0.85, 0.85),

            input_bg: Color::srgba(0.0, 0.0, 0.0, 0.98),
            input_padding_h: 10.0,
            input_padding_v: 7.0,
            input_border_width: 2.0,
            input_border_color: Color::srgb(0.35, 0.75, 1.0),
            input_text_color: Color::WHITE,
            input_ghost_color: Color::srgba(1.0, 1.0, 1.0, 0.30),
            input_prefix: "\u{25b6} ".to_string(),

            dropdown_bg: Color::srgba(0.05, 0.08, 0.12, 0.97),
            dropdown_border_color: Color::srgb(0.15, 0.40, 0.60),
            dropdown_item_divider_color: Color::srgba(0.35, 0.75, 1.0, 0.08),
            dropdown_padding_h: 10.0,
            dropdown_padding_v: 5.0,
            dropdown_text_color: Color::srgb(0.60, 0.70, 0.75),
            dropdown_highlight_bg: Color::srgba(0.35, 0.75, 1.0, 0.12),
            dropdown_highlight_text_color: Color::srgb(0.75, 0.92, 1.0),

            toggle_key: KeyCode::Backquote,
        }
    }
}
