//! Minimal setup: open the console with `` ` `` and try `say hello world`.
//!
//! Run with: `cargo run --example basic`

use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, CommandArgs, ConsoleAppExt};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
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
