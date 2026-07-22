//! Inspect and change a reflected Bevy state from the developer console.
//!
//! Try:
//!   state get GameState
//!   state set GameState Playing
//!   state set GameState Paused
//!
//! Run with: `cargo run --example states`

use bevy::prelude::*;
use chill_bevy_console::{BuiltinCommand, ChillConsole, ConsoleAppExt, ConsoleLineMessage};

#[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    Menu,
    Playing,
    Paused,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default().with_builtin_commands([BuiltinCommand::State]))
        .init_state::<GameState>()
        .add_console_state::<GameState>()
        .add_systems(Startup, setup)
        .add_systems(Update, report_state)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn report_state(state: Res<State<GameState>>, mut console: MessageWriter<ConsoleLineMessage>) {
    if state.is_changed() {
        console.write(ConsoleLineMessage::info(format!(
            "Game state: {:?}",
            state.get()
        )));
    }
}
