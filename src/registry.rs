use bevy::ecs::system::SystemId;
use bevy::prelude::*;
use std::collections::BTreeMap;

pub struct CommandDef {
    pub usage: &'static str,
    pub system_id: SystemId<In<Vec<String>>, String>,
}

/// Registry of all commands available in the console.
///
/// You can inject this as a resource and call `register` directly, but the
/// preferred way is `app.add_console_command(name, usage, my_system)`.
#[derive(Resource, Default)]
pub struct ConsoleRegistry {
    pub commands: BTreeMap<String, CommandDef>,
}

impl ConsoleRegistry {
    pub fn register(
        &mut self,
        name: &str,
        usage: &'static str,
        system_id: SystemId<In<Vec<String>>, String>,
    ) {
        self.commands
            .insert(name.to_string(), CommandDef { usage, system_id });
    }
}
