mod args;
mod commands;
mod input;
mod registry;
mod state;
mod ui;

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

#[cfg(feature = "persistent-history")]
use std::path::Path;

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

        // Pre-populate ConsoleState from the persisted history file (if any),
        // so that Up/Down recall works on the very first frame and the user can
        // see what they ran in the previous session.
        #[allow(unused_mut)]
        let mut initial_state = ConsoleState::default();
        #[cfg(feature = "persistent-history")]
        if let Some(path) = &self.config.history_file {
            initial_state.cmd_history = load_cmd_history(path, self.config.history_max_entries);
            if !initial_state.cmd_history.is_empty() {
                initial_state
                    .history
                    .push("── previous session ──".to_string());
                for cmd in &initial_state.cmd_history {
                    initial_state.history.push(format!("> {cmd}"));
                }
            }
        }

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
                    persist_cmd_history,
                    update_console_ui.run_if(console_open_and_changed),
                )
                    .chain(),
            );
    }
}

// ── Command-history persistence ───────────────────────────────────────────────

#[cfg(feature = "persistent-history")]
fn load_cmd_history(path: &Path, max_entries: usize) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(
                "chill_bevy_console: history file {:?} not found yet (will be created on first command)",
                path
            );
            return Vec::new();
        }
        Err(e) => {
            warn!(
                "chill_bevy_console: failed to read history file {:?}: {}",
                path, e
            );
            return Vec::new();
        }
    };
    let mut lines: Vec<String> = content
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let max = max_entries.max(1);
    if lines.len() > max {
        let excess = lines.len() - max;
        lines.drain(0..excess);
    }
    info!(
        "chill_bevy_console: loaded {} history entries from {:?}",
        lines.len(),
        path
    );
    lines
}

#[cfg(feature = "persistent-history")]
fn persist_cmd_history(config: Res<ConsoleConfig>, mut state: ResMut<ConsoleState>) {
    if !state.cmd_history_dirty {
        return;
    }
    state.cmd_history_dirty = false;

    let Some(path) = &config.history_file else {
        return;
    };

    let mut content = state.cmd_history.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    match std::fs::write(path, &content) {
        Ok(()) => debug!(
            "chill_bevy_console: wrote {} history entries to {:?}",
            state.cmd_history.len(),
            path
        ),
        Err(e) => warn!(
            "chill_bevy_console: failed to write history file {:?}: {}",
            path, e
        ),
    }
}

/// No-op stub when the `persistent-history` feature is disabled.
#[cfg(not(feature = "persistent-history"))]
fn persist_cmd_history() {}
