//! Command request queueing, execution, and output collection.

use crate::{
    Args, CommandExecutor, ConsoleAliases, ConsoleBuffer, ConsoleCommandExecuted,
    ConsoleCommandQueue, ConsoleLevel, ConsoleLineMessage, ConsoleLineSource, ConsoleRegistry,
    ConsoleRequest, ConsoleState,
};
use bevy::prelude::*;

pub(crate) fn has_pending_command(queue: Option<Res<ConsoleCommandQueue>>) -> bool {
    queue.is_some_and(|queue| !queue.is_empty())
}

/// Adds application requests to the same FIFO queue as local input.
pub(crate) fn queue_console_requests(
    mut requests: MessageReader<ConsoleRequest>,
    mut queue: ResMut<ConsoleCommandQueue>,
) {
    for request in requests.read() {
        queue.push(request.clone());
    }
}

/// Receives structured lines emitted by game systems.
pub(crate) fn collect_console_lines(
    mut messages: MessageReader<ConsoleLineMessage>,
    mut buffer: ResMut<ConsoleBuffer>,
) {
    for line in messages.read() {
        buffer.push(line.level, line.source.clone(), &line.text);
    }
}

pub(crate) fn execute_pending_commands(world: &mut World) {
    let Some(queued) = world.resource_mut::<ConsoleCommandQueue>().pop_front() else {
        return;
    };
    let request = queued.request;
    let cmd_str = request.input;

    let parsed = crate::ParsedInput::parse(&cmd_str);
    if let Some(error) = parsed.error {
        write_line(
            world,
            ConsoleLevel::Error,
            ConsoleLineSource::System,
            format!("Parse error: {}", error.message),
        );
        world.write_message(ConsoleCommandExecuted {
            input: cmd_str,
            command: None,
            origin: request.origin,
            succeeded: false,
        });
        return;
    }
    let Some(name) = parsed.command() else { return };
    let args = Args::from_parsed(&parsed);
    let command = {
        let registry = world.resource::<ConsoleRegistry>();
        registry
            .get(name)
            .map(|definition| (definition.spec.name.clone(), definition.executor))
    };

    let Some((command_name, executor)) = command else {
        if let Some(expansion) = world
            .resource::<ConsoleAliases>()
            .get(name)
            .map(str::to_owned)
        {
            if queued.alias_depth >= 16 {
                write_line(
                    world,
                    ConsoleLevel::Error,
                    ConsoleLineSource::System,
                    format!("Alias expansion limit exceeded while resolving `{name}`"),
                );
                world.write_message(ConsoleCommandExecuted {
                    input: cmd_str,
                    command: Some(name.to_string()),
                    origin: request.origin,
                    succeeded: false,
                });
                return;
            }
            let suffix = &cmd_str[parsed.tokens[0].range.end..];
            let history_index = queued.history_index.or_else(|| {
                world
                    .resource_mut::<ConsoleState>()
                    .take_pending_history_index(&cmd_str)
            });
            world
                .resource_mut::<ConsoleCommandQueue>()
                .push_alias_expansion(
                    ConsoleRequest {
                        input: format!("{expansion}{suffix}"),
                        origin: request.origin,
                    },
                    queued.alias_depth + 1,
                    history_index,
                );
            return;
        }
        echo_command(world, &cmd_str, name, queued.history_index);
        write_line(
            world,
            ConsoleLevel::Error,
            ConsoleLineSource::System,
            format!("Unknown command: {name}"),
        );
        world.write_message(ConsoleCommandExecuted {
            input: cmd_str,
            command: Some(name.to_string()),
            origin: request.origin,
            succeeded: false,
        });
        return;
    };

    let command_source = ConsoleLineSource::Command {
        name: command_name.clone(),
    };
    echo_command(world, &cmd_str, &command_name, queued.history_index);
    let result = match executor {
        CommandExecutor::Structured(id) => match world.run_system_with(id, args) {
            Ok(output) => output.lines,
            Err(error) => vec![(ConsoleLevel::Error, format!("System error: {error}"))],
        },
        #[cfg(feature = "resource-properties")]
        CommandExecutor::Exclusive(command) => command(world, args).lines,
    };
    let succeeded = !result
        .iter()
        .any(|(level, _)| *level == ConsoleLevel::Error);
    for (level, text) in result {
        write_line(world, level, command_source.clone(), text);
    }
    world.write_message(ConsoleCommandExecuted {
        input: cmd_str,
        command: Some(command_name),
        origin: request.origin,
        succeeded,
    });
}

fn write_line(
    world: &mut World,
    level: ConsoleLevel,
    source: ConsoleLineSource,
    text: impl AsRef<str>,
) {
    world
        .resource_mut::<ConsoleBuffer>()
        .push(level, source, text.as_ref());
}

fn echo_command(world: &mut World, input: &str, name: &str, history_index: Option<usize>) {
    write_line(
        world,
        ConsoleLevel::Info,
        ConsoleLineSource::CommandEcho {
            name: name.to_string(),
        },
        format!("> {input}"),
    );
    if let Some(history_index) = history_index.or_else(|| {
        world
            .resource_mut::<ConsoleState>()
            .take_pending_history_index(input)
    }) && let Some(line_id) = world
        .resource::<ConsoleBuffer>()
        .last_line()
        .map(|line| line.id)
    {
        world
            .resource_mut::<ConsoleState>()
            .set_history_line_id(history_index, line_id);
    }
}
