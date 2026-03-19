mod commands;
mod input;
mod registry;
mod state;
mod ui;

pub mod config;

pub use config::ConsoleConfig;
pub use registry::ConsoleRegistry;
pub use state::ConsoleState;

use bevy::asset::uuid_handle;
use bevy::prelude::*;

// ── Embedded font ──────────────────────────────────────────────────────────────

/// Stable handle for the embedded UbuntuMono font.
/// Only meaningful when the `embedded-font` feature is enabled.
#[cfg(feature = "embedded-font")]
pub const UBUNTU_MONO_FONT_HANDLE: Handle<Font> =
    uuid_handle!("7fca4e91-3b58-d20a-9c63-e0174f2b85d6");

use input::{
    capture_console_input, console_open, console_open_and_changed, execute_pending_commands,
    has_pending_command, toggle_console,
};
use ui::{ConsoleAssets, update_console_ui};

// ── Command type ───────────────────────────────────────────────────────────────

/// The input type for console command systems: a list of whitespace-split arguments.
///
/// ```rust,ignore
/// fn say_cmd(In(args): CommandArgs) -> String {
///     args.join(" ")
/// }
/// ```
pub type CommandArgs = In<Vec<String>>;

// ── App extension ──────────────────────────────────────────────────────────────

pub trait ConsoleAppExt {
    /// Register a Bevy system as a console command.
    ///
    /// The system receives the command arguments as `In<Vec<String>>` and must
    /// return a `String` (the output shown in the console, or empty for no output).
    ///
    /// ```rust,ignore
    /// fn say_cmd(In(args): CommandArgs) -> String {
    ///     args.join(" ")
    /// }
    ///
    /// app.add_console_command("say", "say <text> — echo text", say_cmd);
    /// ```
    fn add_console_command<M>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Vec<String>>, String, M> + 'static,
    ) -> &mut Self;
}

impl ConsoleAppExt for App {
    fn add_console_command<M>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Vec<String>>, String, M> + 'static,
    ) -> &mut Self {
        self.init_resource::<ConsoleRegistry>();
        let system_id = self.world_mut().register_system(system);
        self.world_mut()
            .resource_mut::<ConsoleRegistry>()
            .register(name, usage, system_id);
        self
    }
}

// ── Run condition ──────────────────────────────────────────────────────────────

/// Returns `true` when the console is **closed**.
///
/// Use this as a run condition to suppress gameplay input while the console is open:
///
/// ```rust,ignore
/// app.add_systems(Update, handle_movement.run_if(console_closed));
/// ```
pub fn console_closed(state: Option<Res<ConsoleState>>) -> bool {
    state.map_or(true, |s| !s.open)
}

// ── Plugin ─────────────────────────────────────────────────────────────────────

/// The main plugin.
///
/// ```rust,ignore
/// app.add_plugins(ChillConsole::default());
///
/// // Or with custom config:
/// app.add_plugins(ChillConsole {
///     config: ConsoleConfig {
///         input_border_color: Color::srgb(0.2, 0.8, 0.4),
///         toggle_key: KeyCode::F1,
///         ..default()
///     },
/// });
/// ```
pub struct ChillConsole {
    pub config: ConsoleConfig,
}

impl Default for ChillConsole {
    fn default() -> Self {
        Self {
            config: ConsoleConfig::default(),
        }
    }
}

impl Plugin for ChillConsole {
    fn build(&self, app: &mut App) {
        // Embed font bytes into Assets<Font> before ConsoleAssets is initialized,
        // so that FromWorld can resolve UBUNTU_MONO_FONT_HANDLE immediately.
        #[cfg(feature = "embedded-font")]
        {
            let bytes = include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/UbuntuMono-R.ttf"
            ));
            let font = Font::try_from_bytes(bytes.to_vec()).expect("embedded font is valid");
            let _ = app
                .world_mut()
                .resource_mut::<Assets<Font>>()
                .insert(&UBUNTU_MONO_FONT_HANDLE, font);
        }

        app.insert_resource(self.config.clone())
            .init_resource::<ConsoleRegistry>()
            .init_resource::<ConsoleState>()
            .init_resource::<ConsoleAssets>()
            .add_plugins(commands::plugin)
            .add_systems(
                Update,
                (
                    toggle_console,
                    capture_console_input.run_if(console_open),
                    execute_pending_commands.run_if(has_pending_command),
                    update_console_ui.run_if(console_open_and_changed),
                )
                    .chain(),
            );
    }
}
