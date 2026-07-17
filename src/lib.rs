//! A configurable developer console plugin for [Bevy](https://bevyengine.org) games.
//!
//! Press `` ` `` (backtick) to toggle the console open and closed. Commands are
//! plain Bevy systems that take [`CommandArgs`] and return a [`String`] or
//! [`ConsoleResult`].
//!
//! # Quickstart
//!
//! ```no_run
//! use bevy::prelude::*;
//! use chill_bevy_console::{ChillConsole, CommandArgs, ConsoleAppExt, console_closed};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(ChillConsole::default())
//!         .add_console_command("say", "say <text> - echo text", say_cmd)
//!         .add_systems(Update, gameplay_input.run_if(console_closed))
//!         .run();
//! }
//!
//! fn say_cmd(In(args): CommandArgs) -> String {
//!     args.join(" ")
//! }
//!
//! fn gameplay_input() { /* movement, jump, etc. */ }
//! ```
//!
//! # Cargo features
//!
//! - `embedded-font` — embed `UbuntuMono-R.ttf` in the binary and use it as the
//!   default font, so consumers don't need to ship a font asset.
//! - `persistent-history` — load and save entered commands to a plain-text file
//!   between runs. The path
//!   is configured via [`ConsoleConfig::history_file`].
//! - `resource-properties` — expose selected fields on Bevy resources through
//!   the console. See [`ConsoleResource`] and [`ConsoleAppExt::add_console_resource`].
//!
//! # Customization
//!
//! Every visual element is configurable via [`ConsoleConfig`], with built-in
//! presets ([`ConsoleConfig::chillgames`], [`ConsoleConfig::matrix`],
//! [`ConsoleConfig::source`]) as starting points. See the
//! [`USAGE.md`](https://github.com/Chillgames-net/bevy_console/blob/main/USAGE.md)
//! guide and the [`examples/`](https://github.com/Chillgames-net/bevy_console/tree/main/examples)
//! directory for runnable demos.

mod args;
mod commands;
mod completion;
mod input;
mod logging;
mod model;
mod parser;
mod registry;
mod state;
mod ui;

#[cfg(feature = "resource-properties")]
mod resource_properties;

#[cfg(feature = "persistent-history")]
mod persistence;

pub mod config;

pub use args::Args;
pub use config::{BuiltinCommand, BuiltinCommands, ConsoleConfig};
pub use logging::{ConsoleLogCapture, console_log_layer};
pub use model::{
    ArgumentKind, ArgumentSpec, CommandOrigin, CommandSpec, CompletionItem, CompletionRequest,
    ConsoleAliases, ConsoleBinds, ConsoleBuffer, ConsoleCommandExecuted, ConsoleCommandQueue,
    ConsoleKeyBinding, ConsoleKeyModifiers, ConsoleLevel, ConsoleLine, ConsoleLineMessage,
    ConsoleLineSource, ConsoleRequest, ConsoleResult,
};
pub use parser::{ParseError, ParsedInput, ParsedToken, QuoteStyle};
pub use registry::{CommandDef, CommandExecutor, ConsoleRegistry};
pub use state::ConsoleState;

#[cfg(feature = "resource-properties")]
pub use chill_bevy_console_derive::ConsoleResource;
#[cfg(feature = "resource-properties")]
pub use resource_properties::{
    ConsoleProperty, ConsolePropertyValue, ConsoleResource, ConsoleResources,
};

// Allows the re-exported derive to refer to this crate when it is used by the
// crate's own tests and doctests.
#[cfg(feature = "resource-properties")]
extern crate self as chill_bevy_console;

#[cfg(feature = "embedded-font")]
use bevy::asset::uuid_handle;
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::prelude::*;
use bevy::ui_widgets::EditableTextInputPlugin;

// ── Embedded font ──────────────────────────────────────────────────────────────

/// Stable handle for the embedded UbuntuMono font.
/// Only meaningful when the `embedded-font` feature is enabled.
#[cfg(feature = "embedded-font")]
pub const UBUNTU_MONO_FONT_HANDLE: Handle<Font> =
    uuid_handle!("7fca4e91-3b58-d20a-9c63-e0174f2b85d6");

use completion::{has_dirty_completion, refresh_completions};
use input::{
    capture_console_input, collect_console_lines, console_open, console_open_and_changed,
    execute_pending_commands, handle_toggle_key, has_pending_command, queue_bound_commands,
    queue_console_requests, scroll_console, sync_console_input, sync_console_ui,
};
use logging::drain_captured_logs;
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
    /// The system receives the command arguments as `In<Args>` and must
    /// return a type that converts into [`ConsoleResult`]. Most commands return
    /// a `String` (the output shown in the console, or empty for no output).
    /// Return `ConsoleResult` to emit lines with individual severity levels.
    ///
    /// ```no_run
    /// # use chill_bevy_console::{CommandArgs, ConsoleAppExt};
    /// # use bevy::prelude::*;
    /// fn say_cmd(In(args): CommandArgs) -> String {
    ///     args.join(" ")
    /// }
    /// # let mut app = App::new();
    /// app.add_console_command("say", "say <text> - echo text", say_cmd);
    /// ```
    fn add_console_command<M, O>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Args>, O, M> + 'static,
    ) -> &mut Self
    where
        O: Into<ConsoleResult> + 'static;

    /// Register a command with aliases, argument choices, dynamic completion,
    /// and structured help. Command systems may return either `String` or
    /// [`ConsoleResult`].
    fn add_console_command_spec<M, O>(
        &mut self,
        spec: CommandSpec,
        system: impl IntoSystem<In<Args>, O, M> + 'static,
    ) -> &mut Self
    where
        O: Into<ConsoleResult> + 'static;

    /// Add a dynamic completer for one argument of a registered command. The
    /// completer is a normal Bevy system and may query resources or entities.
    fn add_console_completer<M>(
        &mut self,
        command: &str,
        argument_index: usize,
        completer: impl IntoSystem<In<CompletionRequest>, Vec<CompletionItem>, M> + 'static,
    ) -> &mut Self;

    /// Register the opt-in console properties generated for a Bevy resource.
    #[cfg(feature = "resource-properties")]
    fn add_console_resource<R: ConsoleResource>(&mut self) -> &mut Self;
}

impl ConsoleAppExt for App {
    fn add_console_command<M, O>(
        &mut self,
        name: &'static str,
        usage: &'static str,
        system: impl IntoSystem<In<Args>, O, M> + 'static,
    ) -> &mut Self
    where
        O: Into<ConsoleResult> + 'static,
    {
        self.add_console_command_spec(CommandSpec::new(name).help(usage), system)
    }

    fn add_console_command_spec<M, O>(
        &mut self,
        spec: CommandSpec,
        system: impl IntoSystem<In<Args>, O, M> + 'static,
    ) -> &mut Self
    where
        O: Into<ConsoleResult> + 'static,
    {
        self.init_resource::<ConsoleRegistry>();
        let system_id = self
            .world_mut()
            .register_system(system.map(into_console_result::<O>));
        self.world_mut()
            .resource_mut::<ConsoleRegistry>()
            .register_result_spec(spec, system_id);
        self
    }

    fn add_console_completer<M>(
        &mut self,
        command: &str,
        argument_index: usize,
        completer: impl IntoSystem<In<CompletionRequest>, Vec<CompletionItem>, M> + 'static,
    ) -> &mut Self {
        self.init_resource::<ConsoleRegistry>();
        let system_id = self.world_mut().register_system(completer);
        let registered = self
            .world_mut()
            .resource_mut::<ConsoleRegistry>()
            .register_completer(command, argument_index, system_id);
        assert!(
            registered,
            "cannot attach a completer to unknown command `{command}`"
        );
        self
    }

    #[cfg(feature = "resource-properties")]
    fn add_console_resource<R: ConsoleResource>(&mut self) -> &mut Self {
        resource_properties::register_resource::<R>(self);
        self
    }
}

fn into_console_result<O: Into<ConsoleResult>>(output: O) -> ConsoleResult {
    output.into()
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
    state.is_none_or(|s| !s.open)
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
///     ..default()
/// });
/// ```
#[derive(Default)]
pub struct ChillConsole {
    pub config: ConsoleConfig,
    /// Built-in commands registered by the plugin. Defaults to `help` and
    /// `clear`.
    pub builtin_commands: BuiltinCommands,
}

impl ChillConsole {
    /// Replaces the enabled built-in commands.
    ///
    /// ```
    /// # use chill_bevy_console::{BuiltinCommand, ChillConsole};
    /// let plugin = ChillConsole::default()
    ///     .with_builtin_commands([BuiltinCommand::Help, BuiltinCommand::Alias]);
    /// ```
    pub fn with_builtin_commands(mut self, commands: impl Into<BuiltinCommands>) -> Self {
        self.builtin_commands = commands.into();
        self
    }
}

impl Plugin for ChillConsole {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<InputFocusPlugin>() {
            app.add_plugins(InputFocusPlugin);
        }
        if !app.is_plugin_added::<InputDispatchPlugin>() {
            app.add_plugins(InputDispatchPlugin);
        }
        if !app.is_plugin_added::<EditableTextInputPlugin>() {
            app.add_plugins(EditableTextInputPlugin);
        }

        // Embed font bytes into Assets<Font> before ConsoleAssets is initialized,
        // so that FromWorld can resolve UBUNTU_MONO_FONT_HANDLE immediately.
        #[cfg(feature = "embedded-font")]
        {
            let bytes = include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/UbuntuMono-R.ttf"
            ));
            let font = Font::from_bytes(bytes.to_vec());
            let _ = app
                .world_mut()
                .resource_mut::<Assets<Font>>()
                .insert(&UBUNTU_MONO_FONT_HANDLE, font);
        }

        #[cfg(feature = "persistent-history")]
        let (initial_state, initial_buffer) = persistence::load_initial_data(&self.config);
        #[cfg(not(feature = "persistent-history"))]
        let (initial_state, initial_buffer) = (
            ConsoleState::default(),
            ConsoleBuffer::new(self.config.max_history_lines),
        );

        app.insert_resource(self.config.clone())
            .insert_resource(self.builtin_commands.clone())
            .init_resource::<ConsoleRegistry>()
            .init_resource::<ConsoleAliases>()
            .init_resource::<ConsoleBinds>()
            .init_resource::<ConsoleLogCapture>()
            .insert_resource(initial_state)
            .insert_resource(initial_buffer)
            .init_resource::<ConsoleCommandQueue>()
            .init_resource::<ConsoleAssets>()
            .add_message::<ConsoleRequest>()
            .add_message::<ConsoleLineMessage>()
            .add_message::<ConsoleCommandExecuted>()
            .add_plugins(commands::plugin)
            .add_systems(
                Update,
                (
                    handle_toggle_key,
                    sync_console_ui,
                    sync_console_input.run_if(console_open),
                    capture_console_input,
                    queue_bound_commands,
                    queue_console_requests,
                    refresh_completions.run_if(has_dirty_completion),
                    scroll_console,
                    execute_pending_commands.run_if(has_pending_command),
                    collect_console_lines,
                    drain_captured_logs,
                    update_console_ui.run_if(console_open_and_changed),
                )
                    .chain(),
            );

        #[cfg(feature = "resource-properties")]
        app.add_plugins(resource_properties::plugin);

        #[cfg(feature = "persistent-history")]
        app.add_plugins(persistence::plugin);
    }
}
