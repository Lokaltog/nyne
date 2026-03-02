//! Event sinks for invalidation event collection and processing.

use std::mem;

use parking_lot::Mutex;

use super::invalidation::{EventSink, InvalidationEvent};

/// An [`EventSink`] that logs events via `tracing::debug`.
///
/// Fire-and-forget — `drain()` always returns empty (default impl).
pub struct LoggingEventSink;

impl EventSink for LoggingEventSink {
    fn emit(&self, event: InvalidationEvent) {
        tracing::debug!(?event, "invalidation event");
    }
}

/// An [`EventSink`] that collects events for deferred processing.
///
/// Events emitted during a FUSE operation are buffered and drained
/// by the Router after the operation completes. This decouples event
/// emission (which happens inside provider/node code) from event
/// processing (which requires Router access).
pub struct BufferedEventSink {
    buffer: Mutex<Vec<InvalidationEvent>>,
}

impl Default for BufferedEventSink {
    fn default() -> Self { Self::new() }
}

impl BufferedEventSink {
    pub const fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
        }
    }
}

impl EventSink for BufferedEventSink {
    fn emit(&self, event: InvalidationEvent) {
        tracing::debug!(?event, "invalidation event (buffered)");
        self.buffer.lock().push(event);
    }

    fn drain(&self) -> Vec<InvalidationEvent> {
        let mut buf = self.buffer.lock();
        mem::take(&mut *buf)
    }
}
