use std::fmt;

use colored::{ColoredString, Colorize};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::field::Visit;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

use crate::Result;

/// Setup logging with a charmbracelet/log-style human-friendly formatter.
///
/// Output looks like:
/// ```text
/// 14:02:51 INFO  DNS server listening on UDP 0.0.0.0:53
/// 14:02:53 WARN  Query from 10.0.0.4 has no questions
/// 14:02:55 ERROR Error handling query: timed out
/// ```
/// Structured fields are rendered as dimmed `key=value` pairs after the message.
pub fn setup_logging() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .event_format(CharmFormatter)
        .init();

    Ok(())
}

/// A compact, colorful event formatter inspired by charmbracelet/log.
struct CharmFormatter;

impl<S, N> FormatEvent<S, N> for CharmFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        // Dimmed timestamp (local wall-clock time, seconds resolution).
        let now = chrono::Local::now().format("%H:%M:%S");
        write!(writer, "{} ", now.to_string().dimmed())?;

        // Colored, fixed-width level badge.
        write!(writer, "{} ", level_badge(meta.level()))?;

        // Message, then the remaining fields as `key=value` pairs.
        let mut visitor = CharmVisitor {
            writer: &mut writer,
            wrote_message: false,
            result: Ok(()),
        };
        event.record(&mut visitor);
        visitor.result?;

        writeln!(writer)
    }
}

/// Renders a 5-char-wide, colored level label (matching charm's vibe).
fn level_badge(level: &Level) -> ColoredString {
    match *level {
        Level::TRACE => "TRACE".magenta().bold(),
        Level::DEBUG => "DEBUG".blue().bold(),
        Level::INFO => "INFO ".green().bold(),
        Level::WARN => "WARN ".yellow().bold(),
        Level::ERROR => "ERROR".red().bold(),
    }
}

/// Visitor that prints the `message` field bare and everything else as
/// dimmed `key=value` pairs.
struct CharmVisitor<'a, 'w> {
    writer: &'a mut Writer<'w>,
    wrote_message: bool,
    result: fmt::Result,
}

impl CharmVisitor<'_, '_> {
    fn write_field(&mut self, name: &str, value: &dyn fmt::Debug) {
        if self.result.is_err() {
            return;
        }
        self.result = if name == "message" {
            self.wrote_message = true;
            write!(self.writer, "{:?}", DebugAsDisplay(value))
        } else {
            // Leading space separates fields from the message / each other.
            write!(
                self.writer,
                " {}={}",
                name.dimmed(),
                format!("{:?}", DebugAsDisplay(value)).dimmed()
            )
        };
    }
}

impl Visit for CharmVisitor<'_, '_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.write_field(field.name(), value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        // Avoid the surrounding quotes that Debug would add for strings.
        if self.result.is_err() {
            return;
        }
        if field.name() == "message" {
            self.wrote_message = true;
            self.result = write!(self.writer, "{}", value);
        } else {
            self.result = write!(self.writer, " {}={}", field.name().dimmed(), value.dimmed());
        }
    }
}

/// Prints a `Debug` value but strips the outer quotes that strings get,
/// so messages read naturally.
struct DebugAsDisplay<'a>(&'a dyn fmt::Debug);

impl fmt::Debug for DebugAsDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered = format!("{:?}", self.0);
        let trimmed = rendered
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&rendered);
        f.write_str(trimmed)
    }
}
