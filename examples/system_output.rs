//! Write output from ordinary Bevy systems with `ConsoleLineMessage`.
//!
//! Open the console with `` ` `` to see the startup messages.
//!
//! Run with: `cargo run --example system_output`

use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use chill_bevy_console::{ChillConsole, ConsoleLevel, ConsoleLineMessage, ConsoleLineSource};
use std::time::Duration;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .add_systems(Startup, (setup, report_startup))
        .add_systems(
            Update,
            every_second.run_if(on_timer(Duration::from_secs(1))),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn report_startup(mut console: MessageWriter<ConsoleLineMessage>) {
    console.write(ConsoleLineMessage::info("Game initialized"));
    console.write(ConsoleLineMessage {
        level: ConsoleLevel::Warn,
        source: ConsoleLineSource::System,
        text: "This is a warning from a game system".into(),
    });
}

fn every_second(mut console: MessageWriter<ConsoleLineMessage>, time: Res<Time>) {
    let elapsed_secs = time.elapsed_secs();
    console.write(ConsoleLineMessage::info(format!(
        "Elapsed sec {elapsed_secs}"
    )));
}
