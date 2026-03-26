//! Event sinks for invalidation event collection and processing.

//! Event sink implementations for invalidation event collection.
//!
//! Two concrete [`EventSink`] implementations are provided:
//!
//! - [`LoggingEventSink`] — fire-and-forget, for contexts where events are
//!   informational only (e.g., nested resolver calls that cannot trigger
//!   cache invalidation themselves).
//! - [`BufferedEventSink`] — collects events during a FUSE operation so the
//!   router can process them after the operation completes. This decoupling
//!   is necessary because event processing requires `&mut` access to caches,
//!   but providers hold only `&` references during their callbacks.

use std::mem;

use parking_lot::Mutex;

use super::invalidation::{EventSink, InvalidationEvent};

/// An [`EventSink`] that logs events via `tracing::debug`.
///
/// Fire-and-forget — `drain()` always returns empty (default impl).
pub struct LoggingEventSink;

/// [`EventSink`] implementation that logs events via `tracing::debug`.
impl EventSink for LoggingEventSink {
    /// Log the event at debug level.
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
#[derive(Default)]
pub struct BufferedEventSink {
    buffer: Mutex<Vec<InvalidationEvent>>,
}

/// Construction for the buffered event sink.
impl BufferedEventSink {
    /// Create a new buffered event sink with an empty buffer.
    pub const fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
        }
    }
}

/// [`EventSink`] implementation that buffers events for deferred processing.
impl EventSink for BufferedEventSink {
    /// Log and buffer the event for later draining.
    fn emit(&self, event: InvalidationEvent) {
        tracing::debug!(?event, "invalidation event (buffered)");
        self.buffer.lock().push(event);
    }

    /// Take all buffered events, leaving the buffer empty.
    fn drain(&self) -> Vec<InvalidationEvent> {
        let mut buf = self.buffer.lock();
        mem::take(&mut *buf)
    }
}
