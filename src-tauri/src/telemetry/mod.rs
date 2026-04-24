#![allow(dead_code)]
// ============================================================================
// Récupère — Local tracing ring buffer
// ============================================================================
// An in-memory, bounded ring of the most recent `tracing` events. Used by
// the support bundle and the Tauri command `get_recent_traces` so operators
// can attach a fresh log excerpt to a support ticket without grepping
// through GB of stderr. Events NEVER leave the device — AGENTS.md forbids
// cloud-imposed telemetry for sensitive operations.
//
// Capacity is bounded (`RING_CAPACITY`), oldest entries drop when full. The
// layer plugs into the same `tracing_subscriber::Registry` that the rest of
// the app already uses so no call-site change is needed.
// ============================================================================

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum events kept in memory. ~500 events is enough to cover a full
/// interactive session without blowing past a few MB of residency.
pub const RING_CAPACITY: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Unix epoch milliseconds when the event fired.
    pub timestamp_ms: u64,
    /// TRACE / DEBUG / INFO / WARN / ERROR
    pub level: String,
    /// Tracing target (usually a module path like `recupere::imaging`).
    pub target: String,
    /// Rendered event message (fields are flattened into `key=value` pairs).
    pub message: String,
}

static RING: OnceLock<Mutex<RingBuffer>> = OnceLock::new();

fn ring() -> &'static Mutex<RingBuffer> {
    RING.get_or_init(|| Mutex::new(RingBuffer::new(RING_CAPACITY)))
}

/// Return up to `limit` most recent entries, newest last. Passing `None`
/// returns the full ring.
pub fn recent_traces(limit: Option<usize>) -> Vec<TraceEntry> {
    let guard = match ring().lock() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    };
    let take = limit.unwrap_or(guard.buffer.len()).min(guard.buffer.len());
    let start = guard.buffer.len().saturating_sub(take);
    guard.buffer.iter().skip(start).cloned().collect()
}

/// Drop every captured entry. Exposed so the UI can clear the view before
/// reproducing a bug (keeps the support bundle focused on the repro run).
pub fn clear_traces() {
    if let Ok(mut guard) = ring().lock() {
        guard.buffer.clear();
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

struct RingBuffer {
    buffer: std::collections::VecDeque<TraceEntry>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: std::collections::VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, entry: TraceEntry) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(entry);
    }
}

// ---------------------------------------------------------------------------
// tracing::Layer implementation
// ---------------------------------------------------------------------------

pub struct RingBufferLayer;

impl RingBufferLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RingBufferLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let entry = TraceEntry {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            level: level_to_string(*meta.level()),
            target: meta.target().to_string(),
            message: visitor.finish(),
        };

        if let Ok(mut guard) = ring().lock() {
            guard.push(entry);
        }
    }
}

fn level_to_string(level: Level) -> String {
    match level {
        Level::TRACE => "TRACE",
        Level::DEBUG => "DEBUG",
        Level::INFO => "INFO",
        Level::WARN => "WARN",
        Level::ERROR => "ERROR",
    }
    .to_string()
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl MessageVisitor {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.fields
        } else {
            format!("{} {}", self.message, self.fields)
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(&mut self.message, "{value:?}");
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            let _ = write!(&mut self.fields, "{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            let _ = write!(&mut self.fields, "{}={value}", field.name());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_bounded_to_capacity() {
        let mut ring = RingBuffer::new(3);
        for id in 0..10 {
            ring.push(TraceEntry {
                timestamp_ms: id,
                level: "INFO".into(),
                target: "test".into(),
                message: format!("event {id}"),
            });
        }
        assert_eq!(ring.buffer.len(), 3);
        // Oldest survivors are the last 3 pushes (7, 8, 9).
        assert_eq!(ring.buffer.front().unwrap().timestamp_ms, 7);
        assert_eq!(ring.buffer.back().unwrap().timestamp_ms, 9);
    }

    #[test]
    fn level_to_string_covers_every_variant() {
        assert_eq!(level_to_string(Level::TRACE), "TRACE");
        assert_eq!(level_to_string(Level::DEBUG), "DEBUG");
        assert_eq!(level_to_string(Level::INFO), "INFO");
        assert_eq!(level_to_string(Level::WARN), "WARN");
        assert_eq!(level_to_string(Level::ERROR), "ERROR");
    }

    #[test]
    fn recent_traces_returns_most_recent_n() {
        // Seed the global ring with known entries.
        clear_traces();
        for id in 0..20 {
            if let Ok(mut guard) = ring().lock() {
                guard.push(TraceEntry {
                    timestamp_ms: id,
                    level: "INFO".into(),
                    target: "test".into(),
                    message: format!("event {id}"),
                });
            }
        }
        let last_five = recent_traces(Some(5));
        assert_eq!(last_five.len(), 5);
        assert_eq!(last_five.first().unwrap().timestamp_ms, 15);
        assert_eq!(last_five.last().unwrap().timestamp_ms, 19);

        let all = recent_traces(None);
        assert_eq!(all.len(), 20);

        clear_traces();
    }
}
