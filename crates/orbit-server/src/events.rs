//! Request lifecycle events (open-source extension point): cross-cutting capabilities such as audit / metering plug in as subscribers.
//!
//! Subscription: `GET /api/events` (SSE) or a configured `event_sink` webhook.
//! The open-source side does not persist by default; audit / metering services persist after subscribing via an enhanced package.

use serde::Serialize;

/// Event type (defined by the open-source side, extended across versions).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrbitEvent {
    RequestStarted {
        request_id: String,
        method: String,
        url: String,
        user: Option<String>,
    },
    RequestCompleted {
        request_id: String,
        status: u16,
        duration_ms: u64,
        user: Option<String>,
    },
    RequestFailed {
        request_id: String,
        error: String,
        user: Option<String>,
    },
    LoadRunStarted {
        run_id: String,
        vus: u32,
        user: Option<String>,
    },
    LoadRunCompleted {
        run_id: String,
        user: Option<String>,
    },
    MockStarted {
        port: u16,
    },
    MockStopped {
        port: u16,
    },
    ReportSaved {
        report_id: String,
        user: Option<String>,
    },
    PluginLoaded {
        plugin_id: String,
        kind: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_with_type_tag() {
        let ev = OrbitEvent::RequestCompleted {
            request_id: "r1".into(),
            status: 200,
            duration_ms: 12,
            user: None,
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "request_completed");
        assert_eq!(v["status"], 200);
        assert_eq!(v["duration_ms"], 12);
    }
}
