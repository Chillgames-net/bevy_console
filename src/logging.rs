//! Optional bridge from `tracing`/Bevy logs into the console buffer.
//!
//! Install [`console_log_layer`] through `bevy::log::LogPlugin::custom_layer`.
//! The layer only queues lightweight records; [`drain_captured_logs`] moves them
//! into ECS resources on the main thread.

use crate::{ConsoleBuffer, ConsoleLevel, ConsoleLineMessage, ConsoleLineSource};
use bevy::log::{self, tracing, tracing_subscriber};
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

const PERSISTENCE_LOG_TARGET: &str = concat!(env!("CARGO_CRATE_NAME"), "::persistence");

/// Thread-safe staging area shared by the tracing layer and Bevy systems.
#[derive(Clone, Resource)]
pub struct ConsoleLogCapture {
    pending: Arc<Mutex<Vec<ConsoleLineMessage>>>,
}

impl Default for ConsoleLogCapture {
    fn default() -> Self {
        Self {
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// Factory compatible with [`bevy::log::LogPlugin::custom_layer`].
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy::log::LogPlugin;
/// use chill_bevy_console::{ChillConsole, console_log_layer};
///
/// App::new()
///     .add_plugins(DefaultPlugins.set(LogPlugin {
///         custom_layer: console_log_layer,
///         ..default()
///     }))
///     .add_plugins(ChillConsole::default());
/// ```
pub fn console_log_layer(app: &mut App) -> Option<log::BoxedLayer> {
    app.init_resource::<ConsoleLogCapture>();
    let capture = app.world().resource::<ConsoleLogCapture>().clone();
    Some(Box::new(ConsoleTracingLayer { capture }))
}

pub(crate) fn drain_captured_logs(
    capture: Res<ConsoleLogCapture>,
    mut buffer: ResMut<ConsoleBuffer>,
) {
    let Ok(mut pending) = capture.pending.lock() else {
        return;
    };
    for line in pending.drain(..) {
        buffer.push(line.level, line.source, &line.text);
    }
}

struct ConsoleTracingLayer {
    capture: ConsoleLogCapture,
}

impl<S> tracing_subscriber::Layer<S> for ConsoleTracingLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        if is_persistence_log_target(metadata.target()) {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let text = if visitor.message.is_empty() {
            visitor.fields.join(" ")
        } else if visitor.fields.is_empty() {
            visitor.message
        } else {
            format!("{} {}", visitor.message, visitor.fields.join(" "))
        };
        if text.is_empty() {
            return;
        }
        let line = ConsoleLineMessage {
            level: match *metadata.level() {
                tracing::Level::TRACE => ConsoleLevel::Trace,
                tracing::Level::DEBUG => ConsoleLevel::Debug,
                tracing::Level::INFO => ConsoleLevel::Info,
                tracing::Level::WARN => ConsoleLevel::Warn,
                tracing::Level::ERROR => ConsoleLevel::Error,
            },
            source: ConsoleLineSource::Log {
                target: metadata.target().to_string(),
            },
            text,
        };
        if let Ok(mut pending) = self.capture.pending.lock() {
            pending.push(line);
        }
    }
}

fn is_persistence_log_target(target: &str) -> bool {
    target == PERSISTENCE_LOG_TARGET
        || target
            .strip_prefix(PERSISTENCE_LOG_TARGET)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}").trim_matches('"').to_string();
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_persistence_log_target;

    #[test]
    fn persistence_logs_are_excluded_from_console_capture() {
        assert!(is_persistence_log_target("chill_bevy_console::persistence"));
        assert!(is_persistence_log_target(
            "chill_bevy_console::persistence::writer"
        ));
        assert!(!is_persistence_log_target("game::persistence"));
    }
}
