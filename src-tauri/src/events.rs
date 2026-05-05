//! Event sink abstraction shared by Tauri commands and the HTTP test-server.
//!
//! Both ingest and ask emit per-step events (`ingest:page_started`, `ask:token`,
//! etc.). In production they go through Tauri's `app.emit(...)`; under the
//! `test-server` binary they are forwarded to an SSE channel. The orchestration
//! code accepts a sink and stays transport-agnostic.

use std::sync::Arc;

use serde::Serialize;

/// A sink takes (event_name, json_payload). Cheap, callable from sync or async.
pub type EventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// No-op sink useful for tests / contexts where we don't care about events.
pub fn noop_sink() -> EventSink {
    Arc::new(|_, _| {})
}

/// Convenience helper that serializes a payload and forwards to the sink.
pub fn emit<T: Serialize>(sink: &EventSink, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(v) => sink(event, v),
        Err(e) => tracing::warn!("emit {event} serialize failed: {e}"),
    }
}
