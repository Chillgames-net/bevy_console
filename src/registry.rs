use bevy::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

pub type CommandFn = Arc<dyn Fn(&[&str], &mut World) -> String + Send + Sync>;

pub struct CommandDef {
    pub usage: &'static str,
    pub func: CommandFn,
}

/// Registry of all commands available in the console.
///
/// You can inject this as a resource and call `register` directly, but the
/// preferred way is `app.add_console_command::<MyCommand>()`.
#[derive(Resource, Default)]
pub struct ConsoleRegistry {
    pub commands: BTreeMap<String, CommandDef>,
}

impl ConsoleRegistry {
    pub fn register(
        &mut self,
        name: &str,
        usage: &'static str,
        f: impl Fn(&[&str], &mut World) -> String + Send + Sync + 'static,
    ) {
        self.commands.insert(
            name.to_string(),
            CommandDef {
                usage,
                func: Arc::new(f),
            },
        );
    }
}
