//! Event contract: the unified event stream from the Agent loop to the frontend.
//!
//! Two channels coexist (each a fallback for the other):
//! - **Push**: the host `emit`s events to the frontend (Tauri events);
//! - **Pull**: [`EventBus`] pushes events into a ring buffer; the frontend can poll-drain it when push is unreliable.
//!
//! So even when the Tauri webview message-loop problem appears (load tests were switched to pure polling for this historically),
//! we can switch to the pull model without changing Agent logic.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::{StopReason, Usage};
use crate::plan::PlanArtifact;
use crate::proposal::Proposal;
use crate::tools::ToolKind;

/// Lifecycle state of a tool call on the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    /// Waiting for user approval.
    PendingApproval,
    /// Running.
    Running,
    /// Done.
    Completed,
    /// Failed.
    Failed,
    /// Denied by the user.
    Denied,
}

/// Events produced by the Agent.
///
/// `rename_all_fields` is required: the enum-level `rename_all` only renames **variant names**,
/// and fields inside struct variants (`turn_id` / `call_id` / `elapsed_ms` ...) are not converted to camelCase automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AiEvent {
    /// A conversation turn started.
    TurnStarted {
        /// Turn id.
        turn_id: String,
    },
    /// Assistant body delta.
    TextDelta {
        /// Turn id.
        turn_id: String,
        /// Delta text.
        delta: String,
    },
    /// Assistant chain-of-thought delta (DeepSeek / Anthropic thinking).
    ReasoningDelta {
        /// Turn id.
        turn_id: String,
        /// Delta text.
        delta: String,
    },
    /// Tool-call state change.
    ToolCall {
        /// Turn id.
        turn_id: String,
        /// Call id.
        call_id: String,
        /// Tool name.
        name: String,
        /// Authorization tier.
        kind: ToolKind,
        /// Arguments.
        arguments: Value,
        /// State.
        status: ToolStatus,
        /// State note (denial reason, etc.).
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Tool execution result.
    ToolResult {
        /// Turn id.
        turn_id: String,
        /// Call id.
        call_id: String,
        /// Whether it succeeded.
        ok: bool,
        /// Conclusion summary.
        summary: String,
        /// Structured result.
        payload: Value,
        /// Elapsed time (milliseconds).
        elapsed_ms: u64,
    },
    /// Changes **already persisted** by a write op (taken effect directly in Agent mode; the frontend replays before/after diffs from it).
    ProposalReady {
        /// Turn id.
        turn_id: String,
        /// Call id.
        call_id: String,
        /// Applied change.
        proposal: Proposal,
    },
    /// Plan mode produced (or revised) a plan, which has been persisted with the session.
    PlanReady {
        /// Turn id.
        turn_id: String,
        /// Call id.
        call_id: String,
        /// Plan (including the revision number).
        plan: PlanArtifact,
    },
    /// The host has modified workspace data; the frontend must reload the snapshot.
    DataChanged {
        /// Change scope description (e.g. `request` / `scenario` / `collection`).
        scope: String,
    },
    /// Long-task progress (scenario steps / load test stages), for display only.
    Progress {
        /// Progress text.
        message: String,
    },
    /// A conversation turn finished.
    TurnFinished {
        /// Turn id.
        turn_id: String,
        /// Stop reason.
        stop_reason: StopReason,
        /// Accumulated usage.
        usage: Usage,
        /// Whether it was cut off by reaching the maximum number of turns.
        truncated: bool,
    },
    /// Error.
    Error {
        /// Turn id (may not have been assigned to a turn yet).
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// Error text.
        message: String,
        /// Whether it is retryable.
        retryable: bool,
    },
}

/// Event receiver (host implementations: Tauri `emit` / CLI printing / test collection).
pub type EventSink = Arc<dyn Fn(AiEvent) + Send + Sync>;

/// Ring event buffer (pull fallback channel).
///
/// Capacity is fixed; the oldest events are dropped once full - so the front-end not polling for a long time will not grow memory without bound.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<VecDeque<AiEvent>>>,
    capacity: usize,
}

impl EventBus {
    /// Create new (default capacity 2048).
    pub fn new() -> Self {
        Self::with_capacity(2048)
    }

    /// Create new with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Record one event.
    pub fn push(&self, event: AiEvent) {
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(event);
    }

    /// Take out and clear all events.
    pub fn drain(&self) -> Vec<AiEvent> {
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        q.drain(..).collect()
    }

    /// Current number of pending events.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Combine into an [`EventSink`]: push and buffer at the same time.
    pub fn sink(&self, push: Option<EventSink>) -> EventSink {
        let bus = self.clone();
        Arc::new(move |event: AiEvent| {
            bus.push(event.clone());
            if let Some(p) = &push {
                p(event);
            }
        })
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_drains_in_order() {
        let bus = EventBus::new();
        bus.push(AiEvent::TurnStarted {
            turn_id: "t1".into(),
        });
        bus.push(AiEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "a".into(),
        });
        assert_eq!(bus.len(), 2);
        let drained = bus.drain();
        assert_eq!(drained.len(), 2);
        assert!(bus.is_empty());
    }

    #[test]
    fn bus_drops_oldest_when_full() {
        let bus = EventBus::with_capacity(2);
        for i in 0..3 {
            bus.push(AiEvent::TextDelta {
                turn_id: "t".into(),
                delta: i.to_string(),
            });
        }
        let drained = bus.drain();
        assert_eq!(drained.len(), 2);
        match &drained[0] {
            AiEvent::TextDelta { delta, .. } => assert_eq!(delta, "1"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn serializes_to_camel_case_wire_format() {
        // Front-end contract: both variant names and field names must be camelCase (a past pitfall: field names kept snake_case)
        let text = serde_json::to_value(AiEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "hi".into(),
        })
        .unwrap();
        assert_eq!(text["type"], "textDelta");
        assert_eq!(text["turnId"], "t1");
        assert_eq!(text["delta"], "hi");

        let result = serde_json::to_value(AiEvent::ToolResult {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            ok: true,
            summary: "ok".into(),
            payload: serde_json::Value::Null,
            elapsed_ms: 12,
        })
        .unwrap();
        assert_eq!(result["type"], "toolResult");
        assert_eq!(result["callId"], "c1");
        assert_eq!(result["elapsedMs"], 12);

        let finished = serde_json::to_value(AiEvent::TurnFinished {
            turn_id: "t1".into(),
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
            truncated: false,
        })
        .unwrap();
        assert_eq!(finished["stopReason"], "endTurn");
        assert_eq!(finished["usage"]["inputTokens"], 0);

        let tool = serde_json::to_value(AiEvent::ToolCall {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            name: "run_request".into(),
            kind: crate::tools::ToolKind::Execute,
            arguments: serde_json::json!({}),
            status: ToolStatus::PendingApproval,
            note: None,
        })
        .unwrap();
        assert_eq!(tool["status"], "pendingApproval");
        assert_eq!(tool["kind"], "execute");
        assert!(
            tool.get("note").is_none(),
            "a None note must not appear in the wire format"
        );

        let plan = AiEvent::PlanReady {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            plan: crate::plan::PlanArtifact::from_args(&serde_json::json!({
                "title": "plan",
                "steps": ["a"]
            }))
            .unwrap()
            .stamped(1, 0, 0),
        };
        let plan = serde_json::to_value(plan).unwrap();
        assert_eq!(plan["type"], "planReady");
        assert_eq!(plan["plan"]["title"], "plan");
        assert_eq!(plan["plan"]["revision"], 1);
    }

    #[test]
    fn sink_forwards_to_push_callback() {
        let bus = EventBus::new();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        let sink = bus.sink(Some(Arc::new(move |e| {
            if let AiEvent::TextDelta { delta, .. } = e {
                seen2.lock().unwrap().push(delta);
            }
        })));
        sink(AiEvent::TextDelta {
            turn_id: "t".into(),
            delta: "x".into(),
        });
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(
            bus.len(),
            1,
            "the push channel must not affect the buffer channel"
        );
    }
}
