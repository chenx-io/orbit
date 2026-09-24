//! Tool contract layer: tool catalog (model-facing JSON Schema) + authorization tiers + host execution interface.
//!
//! Layering:
//! - **This module**: only describes "which tools exist, what their params look like, which tier they belong to, how results are fed back";
//! - **Host implementations** (e.g. Tauri `commands/ai.rs`): actually read/write data and call the execution engine.
//!
//! This keeps `orbit-ai` free of I/O dependencies, so a future native desktop app can reuse the same tool contract.

pub mod catalog;
pub mod validate;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AiResult;
use crate::plan::PlanArtifact;
use crate::proposal::Proposal;

/// Tool authorization tier (basis for mode visibility and confirmation policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    /// Read-only: directly executable in Ask / Plan / Agent modes.
    Read,
    /// Write: Agent mode only, persisted immediately (changes are replayed to the UI as a [`Proposal`]).
    Write,
    /// Execute: Agent mode only, and requires user confirmation **every time** (sends real traffic).
    Execute,
    /// Plan: Plan mode only, no side effects (produces a [`crate::plan::PlanArtifact`]).
    Plan,
}

impl ToolKind {
    /// Whether this tier always requires user confirmation (regardless of mode).
    pub fn always_confirms(self) -> bool {
        matches!(self, ToolKind::Execute)
    }
}

/// Tool declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    /// Tool name (used by the model when calling, snake_case).
    pub name: String,
    /// Authorization tier.
    pub kind: ToolKind,
    /// Model-facing description (English only, per the project convention for AI tooling).
    pub description: String,
    /// Parameter JSON Schema.
    pub parameters: Value,
}

impl ToolSpec {
    /// Convert to one entry of the OpenAI `tools` array.
    pub fn to_openai(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }

    /// Convert to one entry of the Anthropic `tools` array.
    pub fn to_anthropic(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters,
        })
    }
}

/// Tool execution result.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// Whether it succeeded.
    pub ok: bool,
    /// Short conclusion fed back to the model (kept terse to avoid context bloat).
    pub summary: String,
    /// Structured result (for UI card display; may be truncated).
    pub payload: Value,
    /// Changes already persisted by a write op (`None` = pure read or no data change).
    pub proposal: Option<Proposal>,
    /// Plan produced in Plan mode (`None` = not a planning tool).
    pub plan: Option<PlanArtifact>,
}

impl ToolOutcome {
    /// Successful read-only result.
    pub fn read(summary: impl Into<String>, payload: Value) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            payload,
            proposal: None,
            plan: None,
        }
    }

    /// Failure result (fed back to the model as `ok: false` so it can correct itself).
    pub fn failure(summary: impl Into<String>) -> Self {
        Self {
            ok: false,
            summary: summary.into(),
            payload: Value::Null,
            proposal: None,
            plan: None,
        }
    }

    /// Write result: changes are **persisted**; `proposal` is used by the UI to show before/after diffs.
    pub fn proposed(summary: impl Into<String>, proposal: Proposal) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            payload: Value::Null,
            proposal: Some(proposal),
            plan: None,
        }
    }

    /// Planning result: the plan has been persisted with the session.
    pub fn planned(summary: impl Into<String>, plan: PlanArtifact) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            payload: serde_json::to_value(&plan).unwrap_or(Value::Null),
            proposal: None,
            plan: Some(plan),
        }
    }
}

/// Tool execution host (implemented per UI form: Tauri / CLI / future native app).
///
/// Mode (Ask / Agent / Plan) admission is enforced centrally by [`crate::agent::Agent`];
/// the host may assume the invoked tool is allowed in the current mode and just do the job right.
#[async_trait]
pub trait ToolHost: Send + Sync {
    /// Execute one tool call.
    ///
    /// Implementors must:
    /// - return [`crate::error::AiError::UnknownTool`] for unknown tools;
    /// - persist writes directly and include a [`Proposal`] in the result (for UI before/after diffs);
    /// - return [`crate::error::AiError::Invalid`] for bad params, with a message that guides the model to fix it;
    /// - emit [`crate::event::AiEvent::DataChanged`] on the event channel when workspace data is modified.
    async fn call(&self, name: &str, args: &Value) -> AiResult<ToolOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ToolSpec {
        ToolSpec {
            name: "run_request".into(),
            kind: ToolKind::Execute,
            description: "Run a request".into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }
    }

    #[test]
    fn serializes_openai_tool_shape() {
        let v = spec().to_openai();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "run_request");
        assert_eq!(v["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn serializes_anthropic_tool_shape() {
        let v = spec().to_anthropic();
        assert_eq!(v["name"], "run_request");
        assert!(v.get("input_schema").is_some());
    }

    #[test]
    fn execute_kind_always_confirms() {
        assert!(ToolKind::Execute.always_confirms());
        assert!(!ToolKind::Read.always_confirms());
        assert!(!ToolKind::Write.always_confirms());
        assert!(
            !ToolKind::Plan.always_confirms(),
            "planning has no side effects, no confirmation needed"
        );
    }

    #[test]
    fn planned_outcome_carries_plan_and_payload() {
        let plan = crate::plan::PlanArtifact::from_args(&serde_json::json!({
            "title": "t",
            "steps": ["a"]
        }))
        .unwrap()
        .stamped(1, 0, 7);
        let outcome = ToolOutcome::planned("submitted", plan.clone());
        assert!(outcome.ok);
        assert_eq!(outcome.plan, Some(plan.clone()));
        assert_eq!(outcome.payload["title"], "t");
        assert!(outcome.proposal.is_none());
    }

    #[test]
    fn read_outcome_has_no_side_effect_payloads() {
        let outcome = ToolOutcome::read("ok", serde_json::json!({}));
        assert!(outcome.plan.is_none());
        assert!(outcome.proposal.is_none());
    }
}
