use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::field::Visit;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// A single captured log event, ready for display
#[derive(Debug, Clone)]
pub struct LogLine {
    pub time: String,
    pub level: Level,
    /// Message followed by ` key=value` pairs
    pub body: String,
    /// Set when the event carries a `domain` field with message "blocked"
    pub blocked_domain: Option<String>,
}

pub type LogBuffer = Arc<Mutex<VecDeque<LogLine>>>;

/// Tracing layer that captures formatted events into a ring buffer so the
/// TUI can render them (stdout is owned by the terminal UI).
pub struct TuiLogLayer {
    buffer: LogBuffer,
    capacity: usize,
}

impl TuiLogLayer {
    pub fn new(buffer: LogBuffer, capacity: usize) -> Self {
        Self { buffer, capacity }
    }
}

impl<S: Subscriber> Layer<S> for TuiLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = LineVisitor::default();
        event.record(&mut visitor);

        let blocked_domain = if visitor.message == "blocked" {
            visitor.domain.clone()
        } else {
            None
        };

        let mut body = visitor.message;
        body.push_str(&visitor.fields);

        let line = LogLine {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: *event.metadata().level(),
            body,
            blocked_domain,
        };

        let mut buffer = self.buffer.lock().unwrap();
        if buffer.len() >= self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }
}

/// Collects the `message` field and renders the rest as ` key=value` pairs
#[derive(Default)]
struct LineVisitor {
    message: String,
    fields: String,
    domain: Option<String>,
}

impl LineVisitor {
    fn record(&mut self, name: &str, value: String) {
        if name == "message" {
            self.message = value;
        } else {
            if name == "domain" {
                self.domain = Some(value.clone());
            }
            self.fields.push_str(&format!(" {}={}", name, value));
        }
    }
}

impl Visit for LineVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let rendered = format!("{:?}", value);
        // Strip the quotes Debug adds around strings
        let trimmed = rendered
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&rendered)
            .to_string();
        self.record(field.name(), trimmed);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record(field.name(), value.to_string());
    }
}
