//! Persist console history between runs. Requires the `persistent-history` feature.
//!
//! Run a command, close the app, reopen — the history is still there.
//!
//! Run with: `cargo run --example persistent_history --features persistent-history`

use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, CommandArgs, ConsoleAppExt, ConsoleConfig};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole {
            config: ConsoleConfig {
                #[cfg(feature = "persistent-history")]
                history_file: Some("console_history.txt".into()),
                ..default()
            },
        })
        .add_console_command("say", "say <text> — echo text", say_cmd)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}
