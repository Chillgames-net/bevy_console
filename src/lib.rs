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
//! use chill_bevy_console::{ChillConsole, CommandArgs, ConsoleAppExt, ConsoleCommand, console_closed};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(ChillConsole::default())
//!         .add_console_command(ConsoleCommand::new("say", "say <text> - echo text", say_cmd))
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
//! - `persistent-history` — load and save a plain-text input/output transcript
//!   between runs. The path is configured through [`ConsolePersistence`].
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
mod config;
mod editor;
mod execution;
mod input;
mod logging;
mod model;
mod parser;
mod registry;
mod scroll;
mod state;
mod state_commands;
mod ui;

mod resource_properties;

#[cfg(feature = "persistent-history")]
mod persistence;

pub use args::Args;
pub use config::{BuiltinCommand, BuiltinCommands, ConsoleConfig};
pub use logging::console_log_layer;
pub use model::{
    ArgumentKind, ArgumentSpec, CommandOrigin, CompletionItem, CompletionRequest, ConsoleAliases,
    ConsoleBinds, ConsoleBuffer, ConsoleCommand, ConsoleCommandExecuted, ConsoleKeyBinding,
    ConsoleKeyModifiers, ConsoleLevel, ConsoleLine, ConsoleLineMessage, ConsoleLineSource,
    ConsoleRequest, ConsoleResult,
};
pub use parser::{ParseError, ParsedInput, ParsedToken, QuoteStyle};
#[cfg(feature = "persistent-history")]
pub use persistence::ConsolePersistence;
pub use registry::ConsoleRegistry;
pub use state::ConsoleState;

pub use resource_properties::{ConsoleProperty, ConsolePropertyValue};

pub(crate) use logging::ConsoleLogCapture;
pub(crate) use model::ConsoleCommandQueue;

#[cfg(feature = "embedded-font")]
use bevy::asset::uuid_handle;
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::prelude::*;
use bevy::reflect::{FromReflect, GetTypeRegistration, Typed};
use bevy::state::{app::AppExtStates, state::FreelyMutableState};
use bevy::ui_widgets::EditableTextInputPlugin;

// ── Embedded font ──────────────────────────────────────────────────────────────

/// Stable handle for the embedded UbuntuMono font.
/// Only meaningful when the `embedded-font` feature is enabled.
#[cfg(feature = "embedded-font")]
pub const UBUNTU_MONO_FONT_HANDLE: Handle<Font> =
    uuid_handle!("7fca4e91-3b58-d20a-9c63-e0174f2b85d6");

use completion::{has_dirty_completion, refresh_completions};
use execution::{
    collect_console_lines, execute_pending_commands, has_pending_command, queue_console_requests,
};
use input::{
    capture_console_input, console_open, focus_console_input, handle_toggle_key,
    queue_bound_commands, sync_console_input,
};
use logging::drain_captured_logs;
use scroll::scroll_console;
use ui::{ConsoleAssets, console_open_and_changed, sync_console_ui, update_console_ui};

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

/// The input type for console completion systems.
///
/// ```no_run
/// # use chill_bevy_console::ConsoleCompletionRequest;
/// # use bevy::prelude::*;
/// fn complete_map(In(request): ConsoleCompletionRequest) -> Vec<String> {
///     match request.argument_index() {
///         0 => vec!["forest".into()],
///         _ => Vec::new(),
///     }
/// }
/// ```
pub type ConsoleCompletionRequest = In<CompletionRequest>;

// ── App extension ──────────────────────────────────────────────────────────────

pub trait ConsoleAppExt {
    /// Registers a console command builder.
    ///
    /// ```no_run
    /// # use chill_bevy_console::{CommandArgs, ConsoleAppExt, ConsoleCommand};
    /// # use bevy::prelude::*;
    /// fn say_cmd(In(args): CommandArgs) -> String {
    ///     args.join(" ")
    /// }
    /// # let mut app = App::new();
    /// app.add_console_command(
    ///     ConsoleCommand::new("say", "say <text> - echo text", say_cmd),
    /// );
    /// ```
    fn add_console_command(&mut self, command: ConsoleCommand) -> &mut Self;

    /// Registers supported reflected fields from a Bevy resource.
    ///
    /// Properties use the resource's reflected short type path, or its full type
    /// path when multiple registered resources share the same short path. The
    /// resource must derive [`Reflect`] with `#[reflect(Resource)]`.
    fn add_console_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Reflect + FromReflect + GetTypeRegistration + Typed;

    /// Registers an application-specific reflected field type for resource properties.
    fn register_console_property_value<T>(&mut self) -> &mut Self
    where
        T: ConsolePropertyValue + GetTypeRegistration;

    /// Register a reflected Bevy state for the built-in `state` command.
    ///
    /// Call this after [`bevy::state::app::AppExtStates::init_state`].
    fn add_console_state<S>(&mut self) -> &mut Self
    where
        S: FreelyMutableState + FromReflect + GetTypeRegistration + Typed;
}

impl ConsoleAppExt for App {
    fn add_console_command(&mut self, command: ConsoleCommand) -> &mut Self {
        self.init_resource::<ConsoleRegistry>();
        let system_id = self.world_mut().register_boxed_system(command.system);
        let completer_id = command
            .completer
            .map(|completer| self.world_mut().register_boxed_system(completer));
        self.world_mut().resource_mut::<ConsoleRegistry>().register(
            command.spec,
            system_id,
            completer_id,
        );
        self
    }

    fn add_console_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Reflect + FromReflect + GetTypeRegistration + Typed,
    {
        resource_properties::register_resource::<R>(self);
        self
    }

    fn register_console_property_value<T>(&mut self) -> &mut Self
    where
        T: ConsolePropertyValue + GetTypeRegistration,
    {
        resource_properties::register_property_value::<T>(self);
        self
    }

    fn add_console_state<S>(&mut self) -> &mut Self
    where
        S: FreelyMutableState + FromReflect + GetTypeRegistration + Typed,
    {
        self.register_type_mutable_state::<S>()
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
    /// Settings for optional transcript persistence.
    #[cfg(feature = "persistent-history")]
    pub persistence: ConsolePersistence,
    /// Built-in commands registered by the plugin.
    /// Defaults to `help` and `clear`.
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
        let (initial_state, initial_buffer) =
            persistence::load_initial_data(&self.config, &self.persistence);
        #[cfg(not(feature = "persistent-history"))]
        let (initial_state, initial_buffer) = (
            ConsoleState::default(),
            ConsoleBuffer::new(self.config.max_history_lines),
        );

        app.insert_resource(self.config.clone());
        #[cfg(feature = "persistent-history")]
        app.insert_resource(self.persistence.clone());

        app.insert_resource(self.builtin_commands.clone())
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
            .add_plugins(state_commands::plugin)
            .add_systems(
                Update,
                (
                    handle_toggle_key,
                    sync_console_ui,
                    focus_console_input.run_if(console_open),
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

        app.add_plugins(resource_properties::plugin);

        #[cfg(feature = "persistent-history")]
        app.add_plugins(persistence::plugin);
    }
}
