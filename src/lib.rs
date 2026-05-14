mod args;
mod commands;
mod input;
mod registry;
mod state;
mod ui;

#[cfg(feature = "persistent-history")]
mod persistence;

pub mod config;

pub use args::Args;
pub use config::ConsoleConfig;
pub use registry::ConsoleRegistry;
pub use state::ConsoleState;

#[cfg(feature = "embedded-font")]
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
    handle_toggle_key, has_pending_command, scroll_console, sync_console_ui,
};
use ui::{ConsoleAssets, update_console_ui};

// ── Command type ───────────────────────────────────────────────────────────────

/// The input type for console command systems.
///
/// ```no_run
/// # use chill_bevy_console::CommandArgs;
/// # use bevy::prelude::*;
/// fn say_cmd(In(args): CommandArgs) -> String {
///     args.rest(0)
/// }
/// ```
pub type CommandArgs = In<Args>;

// ── App extension ──────────────────────────────────────────────────────────────

pub trait ConsoleAppExt {
    /// Register a Bevy system as a console command.
    ///
    /// The system receives the command arguments as `In<Vec<String>>` and must
    /// return a `String` (the output shown in the console, or empty for no output).
    ///
    /// ```no_run
    /// # use chill_bevy_console::{CommandArgs, ConsoleAppExt};
    /// # use bevy::prelude::*;
    /// fn say_cmd(In(args): CommandArgs) -> String {
    ///     args.join(" ")
    /// }
    /// # let mut app = App::new();
    /// app.add_console_command("say", "say <text> — echo text", say_cmd);
    /// ```
    fn add_console_command<M>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Args>, String, M> + 'static,
    ) -> &mut Self;
}

impl ConsoleAppExt for App {
    fn add_console_command<M>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Args>, String, M> + 'static,
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
/// ```no_run
/// # use bevy::prelude::*;
/// # use chill_bevy_console::console_closed;
/// # fn handle_movement() {}
/// # let mut app = App::new();
/// app.add_systems(Update, handle_movement.run_if(console_closed));
/// ```
pub fn console_closed(state: Option<Res<ConsoleState>>) -> bool {
    state.map_or(true, |s| !s.open)
}

// ── Plugin ─────────────────────────────────────────────────────────────────────

/// The main plugin.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use chill_bevy_console::{ChillConsole, ConsoleConfig};
/// # let mut app = App::new();
/// app.add_plugins(ChillConsole::default());
///
/// // Or with custom config:
/// # let mut app = App::new();
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

        #[cfg(feature = "persistent-history")]
        let initial_state = persistence::load_initial_state(&self.config);
        #[cfg(not(feature = "persistent-history"))]
        let initial_state = ConsoleState::default();

        app.insert_resource(self.config.clone())
            .init_resource::<ConsoleRegistry>()
            .insert_resource(initial_state)
            .init_resource::<ConsoleAssets>()
            .add_plugins(commands::plugin)
            .add_systems(
                Update,
                (
                    handle_toggle_key,
                    sync_console_ui,
                    capture_console_input.run_if(console_open),
                    scroll_console.run_if(console_open),
                    execute_pending_commands.run_if(has_pending_command),
                    update_console_ui.run_if(console_open_and_changed),
                )
                    .chain(),
            );

        #[cfg(feature = "persistent-history")]
        app.add_plugins(persistence::plugin);
    }
}
