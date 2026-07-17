//! Runtime aliases, configurable built-ins, and key bindings.
//!
//! With the console closed, try the following bindings:
//! - `F1` adds 10 points.
//! - `Shift+F1` adds 100 points.
//! - `Ctrl+F1` prints the current score.
//!
//! You can inspect or change them in the console:
//! ```text
//! bind list
//! bind set shift+F1 add_score 250
//! bind remove meta+F1
//! alias set bonus add_score 500
//! bind set F2 bonus
//! ```
//!
//! Run with: `cargo run --example bindings`

use bevy::prelude::*;
use chill_bevy_console::{
    BuiltinCommand, ChillConsole, CommandArgs, ConsoleAppExt, ConsoleBinds, ConsoleKeyBinding,
    ConsoleKeyModifiers,
};

#[derive(Resource, Default)]
struct Score(u32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_resource::<Score>()
        .add_plugins(ChillConsole::default().with_builtin_commands(BuiltinCommand::all()))
        .add_console_command(
            "add_score",
            "add_score <amount> - increase the score",
            add_score,
        )
        .add_console_command("score", "score - show the current score", show_score)
        .add_systems(Startup, (setup, add_default_bindings))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn add_default_bindings(mut binds: ResMut<ConsoleBinds>) {
    binds.set(KeyCode::F1, "add_score 10");
    binds.set_binding(
        ConsoleKeyBinding {
            key: KeyCode::F1,
            modifiers: ConsoleKeyModifiers {
                shift: true,
                ..default()
            },
        },
        "add_score 100",
    );
    binds.set_binding(
        ConsoleKeyBinding {
            key: KeyCode::F1,
            modifiers: ConsoleKeyModifiers {
                ctrl: true,
                ..default()
            },
        },
        "score",
    );
}

fn add_score(In(args): CommandArgs, mut score: ResMut<Score>) -> String {
    let Some(amount) = args.parse::<u32>(0) else {
        return "Usage: add_score <amount>".into();
    };
    score.0 += amount;
    format!("Score: {}", score.0)
}

fn show_score(In(_args): CommandArgs, score: Res<Score>) -> String {
    format!("Score: {}", score.0)
}
