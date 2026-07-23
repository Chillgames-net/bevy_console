//! Minimal setup: open the console with `` ` `` and try:
//!   say hello world
//!   state get GameState
//!   state set GameState Playing
//!
//! Run with: `cargo run --example basic`

use bevy::prelude::*;
use chill_bevy_console::{
    BuiltinCommand, ChillConsole, CommandArgs, ConsoleAppExt, ConsoleCommand,
};

#[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    Menu,
    Playing,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default().with_builtin_commands([
            BuiltinCommand::Help,
            BuiltinCommand::Clear,
            BuiltinCommand::State,
        ]))
        .init_state::<GameState>()
        .add_console_state::<GameState>()
        .add_console_command(ConsoleCommand::new(
            "say",
            "say <text> - echo text",
            say_cmd,
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}
