use crate::{ConsoleAppExt, ConsoleCommand, ConsoleRegistry, ConsoleState};
use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.add_console_command::<Clear>()
        .add_console_command::<Help>()
        .add_console_command::<Version>();
}

pub struct Clear;
impl ConsoleCommand for Clear {
    const NAME: &'static str = "clear";
    const USAGE: &'static str = "clear — clear the console history";
    fn run(_args: &[&str], world: &mut World) -> String {
        world.resource_mut::<ConsoleState>().history.clear();
        String::new()
    }
}

pub struct Help;
impl ConsoleCommand for Help {
    const NAME: &'static str = "help";
    const USAGE: &'static str = "help — list all available commands";
    fn run(_args: &[&str], world: &mut World) -> String {
        let registry = world.resource::<ConsoleRegistry>();
        registry
            .commands
            .values()
            .map(|def| def.usage)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct Version;
impl ConsoleCommand for Version {
    const NAME: &'static str = "version";
    const USAGE: &'static str = "version — show the console plugin version";
    fn run(_args: &[&str], _world: &mut World) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
