//! Reusable action templates ("script library" entries).
//!
//! Maintained in one place, referenced by many endpoints: request action lists reference an entry id via [`RequestAction::Ref`],
//! and before execution / export [`resolve_action_refs`] expands it into the entry's current concrete action.
//!
//! - An entry may only contain a **script** or a **database action** (the built-in interpolate node and nested references are not allowed);
//! - An entry **does not declare when it runs**: once the reference is placed in an action list the caller orders it, and the action's **actual position** in the list
//!   is the final authority on when it runs (consistent with the built-in interpolate node model);
//! - Entries **carry no workspace ownership**: workspace isolation is handled by `orbit-data`'s wrapper structs,
//!   so the engine layer need not depend on a persistence crate when receiving the library table (same approach as [`crate::DataSourceConfig`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::action::RequestAction;

/// A reusable action template (script library entry).
///
/// `name` is the entry's **sole display name**: on expansion it (or the reference's alias) overrides the inner action's `name`,
/// so the entry content does not need to repeat a name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTemplate {
    /// Library entry id (stable identifier that references look up)
    pub id: String,
    /// Library entry name (display name)
    pub name: String,
    /// Description (display only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The concrete action (only script or database actions; see [`ActionTemplate::is_valid`])
    pub action: RequestAction,
    /// Sort index (list order in the management UI; deterministic sorting helps future projections)
    #[serde(default)]
    pub sort_index: i32,
}

impl ActionTemplate {
    /// Create an entry (empty description, sort index 0)
    pub fn new(id: impl Into<String>, name: impl Into<String>, action: RequestAction) -> Self {
        ActionTemplate {
            id: id.into(),
            name: name.into(),
            description: None,
            action,
            sort_index: 0,
        }
    }

    /// Whether the entry content is executable: only script / database actions are accepted.
    ///
    /// Neither the built-in interpolate node (position marker) nor nested references are allowed - the former would pollute the anchor uniqueness of action lists,
    /// and the latter would make resolution recursive.
    pub fn is_valid(&self) -> bool {
        matches!(
            self.action,
            RequestAction::Script { .. } | RequestAction::Db { .. }
        )
    }
}

/// Expand script library references in an action list **without changing the position or count of any action**.
///
/// - Hit with a valid entry -> replaced in place with the entry's current content (position unchanged);
///   the display name is "reference alias -> entry name" and the enabled state is "reference enabled AND entry enabled"
///   (disabling an entry disables it globally);
/// - Dangling (entry missing) or invalid entry content -> the **reference is kept as-is**, and the execution layer logs one
///   error-level log without aborting the request - silent dropping is forbidden, and so is failing the request as a whole;
/// - Non-reference actions are returned unchanged.
///
/// Callers should call it **before enabled-filtering**: a disabled entry only shows up in the expanded actions,
/// so filtering first and expanding later would miss the "entry disabled" case.
pub fn resolve_action_refs(
    actions: &[RequestAction],
    library: &[ActionTemplate],
) -> Vec<RequestAction> {
    if !actions.iter().any(RequestAction::is_ref) {
        return actions.to_vec();
    }

    // Build the id table once, then replace linearly (O(number of actions))
    let table: HashMap<&str, &ActionTemplate> = library
        .iter()
        .filter(|t| t.is_valid())
        .map(|t| (t.id.as_str(), t))
        .collect();

    actions
        .iter()
        .map(|action| {
            let RequestAction::Ref {
                library_id,
                name,
                enabled,
            } = action
            else {
                return action.clone();
            };
            let Some(tpl) = table.get(library_id.as_str()) else {
                return action.clone();
            };
            let mut resolved = tpl.action.clone();
            match name.as_deref() {
                Some(alias) if !alias.trim().is_empty() => resolved.set_name(alias.to_string()),
                _ => resolved.set_name(tpl.name.clone()),
            }
            resolved.set_enabled(*enabled && tpl.action.is_enabled());
            resolved
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequestAction;

    fn tpl(id: &str, name: &str, action: RequestAction) -> ActionTemplate {
        ActionTemplate::new(id, name, action)
    }

    fn script_action(code: &str) -> RequestAction {
        RequestAction::from_script(code)
    }

    #[test]
    fn template_roundtrip_has_no_execution_stage() {
        let t = tpl("t1", "signing", script_action("sign();"));
        let s = serde_yaml::to_string(&t).unwrap();
        // An entry does not declare when it runs: the caller orders it after the reference is inserted
        assert!(
            !s.contains("stage"),
            "a library entry must not carry a stage field any more: {s}"
        );
        let back: ActionTemplate = serde_yaml::from_str(&s).unwrap();
        assert_eq!(back.name, "signing");
        assert_eq!(back.id, "t1");
        assert_eq!(back.description, None);
        assert_eq!(back.sort_index, 0);
        assert!(back.is_valid());
    }

    #[test]
    fn template_validation_rejects_interpolate_and_nested_ref() {
        let bad = tpl("t1", "anchor", RequestAction::Interpolate);
        assert!(
            !bad.is_valid(),
            "the built-in interpolate node cannot be a library entry"
        );
        let bad2 = tpl("t2", "nested", RequestAction::from_ref("t1"));
        assert!(
            !bad2.is_valid(),
            "library entries must not contain nested references"
        );
        let ok = tpl("t3", "signing", script_action("sign();"));
        assert!(ok.is_valid());
    }

    #[test]
    fn resolve_expands_reference_in_place_with_library_name() {
        let lib = vec![tpl("t-sign", "compute signature", script_action("sign();"))];
        let actions = vec![
            RequestAction::from_script("a();"),
            RequestAction::from_ref("t-sign"),
            RequestAction::from_script("b();"),
        ];
        let out = resolve_action_refs(&actions, &lib);
        assert_eq!(
            out.len(),
            3,
            "resolution must not add or remove actions: {out:?}"
        );
        // Position unchanged
        assert_eq!(out[0].script_code(), Some("a();"));
        assert_eq!(out[2].script_code(), Some("b();"));
        // Replaced in place with the entry content; the display name comes from the entry name
        assert_eq!(out[1].name(), "compute signature");
        assert_eq!(out[1].script_code(), Some("sign();"));
        assert!(!out[1].is_ref());
    }

    #[test]
    fn resolve_applies_alias_and_enabled_overrides() {
        // Entry disabled -> every reference is skipped (disabling an entry disables it globally)
        let mut disabled_tpl = tpl("t-off", "disabled entry", script_action("x();"));
        disabled_tpl.action.set_enabled(false);
        let out = resolve_action_refs(&[RequestAction::from_ref("t-off")], &[disabled_tpl]);
        assert!(!out[0].is_enabled());

        // The reference has a custom display name (the alias wins over the entry name)
        let lib = vec![tpl("t-1", "entry name", script_action("y();"))];
        let mut aliased = RequestAction::from_ref("t-1");
        aliased.set_name("endpoint-specific name".into());
        let out = resolve_action_refs(&[aliased], &lib);
        assert_eq!(out[0].name(), "endpoint-specific name");
        assert_eq!(out[0].script_code(), Some("y();"));

        // The reference itself is disabled
        let mut off = RequestAction::from_ref("t-1");
        off.set_enabled(false);
        let out = resolve_action_refs(&[off], &lib);
        assert!(!out[0].is_enabled());
    }

    #[test]
    fn resolve_keeps_dangling_and_invalid_refs_untouched() {
        // Dangling references (entry missing) and invalid entries (anchor / nested reference) are kept as-is,
        // leaving the execution layer to log an error - never silently dropped.
        let lib = vec![
            tpl("t-anchor", "invalid", RequestAction::Interpolate),
            tpl("t-nested", "invalid", RequestAction::from_ref("t-1")),
        ];
        let actions = vec![
            RequestAction::from_ref("t-missing"),
            RequestAction::from_ref("t-anchor"),
            RequestAction::from_ref("t-nested"),
        ];
        let out = resolve_action_refs(&actions, &lib);
        assert_eq!(out.len(), 3);
        assert!(
            out.iter().all(RequestAction::is_ref),
            "dangling / invalid references must be kept as-is: {out:?}"
        );
    }

    #[test]
    fn resolve_is_noop_without_library() {
        let actions = vec![
            RequestAction::from_script("a();"),
            RequestAction::from_ref("t-1"),
        ];
        let out = resolve_action_refs(&actions, &[]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].script_code(), Some("a();"));
        assert!(out[1].is_ref());
    }
}
