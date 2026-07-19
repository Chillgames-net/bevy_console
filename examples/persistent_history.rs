//! Persist submitted-command recall between runs. Requires the
//! `persistent-history` feature.
//!
//! Run a command, close the app, reopen — it is available through Up/Down recall.
//!
//! Run with: `cargo run --example persistent_history --features persistent-history`

use bevy::prelude::*;
use chill_bevy_console::{
    ChillConsole, CommandArgs, ConsoleAppExt, ConsoleCommand, ConsolePersistence,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole {
            persistence: ConsolePersistence {
                history_file: "console_history.txt".into(),
                ..default()
            },
            ..default()
        })
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
