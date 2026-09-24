//! Plan artifact: the structured output of Plan mode (for the front-end to render and hand over to Agent for implementation with one click).
//!
//! Why not "let the model output a Markdown plan": a plan must **survive in the session over time** (the user may revise it over
//! several turns), and the UI must offer an explicit "start implementing" entry point. The structured [`PlanArtifact`],
//! produced by the `present_plan` tool and persisted with the session, is far more stable than parsing Markdown out of the body text.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AiError, AiResult};

/// One step in a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// Step title (one sentence saying what to do).
    pub title: String,
    /// Extra detail (optional): which requests/scenarios are involved and which parameters to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A plan (may be revised over multiple turns; `revision` increases).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanArtifact {
    /// Plan title.
    pub title: String,
    /// Overall description (optional).
    #[serde(default)]
    pub summary: String,
    /// Steps (in execution order).
    pub steps: Vec<PlanStep>,
    /// Risks / caveats (optional).
    #[serde(default)]
    pub notes: Vec<String>,
    /// Revision number (first version = 1, +1 on each resubmission).
    #[serde(default)]
    pub revision: u32,
    /// Created time (Unix milliseconds).
    #[serde(default)]
    pub created_at: i64,
    /// Last modified time (Unix milliseconds).
    #[serde(default)]
    pub updated_at: i64,
}

/// Upper bound on plan steps: exceeding it means the model is writing implementation details rather than "planning".
pub const MAX_PLAN_STEPS: usize = 30;

impl PlanArtifact {
    /// Parse and validate from the `present_plan` tool arguments.
    ///
    /// A validation failure is fed back to the model as a tool failure for a retry (matching the write-operation validation policy),
    /// so error messages must be **actionable**: state which field is invalid and how to fix it.
    pub fn from_args(args: &Value) -> AiResult<Self> {
        let obj = args
            .as_object()
            .ok_or_else(|| AiError::Invalid("present_plan arguments must be an object".into()))?;
        let title = obj
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AiError::Invalid("present_plan is missing title (plan title)".into()))?;
        let raw_steps = obj
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::Invalid("present_plan is missing the steps array".into()))?;
        if raw_steps.is_empty() {
            return Err(AiError::Invalid(
                "present_plan steps must not be empty: provide at least one concrete implementation action".into(),
            ));
        }
        if raw_steps.len() > MAX_PLAN_STEPS {
            return Err(AiError::Invalid(format!(
                "present_plan has too many steps ({} steps, limit {MAX_PLAN_STEPS}): merge similar steps and keep only the key actions",
                raw_steps.len()
            )));
        }
        let mut steps = Vec::with_capacity(raw_steps.len());
        for (idx, raw) in raw_steps.iter().enumerate() {
            match raw {
                // Allow the plain-string shorthand: the model occasionally passes ["step one", "step two"]
                Value::String(s) if !s.trim().is_empty() => steps.push(PlanStep {
                    title: s.trim().to_string(),
                    detail: None,
                }),
                Value::Object(map) => {
                    let title = map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            AiError::Invalid(format!("present_plan step {} is missing title", idx + 1))
                        })?;
                    let detail = map
                        .get("detail")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    steps.push(PlanStep {
                        title: title.to_string(),
                        detail,
                    });
                }
                _ => {
                    return Err(AiError::Invalid(format!(
                        "present_plan step {} is malformed: expected {{\"title\":\"...\",\"detail\":\"...\"}} or a string",
                        idx + 1
                    )))
                }
            }
        }
        let summary = obj
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let notes = obj
            .get("notes")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Self {
            title: title.to_string(),
            summary,
            steps,
            notes,
            revision: 0,
            created_at: 0,
            updated_at: 0,
        })
    }

    /// Timestamps: the first version sets the created time; a revision keeps the created time and refreshes the modified time.
    pub fn stamped(mut self, revision: u32, created_at: i64, now: i64) -> Self {
        self.revision = revision.max(1);
        self.created_at = if created_at > 0 { created_at } else { now };
        self.updated_at = now;
        self
    }

    /// One-sentence summary for the model to read (fed back into the tool result so it does not resubmit the same plan).
    pub fn digest(&self) -> String {
        format!(
            "Plan \"{}\" revision {} recorded, {} steps: {}",
            self.title,
            self.revision,
            self.steps.len(),
            self.steps
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>()
                .join("；")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args() -> Value {
        json!({
            "title": "Add automated cases for the login request",
            "summary": "Cover both the success and failure paths",
            "steps": [
                { "title": "Read the login request definition", "detail": "get_request r1" },
                { "title": "Create case: login failure path" },
            ],
            "notes": ["Check that the test environment is available first"],
        })
    }

    #[test]
    fn parses_valid_plan() {
        let plan = PlanArtifact::from_args(&args()).unwrap();
        assert_eq!(plan.title, "Add automated cases for the login request");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[1].detail, None);
        assert_eq!(plan.notes.len(), 1);
    }

    #[test]
    fn accepts_plain_string_steps() {
        let plan = PlanArtifact::from_args(&json!({
            "title": "t",
            "steps": ["step one", "step two"]
        }))
        .unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].title, "step one");
    }

    #[test]
    fn rejects_empty_or_missing_title() {
        assert!(PlanArtifact::from_args(&json!({"title": "  ", "steps": ["a"]})).is_err());
        assert!(PlanArtifact::from_args(&json!({"steps": ["a"]})).is_err());
    }

    #[test]
    fn rejects_empty_steps_with_actionable_message() {
        let err = PlanArtifact::from_args(&json!({"title": "t", "steps": []})).unwrap_err();
        assert!(
            err.user_message().contains("at least one"),
            "{}",
            err.user_message()
        );
    }

    #[test]
    fn rejects_too_many_steps() {
        let steps: Vec<String> = (0..MAX_PLAN_STEPS + 1).map(|i| format!("s{i}")).collect();
        let err = PlanArtifact::from_args(&json!({"title": "t", "steps": steps})).unwrap_err();
        assert!(
            err.user_message().contains("limit"),
            "{}",
            err.user_message()
        );
    }

    #[test]
    fn rejects_step_without_title() {
        let err = PlanArtifact::from_args(&json!({"title": "t", "steps": [{"detail": "x"}]}))
            .unwrap_err();
        assert!(
            err.user_message().contains("missing title"),
            "{}",
            err.user_message()
        );
    }

    #[test]
    fn stamping_keeps_creation_time_across_revisions() {
        let plan = PlanArtifact::from_args(&args()).unwrap();
        let v1 = plan.clone().stamped(1, 0, 1_000);
        assert_eq!(v1.revision, 1);
        assert_eq!(v1.created_at, 1_000);
        let v2 = plan.clone().stamped(2, v1.created_at, 2_000);
        assert_eq!(v2.revision, 2);
        assert_eq!(
            v2.created_at, 1_000,
            "a revision must not rewrite the created time"
        );
        assert_eq!(v2.updated_at, 2_000);
    }

    #[test]
    fn digest_lists_step_titles() {
        let plan = PlanArtifact::from_args(&args()).unwrap().stamped(3, 0, 1);
        let digest = plan.digest();
        assert!(digest.contains("revision 3"));
        assert!(digest.contains("Read the login request definition"));
        assert!(digest.contains("Create case: login failure path"));
    }

    #[test]
    fn serializes_camel_case() {
        let plan = PlanArtifact::from_args(&args()).unwrap().stamped(1, 5, 9);
        let v = serde_json::to_value(&plan).unwrap();
        assert_eq!(v["createdAt"], 5);
        assert_eq!(v["updatedAt"], 9);
        assert_eq!(v["steps"][0]["detail"], "get_request r1");
        assert!(
            v["steps"][1].get("detail").is_none(),
            "an empty detail must not appear in the wire format"
        );
    }
}
