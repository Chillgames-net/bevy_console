use crate::{CommandArgs, ConsoleAppExt, ConsoleRegistry, ConsoleState};
use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.add_console_command("clear", "clear — clear the console history", clear_cmd);
    app.add_console_command("help", "help — list all available commands", help_cmd);
}

fn clear_cmd(In(_args): CommandArgs, mut state: ResMut<ConsoleState>) -> String {
    state.history.clear();
    String::new()
}

fn help_cmd(In(_args): CommandArgs, registry: Res<ConsoleRegistry>) -> String {
    registry
        .commands
        .values()
        .map(|def| def.usage)
        .collect::<Vec<_>>()
        .join("\n")
}
