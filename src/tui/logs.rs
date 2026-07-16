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
    /// Set when the event carries `blocked = true` and a `domain` field
    /// (see the blocked-query log call in dns.rs)
    pub blocked_domain: Option<String>,
    /// How many consecutive times this exact line was logged; rendered as
    /// an `(xN)` suffix when > 1, mirroring the CLI formatter's collapsing
    pub repeat: u64,
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

        let blocked_domain = if visitor.blocked {
            visitor.domain.clone()
        } else {
            None
        };

        let mut body = visitor.message;
        body.push_str(&visitor.fields);

        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        let level = *event.metadata().level();

        let mut buffer = self.buffer.lock().unwrap();

        // Collapse consecutive identical lines (same repeated blocked tracker,
        // typically) into one entry with a bumped repeat counter, like the CLI.
        if let Some(last) = buffer.back_mut() {
            if last.level == level && last.body == body {
                last.repeat += 1;
                last.time = time;
                return;
            }
        }

        if buffer.len() >= self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(LogLine {
            time,
            level,
            body,
            blocked_domain,
            repeat: 1,
        });
    }
}

/// Collects the `message` field and renders the rest as ` key=value` pairs
#[derive(Default)]
struct LineVisitor {
    message: String,
    fields: String,
    domain: Option<String>,
    blocked: bool,
}

impl LineVisitor {
    fn record(&mut self, name: &str, value: String) {
        match name {
            "message" => self.message = value,
            // Structured marker on blocked-query events; kept out of the body
            "blocked" => self.blocked = value == "true",
            _ => {
                if name == "domain" {
                    self.domain = Some(value.clone());
                }
                self.fields.push_str(&format!(" {}={}", name, value));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn consecutive_identical_lines_collapse_into_repeat_counter() {
        let buffer: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
        let subscriber =
            tracing_subscriber::registry().with(TuiLogLayer::new(Arc::clone(&buffer), 10));

        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..3 {
                tracing::info!(domain = "ads.example.com", blocked = true, "blocked");
            }
            tracing::info!(domain = "other.example.com", blocked = true, "blocked");
            tracing::info!(domain = "ads.example.com", blocked = true, "blocked");
        });

        let buffer = buffer.lock().unwrap();
        let repeats: Vec<u64> = buffer.iter().map(|l| l.repeat).collect();
        // A repeat only collapses while consecutive; interleaved lines start over
        assert_eq!(repeats, vec![3, 1, 1]);
        assert_eq!(buffer[0].blocked_domain.as_deref(), Some("ads.example.com"));
    }
}
