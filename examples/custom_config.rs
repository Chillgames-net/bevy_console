//! Customize colors, sizes, and the toggle key via `ConsoleConfig`.
//!
//! This example uses the built-in `chillgames()` preset and overrides the
//! toggle key to `F1`. Swap in `matrix()`, `source()`, or build your own.
//!
//! Run with: `cargo run --example custom_config`

use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, ConsoleConfig};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole {
            config: ConsoleConfig {
                toggle_key: KeyCode::F1,
                ..ConsoleConfig::chillgames()
            },
        })
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}
