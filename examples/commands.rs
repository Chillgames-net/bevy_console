//! Demonstrates the `Args` helpers: `get`, `parse`, and `rest`, plus a command
//! that takes a system param (`ResMut<Score>`).
//!
//! Try:
//!   teleport 12.5 -3
//!   greet Ben
//!   echo this is a long message
//!   add_score 5
//!
//! Run with: `cargo run --example commands`

use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, CommandArgs, ConsoleAppExt};

#[derive(Resource, Default)]
struct Score(i32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .init_resource::<Score>()
        .add_console_command(
            "teleport",
            "teleport <x> <y> — parse two floats",
            teleport_cmd,
        )
        .add_console_command("greet", "greet <name> — uses args.get", greet_cmd)
        .add_console_command("echo", "echo <text...> — uses args.rest", echo_cmd)
        .add_console_command(
            "add_score",
            "add_score <n> — mutate a resource",
            add_score_cmd,
        )
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn teleport_cmd(In(args): CommandArgs) -> String {
    let (Some(x), Some(y)) = (args.parse::<f32>(0), args.parse::<f32>(1)) else {
        return "Usage: teleport <x> <y>".to_string();
    };
    format!("Teleporting to ({x}, {y})")
}

fn greet_cmd(In(args): CommandArgs) -> String {
    match args.get(0) {
        Some(name) => format!("Hello, {name}!"),
        None => "Usage: greet <name>".to_string(),
    }
}

fn echo_cmd(In(args): CommandArgs) -> String {
    args.rest(0)
}

fn add_score_cmd(In(args): CommandArgs, mut score: ResMut<Score>) -> String {
    let Some(n) = args.parse::<i32>(0) else {
        return "Usage: add_score <n>".to_string();
    };
    score.0 += n;
    format!("Score is now {}", score.0)
}
