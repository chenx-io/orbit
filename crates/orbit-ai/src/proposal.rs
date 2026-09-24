//! Write-operation proposals: the AI does not write to the store directly but produces "changes pending confirmation", which the front-end shows as a diff before applying.
//!
//! The carrier of tiered authorization: Write-class tools produce [`Proposal`]; Execute-class tools, on the host side,
//! wait for the user to click confirm before actually sending the network request.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Single-field diff kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    /// Field added.
    Added,
    /// Field removed.
    Removed,
    /// Field changed.
    Changed,
}

/// One field-level diff (for the front-end two-column comparison view).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    /// Field path (dot-separated, e.g. `headers.auth.token`).
    pub path: String,
    /// Before the change (`None` when added).
    pub before: Option<Value>,
    /// After the change (`None` when removed).
    pub after: Option<Value>,
    /// Diff kind.
    pub kind: DiffKind,
}

/// The action a proposal performs (the host uses it to call the corresponding `DataService` write method).
///
/// Same as [`crate::event::AiEvent`]: fields inside struct variants need `rename_all_fields`
/// to be converted to camelCase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProposalAction {
    /// Create a request in a collection (optionally attached to a folder/collection root).
    CreateRequest {
        /// Target collection id.
        collection_id: String,
        /// Target parent-node id (`None` = collection root).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<String>,
    },
    /// Update an existing request.
    UpdateRequest {
        /// Target request id.
        request_id: String,
    },
    /// Create a collection.
    CreateCollection,
    /// Create a scenario folder.
    CreateScenarioFolder,
    /// Create an automation scenario.
    CreateScenario {
        /// Owning folder (`None` = root).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folder_id: Option<String>,
    },
    /// Update an existing scenario.
    UpdateScenario {
        /// Target scenario id.
        scenario_id: String,
    },
    /// Create a test suite.
    CreateSuite,
    /// Create a CSV dataset.
    CreateDataSet,
    /// Add / update an action-library entry (`template_id` is `None` = create new).
    SaveActionTemplate {
        /// Target entry id (`None` = create new).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template_id: Option<String>,
    },
    /// Delete an action-library entry.
    ///
    /// References to it in requests are **not** cleaned up and become dangling (an error is logged at run time but the request is not aborted),
    /// so the impact must be explained to the user before deleting.
    DeleteActionTemplate {
        /// Target entry id.
        template_id: String,
    },
}

/// A proposal pending confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    /// Proposal id (passed back when applying/rejecting).
    pub id: String,
    /// Name of the tool that produced this proposal.
    pub tool: String,
    /// User-facing title (e.g. "New request: Login").
    pub title: String,
    /// Target entity description (e.g. "Collection User Center / root").
    pub target: String,
    /// Action.
    pub action: ProposalAction,
    /// Full object before the change (`None` when created).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// Full object after the change (written when applied).
    pub after: Value,
    /// Field-level diffs (all `Added` when created).
    pub diffs: Vec<FieldDiff>,
    /// Created time (Unix milliseconds).
    pub created_at: i64,
}

impl Proposal {
    /// Build a proposal and compute the field diffs automatically.
    pub fn new(
        tool: impl Into<String>,
        title: impl Into<String>,
        target: impl Into<String>,
        action: ProposalAction,
        before: Option<Value>,
        after: Value,
    ) -> Self {
        let diffs = match &before {
            Some(b) => diff_values(b, &after),
            None => collect_all_paths(&after)
                .into_iter()
                .map(|(path, value)| FieldDiff {
                    path,
                    before: None,
                    after: Some(value),
                    kind: DiffKind::Added,
                })
                .collect(),
        };
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            title: title.into(),
            target: target.into(),
            action,
            before,
            after,
            diffs,
            created_at: jiff::Timestamp::now().as_millisecond(),
        }
    }
}

/// Recursively compute the field-level diff between two JSON values (ignoring differences in key order only).
pub fn diff_values(before: &Value, after: &Value) -> Vec<FieldDiff> {
    let mut out = Vec::new();
    walk("", Some(before), Some(after), &mut out);
    out
}

fn walk(path: &str, before: Option<&Value>, after: Option<&Value>, out: &mut Vec<FieldDiff>) {
    match (before, after) {
        (None, None) => {}
        (None, Some(a)) => out.push(FieldDiff {
            path: path.to_string(),
            before: None,
            after: Some(a.clone()),
            kind: DiffKind::Added,
        }),
        (Some(b), None) => out.push(FieldDiff {
            path: path.to_string(),
            before: Some(b.clone()),
            after: None,
            kind: DiffKind::Removed,
        }),
        (Some(b), Some(a)) => {
            if b == a {
                return;
            }
            match (b, a) {
                (Value::Object(bo), Value::Object(ao)) => {
                    let mut keys: Vec<&String> = bo.keys().chain(ao.keys()).collect();
                    keys.sort();
                    keys.dedup();
                    for key in keys {
                        let child = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        };
                        walk(&child, bo.get(key), ao.get(key), out);
                    }
                }
                (Value::Array(ba), Value::Array(aa)) => {
                    // Arrays are compared **expanded by index**: for action lists (preActions/postActions) it must be possible to locate
                    // "which field of which action"; a wholesale replacement would leave the change card with a single line, "preActions changed",
                    // and the user could not audit which step the AI actually touched. The same applies, and reads more clearly, for headers/assertions.
                    let common = ba.len().min(aa.len());
                    for i in 0..common {
                        walk(&format!("{path}[{i}]"), Some(&ba[i]), Some(&aa[i]), out);
                    }
                    for (i, b) in ba.iter().enumerate().skip(common) {
                        walk(&format!("{path}[{i}]"), Some(b), None, out);
                    }
                    for (i, a) in aa.iter().enumerate().skip(common) {
                        walk(&format!("{path}[{i}]"), None, Some(a), out);
                    }
                }
                _ => out.push(FieldDiff {
                    path: path.to_string(),
                    before: Some(b.clone()),
                    after: Some(a.clone()),
                    kind: DiffKind::Changed,
                }),
            }
        }
    }
}

/// Collect all leaf paths of a JSON value (for the "everything added" diff of creation-style proposals).
fn collect_all_paths(value: &Value) -> Vec<(String, Value)> {
    fn rec(path: &str, value: &Value, out: &mut Vec<(String, Value)>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let child = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    rec(&child, v, out);
                }
            }
            // Arrays are likewise expanded by index: the path convention for creation-style proposals must match diff_values
            // (otherwise the same edit is shown at different granularities in creation vs. modification proposals).
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    rec(&format!("{path}[{i}]"), item, out);
                }
            }
            other => out.push((path.to_string(), other.clone())),
        }
    }
    let mut out = Vec::new();
    rec("", value, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_changed_scalar_field() {
        let diffs = diff_values(&json!({"method":"GET"}), &json!({"method":"POST"}));
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "method");
        assert_eq!(diffs[0].kind, DiffKind::Changed);
        assert_eq!(diffs[0].after, Some(json!("POST")));
    }

    #[test]
    fn detects_nested_added_and_removed_fields() {
        let diffs = diff_values(
            &json!({"auth":{"type":"none","token":"old"}}),
            &json!({"auth":{"type":"bearer"}}),
        );
        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"auth.type"));
        assert!(paths.contains(&"auth.token"));
        let token = diffs.iter().find(|d| d.path == "auth.token").unwrap();
        assert_eq!(token.kind, DiffKind::Removed);
        let t = diffs.iter().find(|d| d.path == "auth.type").unwrap();
        assert_eq!(t.kind, DiffKind::Changed);
    }

    /// Arrays are expanded by index: a change to an action list must be locatable to "which field of which action".
    #[test]
    fn indexes_array_elements_in_diff() {
        let diffs = diff_values(
            &json!({"headers":[{"key":"A","value":"1"}]}),
            &json!({"headers":[{"key":"A","value":"1"},{"key":"B","value":"2"}]}),
        );
        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["headers[1]"],
            "added items expand by index: {paths:?}"
        );
        assert_eq!(diffs[0].kind, DiffKind::Added);
    }

    #[test]
    fn locates_changed_field_inside_action_list() {
        let diffs = diff_values(
            &json!({"preActions":[{"type":"interpolate"},{"type":"db","sql":"SELECT 1"}]}),
            &json!({"preActions":[{"type":"interpolate"},{"type":"db","sql":"SELECT 2"}]}),
        );
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "preActions[1].sql");
        assert_eq!(diffs[0].before, Some(json!("SELECT 1")));
        assert_eq!(diffs[0].after, Some(json!("SELECT 2")));
    }

    #[test]
    fn detects_removed_action_and_untouched_anchor() {
        let diffs = diff_values(
            &json!({"preActions":[{"type":"interpolate"},{"type":"script","code":"a()"}]}),
            &json!({"preActions":[{"type":"interpolate"}]}),
        );
        assert_eq!(
            diffs.len(),
            1,
            "only the deleted action should be reported: {diffs:?}"
        );
        assert_eq!(diffs[0].path, "preActions[1]");
        assert_eq!(diffs[0].kind, DiffKind::Removed);
    }

    #[test]
    fn identical_values_produce_no_diff() {
        assert!(diff_values(&json!({"a":1}), &json!({"a":1})).is_empty());
    }

    #[test]
    fn new_proposal_lists_all_leaves_as_added() {
        let p = Proposal::new(
            "create_request",
            "Create request: Login",
            "Collection User Center",
            ProposalAction::CreateRequest {
                collection_id: "c1".into(),
                parent_id: None,
            },
            None,
            json!({"name":"Login","method":"POST","auth":{"type":"none"}}),
        );
        assert!(p.diffs.iter().all(|d| d.kind == DiffKind::Added));
        let paths: Vec<&str> = p.diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"name"));
        assert!(paths.contains(&"auth.type"));
        assert!(!p.id.is_empty());
    }

    #[test]
    fn proposal_action_serializes_camel_case_fields() {
        let action = serde_json::to_value(ProposalAction::CreateRequest {
            collection_id: "c1".into(),
            parent_id: Some("f1".into()),
        })
        .unwrap();
        assert_eq!(action["type"], "createRequest");
        assert_eq!(action["collectionId"], "c1");
        assert_eq!(action["parentId"], "f1");

        let action = serde_json::to_value(ProposalAction::UpdateScenario {
            scenario_id: "s1".into(),
        })
        .unwrap();
        assert_eq!(action["type"], "updateScenario");
        assert_eq!(action["scenarioId"], "s1");
    }

    /// Entry-level proposals: no id on creation (the front-end groups by `type`), id included on deletion.
    #[test]
    fn action_template_proposals_serialize_camel_case() {
        let created =
            serde_json::to_value(ProposalAction::SaveActionTemplate { template_id: None }).unwrap();
        assert_eq!(created["type"], "saveActionTemplate");
        assert!(
            created.get("templateId").is_none(),
            "a creation must not carry an id"
        );

        let updated = serde_json::to_value(ProposalAction::SaveActionTemplate {
            template_id: Some("tpl-1".into()),
        })
        .unwrap();
        assert_eq!(updated["templateId"], "tpl-1");

        let removed = serde_json::to_value(ProposalAction::DeleteActionTemplate {
            template_id: "tpl-1".into(),
        })
        .unwrap();
        assert_eq!(removed["type"], "deleteActionTemplate");
        assert_eq!(removed["templateId"], "tpl-1");
    }

    /// Deletion-style proposals use `after: {}` to express "the whole thing is gone": each field reports a Removed entry,
    /// so the change card can tell the user field by field what was lost (instead of a vague single line "changed").
    #[test]
    fn delete_proposal_reports_every_field_as_removed() {
        let p = Proposal::new(
            "delete_action_template",
            "Delete library item: Compute signature",
            "Script library",
            ProposalAction::DeleteActionTemplate {
                template_id: "tpl-1".into(),
            },
            Some(
                json!({"id":"tpl-1","name":"Compute signature","action":{"type":"script","code":"sign()"}}),
            ),
            json!({}),
        );
        assert!(!p.diffs.is_empty());
        assert!(p.diffs.iter().all(|d| d.kind == DiffKind::Removed));
        let paths: Vec<&str> = p.diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"name"));
        // When a whole subtree is deleted it is reported at the parent path (not expanded leaf by leaf, to avoid drowning the change card in noise)
        assert!(paths.contains(&"action"));
    }

    #[test]
    fn proposal_serializes_camel_case_wire_format() {
        let p = Proposal::new(
            "create_request",
            "Create request",
            "Collection A",
            ProposalAction::CreateCollection,
            None,
            json!({"name":"x"}),
        );
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["tool"], "create_request");
        assert!(v["createdAt"].is_number());
        assert!(v["diffs"].is_array());
        assert!(
            v.get("before").is_none(),
            "a creation proposal must not carry before"
        );
    }

    #[test]
    fn update_proposal_carries_before_for_diff() {
        let p = Proposal::new(
            "update_request",
            "Update request",
            "Collection/Login",
            ProposalAction::UpdateRequest {
                request_id: "r1".into(),
            },
            Some(json!({"url":"https://a.com"})),
            json!({"url":"https://b.com"}),
        );
        assert_eq!(p.diffs.len(), 1);
        assert_eq!(p.diffs[0].before, Some(json!("https://a.com")));
    }
}
