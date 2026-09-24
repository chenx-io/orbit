//! Work modes: Ask / Agent / Plan (aligned with the mental model of mainstream agent tools).
//!
//! Modes decide three things, and **all of them are enforced at the engine layer** (not just a gentlemen's agreement inside the prompt):
//! 1. **Tool visibility**: the tool list sent to the model is filtered by mode (Ask/Plan never even see write and execute tools);
//! 2. **Execution gate**: even if the model invents a filtered-out tool name, it is denied and the error is fed back;
//! 3. **Whether writes need confirmation**: only Agent mode can write, and it writes straight to the store (writes are undoable: the snapshot is revertible).
//!
//! ```text
//! Mode   Read tools  Write tools    Execute tools        Plan tools
//! Ask     ✅       ❌            ❌                 ❌
//! Plan    ✅       ❌            ❌                 ✅
//! Agent   ✅      ✅ direct write   ✅ confirm each time   ❌
//! ```
//!
//! Execution tools (run_request / run_scenario / run_load_test) must always be confirmed one by one in every mode:
//! writes only change a local, revertible snapshot, whereas execution sends real requests to **external systems** (load tests even generate load),
//! which is irreversible; the two risks are not symmetric.

use serde::{Deserialize, Serialize};

use crate::tools::ToolKind;

/// Work mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMode {
    /// Read-only chat: look things up, explain endpoints, analyze failures; changes no data.
    Ask,
    /// Full capability: can read and write API assets and drive the execution engine (execution still needs confirmation).
    Agent,
    /// Plan: read-only investigation + an actionable plan, implemented via [`AiMode::Agent`] once the user confirms.
    Plan,
}

impl Default for AiMode {
    /// Defaults to Agent: consistent with mainstream tools (open it and get to work).
    fn default() -> Self {
        AiMode::Agent
    }
}

impl AiMode {
    /// Wire-format tag (matches the frontend `AiMode` literals).
    pub fn as_tag(self) -> &'static str {
        match self {
            AiMode::Ask => "ask",
            AiMode::Agent => "agent",
            AiMode::Plan => "plan",
        }
    }

    /// Parse a tag; unknown values fall back to the default mode (a string in a snapshot may come from an older version).
    pub fn from_tag(tag: &str) -> Self {
        match tag.trim().to_ascii_lowercase().as_str() {
            "ask" => AiMode::Ask,
            "plan" => AiMode::Plan,
            _ => AiMode::Agent,
        }
    }

    /// Whether the mode is read-only (Ask / Plan). In read-only modes write tools are neither visible nor executable.
    pub fn is_read_only(self) -> bool {
        !matches!(self, AiMode::Agent)
    }

    /// Whether this mode allows a given class of tools.
    ///
    /// Note: Agent allows [`ToolKind::Execute`], but the user must still confirm each execution
    /// (enforced by the agent loop's [`crate::agent::Approver`], unrelated to this function);
    /// planning tools are exposed only in Plan mode - once you have decided to act, you should not go back to producing a plan.
    pub fn allows(self, kind: ToolKind) -> bool {
        match self {
            AiMode::Ask => kind == ToolKind::Read,
            AiMode::Plan => matches!(kind, ToolKind::Read | ToolKind::Plan),
            AiMode::Agent => kind != ToolKind::Plan,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_agent() {
        assert_eq!(AiMode::default(), AiMode::Agent);
    }

    #[test]
    fn tag_round_trip() {
        for mode in [AiMode::Ask, AiMode::Agent, AiMode::Plan] {
            assert_eq!(AiMode::from_tag(mode.as_tag()), mode);
        }
        // The wire format must be the lowercase literal the frontend knows
        assert_eq!(serde_json::to_value(AiMode::Plan).unwrap(), "plan");
    }

    #[test]
    fn unknown_tag_falls_back_to_agent() {
        assert_eq!(AiMode::from_tag(""), AiMode::Agent);
        assert_eq!(AiMode::from_tag("ADMIN"), AiMode::Agent);
        assert_eq!(AiMode::from_tag(" Agent "), AiMode::Agent);
    }

    #[test]
    fn ask_can_only_read() {
        assert!(AiMode::Ask.allows(ToolKind::Read));
        assert!(!AiMode::Ask.allows(ToolKind::Write));
        assert!(!AiMode::Ask.allows(ToolKind::Execute));
        assert!(!AiMode::Ask.allows(ToolKind::Plan));
        assert!(AiMode::Ask.is_read_only());
    }

    #[test]
    fn plan_reads_and_plans_but_never_writes_or_runs() {
        assert!(AiMode::Plan.allows(ToolKind::Read));
        assert!(AiMode::Plan.allows(ToolKind::Plan));
        assert!(!AiMode::Plan.allows(ToolKind::Write));
        assert!(
            !AiMode::Plan.allows(ToolKind::Execute),
            "the planning phase must not generate real traffic"
        );
        assert!(AiMode::Plan.is_read_only());
    }

    #[test]
    fn agent_has_full_toolbox_but_planning_is_plan_only() {
        assert!(AiMode::Agent.allows(ToolKind::Read));
        assert!(AiMode::Agent.allows(ToolKind::Write));
        assert!(AiMode::Agent.allows(ToolKind::Execute));
        assert!(
            !AiMode::Agent.allows(ToolKind::Plan),
            "plans are produced only in Plan mode"
        );
        assert!(!AiMode::Agent.is_read_only());
    }

    #[test]
    fn every_kind_has_at_least_one_mode_that_allows_it() {
        for kind in [
            ToolKind::Read,
            ToolKind::Write,
            ToolKind::Execute,
            ToolKind::Plan,
        ] {
            assert!(
                [AiMode::Ask, AiMode::Agent, AiMode::Plan]
                    .into_iter()
                    .any(|m| m.allows(kind)),
                "{kind:?} is unavailable in every mode, which means the mode table has a gap"
            );
        }
    }
}
