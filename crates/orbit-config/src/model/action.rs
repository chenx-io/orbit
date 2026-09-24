//! Pre/post request "action" model - a single action in the ordered action list.
//!
//! Upgraded from the old "single JS script string": an action can be a JS script or a
//! **read-only** database query (relational SQL / read-only Redis command) whose result can be written into variables,
//! for later script actions or subsequent requests to reference.
//!
//! YAML shape (discriminated by the `type` tag):
//! ```yaml
//! pre_actions:
//!   - type: db
//!     name: "fetch user info"
//!     datasource: "user-db"
//!     sql: "SELECT id, mobile FROM users WHERE id = '{{user_id}}'"
//!     columns:
//!       - { column: id, var: dbUserId }
//!       - { column: mobile, var: mobile }
//!   # Built-in "interpolate" node: turns the request template into the final message (variable interpolation + body encoding/assembly).
//!   # Before it = pre-interpolation (may write variables consumed by interpolation and rewrite the template); after it = post-interpolation (rewrites are the final bytes).
//!   - type: interpolate
//!   - type: script
//!     name: "rewrite request headers"
//!     code: "pm.request.headers.upsert({ key: 'X-Uid', value: pm.variables.get('dbUserId') });"
//!   # Reference to a "script library" entry: expanded to the entry's current content before execution (editing the entry takes effect at every reference).
//!   # name is an optional display alias; when the entry is deleted the reference is kept as-is and the execution layer logs an error (the request is not aborted).
//!   - type: ref
//!     library_id: "tpl-sign"
//! ```

use serde::{Deserialize, Serialize};

use super::check::{DbTarget, RetryPolicy};

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// Multi-column mapping entry: writes column `column` of a query result row into variable `var`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnVar {
    /// Result set column name
    pub column: String,
    /// Variable to write into
    pub var: String,
}

/// Pre/post request action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestAction {
    /// JS script action (can rewrite the request / read the response / write variables)
    Script {
        /// Custom name (display only)
        #[serde(default, skip_serializing_if = "String::is_empty")]
        name: String,
        /// Whether enabled (skipped when disabled)
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
        /// Script language (only `js` is supported for now; reserved field)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// Script source code
        code: String,
    },
    /// Read-only database action: query the database and write the result into variables
    Db {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        name: String,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
        /// Datasource id / name (configured in the global datasource module)
        datasource: String,
        /// Read-only relational SQL (supports `${var}` / `{{var}}` interpolation)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sql: Option<String>,
        /// Read-only Redis command (e.g. GET / HGET; pick one of this and `sql`)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// Redis command arguments (support variable interpolation)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Single-value extraction mode (defaults to scalar = first row, first column)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<DbTarget>,
        /// Variable name for the single value (may coexist with `columns`)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extract_var: Option<String>,
        /// Multi-column mapping: writes the columns of row `row` into multiple variables
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        columns: Vec<ColumnVar>,
        /// Row index used by the multi-column mapping (default 0)
        #[serde(default)]
        row: usize,
        /// Polling retry policy (waits for the data to become ready)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry: Option<RetryPolicy>,
    },
    /// Built-in "interpolate" node: turns the request template into the final message (variable interpolation + body encoding/assembly).
    ///
    /// **Cannot be edited / deleted / disabled / reordered**: it is the only position marker in the pre-action list,
    /// dividing "pre-interpolation actions" from "post-interpolation actions" - actions before it operate on the request template
    /// (they may write variables consumed by this interpolation and their rewrites are still interpolated); actions after it operate on the final message
    /// (rewrites become the final bytes with no second interpolation), which suits signing / encryption.
    ///
    /// YAML shape: `{ type: interpolate }`.
    ///
    /// Must be present exactly once in the list (see [`normalize_pre_actions`] for normalization).
    Interpolate,
    /// Reference to a "script library" entry (reusable action template).
    ///
    /// The request stores only a **reference**; before execution `resolve_action_refs` expands it into the entry's current concrete action:
    /// editing the entry -> takes effect at every reference immediately; deleting the entry -> the reference is kept as-is (dangling)
    /// and the execution layer logs an error **without aborting the request**, never dropping it silently.
    ///
    /// YAML shape: `{ type: ref, library_id: "<id>", name: "<optional alias>" }`.
    ///
    /// Position matters as much as for scripts: sitting before or after the built-in interpolate node decides whether the expanded action runs pre- or post-interpolation.
    Ref {
        /// Library entry id
        library_id: String,
        /// Optional display alias (falls back to the library entry name)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Whether enabled (skipped when disabled; also skipped when the library entry itself is disabled)
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        enabled: bool,
    },
}

impl RequestAction {
    /// Whether enabled
    pub fn is_enabled(&self) -> bool {
        match self {
            RequestAction::Script { enabled, .. }
            | RequestAction::Db { enabled, .. }
            | RequestAction::Ref { enabled, .. } => *enabled,
            // The built-in anchor is always enabled (cannot be disabled)
            RequestAction::Interpolate => true,
        }
    }

    /// Whether this is the built-in "interpolate" node
    pub fn is_interpolate(&self) -> bool {
        matches!(self, RequestAction::Interpolate)
    }

    /// Whether this is a script library reference
    pub fn is_ref(&self) -> bool {
        matches!(self, RequestAction::Ref { .. })
    }

    /// Action name (a per-type default is used when unset)
    pub fn name(&self) -> &str {
        match self {
            RequestAction::Script { name, .. } | RequestAction::Db { name, .. } => name,
            // Alias of a reference (the display layer falls back to the library entry name when unset)
            RequestAction::Ref { name, .. } => name.as_deref().unwrap_or(""),
            // The display name comes from i18n and is not persisted
            RequestAction::Interpolate => "",
        }
    }

    /// Action type id (`script` / `db` / `interpolate` / `ref`) used for logs and result grouping
    pub fn kind(&self) -> &'static str {
        match self {
            RequestAction::Script { .. } => "script",
            RequestAction::Db { .. } => "db",
            RequestAction::Interpolate => "interpolate",
            RequestAction::Ref { .. } => "ref",
        }
    }

    /// Source code of a script action (None for non-script actions)
    pub fn script_code(&self) -> Option<&str> {
        match self {
            RequestAction::Script { code, .. } => Some(code.as_str()),
            RequestAction::Db { .. } | RequestAction::Interpolate | RequestAction::Ref { .. } => {
                None
            }
        }
    }

    /// Set the display name (ignored for the built-in interpolate node, which has no name)
    pub fn set_name(&mut self, value: String) {
        match self {
            RequestAction::Script { name, .. } | RequestAction::Db { name, .. } => *name = value,
            RequestAction::Ref { name, .. } => *name = Some(value),
            RequestAction::Interpolate => {}
        }
    }

    /// Set the enabled state (ignored for the built-in interpolate node, which is always enabled)
    pub fn set_enabled(&mut self, value: bool) {
        match self {
            RequestAction::Script { enabled, .. }
            | RequestAction::Db { enabled, .. }
            | RequestAction::Ref { enabled, .. } => *enabled = value,
            RequestAction::Interpolate => {}
        }
    }

    /// Build a script action from source code (used when normalizing the legacy single-script field)
    pub fn from_script(code: impl Into<String>) -> Self {
        RequestAction::Script {
            name: String::new(),
            enabled: true,
            language: None,
            code: code.into(),
        }
    }

    /// Build a script library reference action
    pub fn from_ref(library_id: impl Into<String>) -> Self {
        RequestAction::Ref {
            library_id: library_id.into(),
            name: None,
            enabled: true,
        }
    }
}

/// Normalize an action list: when the explicit list is empty, wrap the legacy single script string into a single script action.
///
/// Legacy data (`pre_script` / `post_script` strings) and new data (action arrays) share this entry point,
/// guaranteeing no loss of historical config; when the explicit list is non-empty the legacy fields are ignored (avoiding double execution).
pub fn normalize_actions(actions: &[RequestAction], legacy: Option<&String>) -> Vec<RequestAction> {
    if !actions.is_empty() {
        return actions.to_vec();
    }
    match legacy {
        Some(code) if !code.trim().is_empty() => vec![RequestAction::from_script(code.clone())],
        _ => Vec::new(),
    }
}

/// Merge the previous two-stage fields (`pre_resolve_*`) into a single list, **without inventing an anchor**.
///
/// - `compat` is empty: return the `actions` / `legacy` normalization as-is (no anchor introduced);
/// - `base` already has the built-in anchor: treated as "already single-list form", so the **compat fields are dropped** (cleanup only).
///   This guard also keeps the operation idempotent - when a client writes back an already-merged list together with un-cleaned compat fields,
///   no duplicate actions are inserted;
/// - otherwise: `compat + [anchor] + base` (`base` means post-interpolation, `compat` means pre-interpolation).
///
/// Meant for import/export and snapshot-migration paths that must not write out an anchor out of thin air when there are no pre-actions;
/// execution and UI paths should use [`normalize_pre_actions`] (which additionally guarantees a single existing anchor).
pub fn merge_pre_actions(
    actions: &[RequestAction],
    legacy: Option<&String>,
    compat: &[RequestAction],
    compat_legacy: Option<&String>,
) -> Vec<RequestAction> {
    let base = normalize_actions(actions, legacy);
    let compat = normalize_actions(compat, compat_legacy);
    if compat.is_empty() || base.iter().any(RequestAction::is_interpolate) {
        return base;
    }

    let mut out = compat;
    out.push(RequestAction::Interpolate);
    out.extend(base);
    out
}

/// Normalize the **pre**-action list into "a single ordered list + one unique built-in interpolate node".
///
/// Parameters:
/// - `actions` / `legacy`: the request's pre-actions and its legacy single-script field (meaning = **post-interpolation**)
/// - `compat` / `compat_legacy`: the "pre-interpolation action" fields left over from the previous two-stage change
///   (`pre_resolve_actions` / `pre_resolve_script`, read for compatibility only and never written back)
///
/// Rules (list order is execution order):
/// 1. First merge the compat fields via [`merge_pre_actions`];
/// 2. If the list already has the built-in anchor -> keep the **first** one and drop the rest;
/// 3. If the list has no anchor -> insert one at the **front**. Existing `pre_actions` / `pre_script` mean
///    "post-interpolation" and must all come after the anchor, so it is inserted at the front rather than the end - this is what keeps existing configs
///    behaving identically;
/// 4. The result contains at least one anchor (the list "always exists and is unique").
pub fn normalize_pre_actions(
    actions: &[RequestAction],
    legacy: Option<&String>,
    compat: &[RequestAction],
    compat_legacy: Option<&String>,
) -> Vec<RequestAction> {
    let merged = merge_pre_actions(actions, legacy, compat, compat_legacy);
    let mut out = Vec::with_capacity(merged.len() + 1);
    let mut seen_anchor = false;
    for action in merged {
        if action.is_interpolate() {
            if seen_anchor {
                // Duplicate anchor: keep only the first
                continue;
            }
            seen_anchor = true;
        }
        out.push(action);
    }
    if !seen_anchor {
        out.insert(0, RequestAction::Interpolate);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_action() {
        let yaml = r#"
type: script
name: "rewrite request"
code: "pm.request.url += '/v2';"
"#;
        let a: RequestAction = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(a.kind(), "script");
        assert!(a.is_enabled());
        assert_eq!(a.name(), "rewrite request");
        assert_eq!(a.script_code(), Some("pm.request.url += '/v2';"));
    }

    #[test]
    fn parse_db_action_with_columns() {
        let yaml = r#"
type: db
datasource: "user-db"
sql: "SELECT id, mobile FROM users WHERE id = '{{user_id}}'"
columns:
  - { column: id, var: dbUserId }
  - { column: mobile, var: mobile }
retry: { interval_ms: 200, max_attempts: 3 }
"#;
        let a: RequestAction = serde_yaml::from_str(yaml).unwrap();
        match &a {
            RequestAction::Db {
                datasource,
                columns,
                retry,
                row,
                ..
            } => {
                assert_eq!(datasource, "user-db");
                assert_eq!(columns.len(), 2);
                assert_eq!(columns[0].var, "dbUserId");
                assert_eq!(retry.as_ref().unwrap().max_attempts, 3);
                assert_eq!(*row, 0);
            }
            other => panic!("expected Db, got {:?}", other),
        }
        assert!(a.script_code().is_none());
    }

    #[test]
    fn db_action_roundtrip_minimal() {
        // Minimal form: datasource + sql + extract_var only
        let yaml = r#"
type: db
datasource: "cache"
sql: "SELECT 1"
extract_var: "one"
"#;
        let a: RequestAction = serde_yaml::from_str(yaml).unwrap();
        let s = serde_yaml::to_string(&a).unwrap();
        let back: RequestAction = serde_yaml::from_str(&s).unwrap();
        assert_eq!(back.kind(), "db");
        assert!(back.is_enabled());
    }

    #[test]
    fn normalize_falls_back_to_legacy_script() {
        let legacy = "pm.request.url += '/x';".to_string();
        let out = normalize_actions(&[], Some(&legacy));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].script_code(), Some("pm.request.url += '/x';"));

        // The legacy field is ignored when the explicit list is non-empty
        let actions = vec![RequestAction::from_script("void 0;")];
        let out2 = normalize_actions(&actions, Some(&legacy));
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0].script_code(), Some("void 0;"));

        // A blank legacy script produces no action
        assert!(normalize_actions(&[], Some(&"   ".to_string())).is_empty());
        assert!(normalize_actions(&[], None).is_empty());
    }

    #[test]
    fn disabled_action_parsed() {
        let a: RequestAction =
            serde_yaml::from_str("type: script\nenabled: false\ncode: \"1;\"").unwrap();
        assert!(!a.is_enabled());
    }

    #[test]
    fn parse_interpolate_node() {
        let a: RequestAction = serde_yaml::from_str("type: interpolate").unwrap();
        assert!(a.is_interpolate());
        assert_eq!(a.kind(), "interpolate");
        assert_eq!(a.name(), "");
        // The built-in node is always enabled (cannot be disabled)
        assert!(a.is_enabled());
        assert_eq!(a.script_code(), None);
        // The YAML shape is fixed to `type: interpolate`
        assert_eq!(
            serde_yaml::to_string(&a).unwrap().trim(),
            "type: interpolate"
        );
    }

    #[test]
    fn normalize_pre_actions_appends_anchor_before_legacy_actions() {
        // Existing `pre_actions` (meaning = post-interpolation): with no explicit anchor, one is inserted at the **front of the list**,
        // so all existing actions end up after interpolation -> behavior is unchanged.
        let actions = vec![RequestAction::from_script("after();")];
        let out = normalize_pre_actions(&actions, None, &[], None);
        assert_eq!(out.len(), 2);
        assert!(out[0].is_interpolate());
        assert_eq!(out[1].script_code(), Some("after();"));
    }

    #[test]
    fn normalize_pre_actions_places_legacy_single_script_after_anchor() {
        // The legacy single-script field (pre_script / prereqScript) means post-interpolation and must come after the anchor
        let legacy = "pm.request.url += '/v2';".to_string();
        let out = normalize_pre_actions(&[], Some(&legacy), &[], None);
        assert_eq!(out.len(), 2);
        assert!(out[0].is_interpolate());
        assert_eq!(out[1].script_code(), Some("pm.request.url += '/v2';"));
    }

    #[test]
    fn normalize_pre_actions_splices_compat_actions_before_anchor() {
        // The previous `pre_resolve_actions` (= pre-interpolation) is spliced in before the anchor
        let base = vec![RequestAction::from_script("after();")];
        let compat = vec![RequestAction::from_script("before();")];
        let out = normalize_pre_actions(&base, None, &compat, None);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].script_code(), Some("before();"));
        assert!(out[1].is_interpolate());
        assert_eq!(out[2].script_code(), Some("after();"));
    }

    #[test]
    fn normalize_pre_actions_keeps_explicit_anchor_position() {
        // An explicit anchor position is the stage boundary: before it = pre-interpolation, after it = post-interpolation
        let base = vec![
            RequestAction::from_script("pre();"),
            RequestAction::Interpolate,
            RequestAction::from_script("post();"),
        ];
        let out = normalize_pre_actions(&base, None, &[], None);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].script_code(), Some("pre();"));
        assert!(out[1].is_interpolate());
        assert_eq!(out[2].script_code(), Some("post();"));
    }

    #[test]
    fn normalize_pre_actions_keeps_first_anchor_and_drops_duplicates() {
        let base = vec![
            RequestAction::from_script("a();"),
            RequestAction::Interpolate,
            RequestAction::from_script("b();"),
            RequestAction::Interpolate,
            RequestAction::from_script("c();"),
        ];
        let out = normalize_pre_actions(&base, None, &[], None);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].script_code(), Some("a();"));
        assert!(out[1].is_interpolate());
        assert_eq!(out[2].script_code(), Some("b();"));
        assert_eq!(out[3].script_code(), Some("c();"));
    }

    #[test]
    fn normalize_pre_actions_always_keeps_a_single_anchor() {
        // Empty config -> only the built-in anchor remains (the list "always exists and is unique")
        let out = normalize_pre_actions(&[], None, &[], None);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_interpolate());
        // An empty / blank legacy single script likewise leaves only the anchor
        assert!(normalize_pre_actions(&[], Some(&"  ".to_string()), &[], None)[0].is_interpolate());
    }

    #[test]
    fn merge_pre_actions_drops_compat_when_list_already_has_anchor() {
        // The list already has an anchor => already single-list form: the compat fields are just un-cleaned leftovers and are dropped outright (idempotent)
        let merged = vec![
            RequestAction::from_script("before();"),
            RequestAction::Interpolate,
            RequestAction::from_script("after();"),
        ];
        let compat = vec![RequestAction::from_script("before();")];
        let out = merge_pre_actions(&merged, None, &compat, None);
        assert_eq!(
            out.len(),
            3,
            "compat actions must not be inserted twice: {out:?}"
        );
        assert_eq!(out[0].script_code(), Some("before();"));
        assert!(out[1].is_interpolate());
        assert_eq!(out[2].script_code(), Some("after();"));
    }

    #[test]
    fn merge_pre_actions_does_not_invent_an_anchor() {
        // Migration path (import/export / snapshot): no anchor is invented when there are no compat fields
        let actions = vec![RequestAction::from_script("a();")];
        let out = merge_pre_actions(&actions, None, &[], None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].script_code(), Some("a();"));
        assert!(merge_pre_actions(&[], None, &[], None).is_empty());

        // With compat fields present an anchor is added, preserving the two-stage semantics
        let compat = vec![RequestAction::from_script("before();")];
        let out = merge_pre_actions(&actions, None, &compat, None);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].script_code(), Some("before();"));
        assert!(out[1].is_interpolate());
        assert_eq!(out[2].script_code(), Some("a();"));
    }

    #[test]
    fn parse_ref_action() {
        let yaml = r#"
type: ref
library_id: "tpl-sign"
name: "signing"
"#;
        let a: RequestAction = serde_yaml::from_str(yaml).unwrap();
        assert!(a.is_ref());
        assert_eq!(a.kind(), "ref");
        assert_eq!(a.name(), "signing");
        assert!(a.is_enabled());
        assert_eq!(a.script_code(), None);
        match &a {
            RequestAction::Ref {
                library_id, name, ..
            } => {
                assert_eq!(library_id, "tpl-sign");
                assert_eq!(name.as_deref(), Some("signing"));
            }
            other => panic!("expected Ref, got {other:?}"),
        }
    }

    #[test]
    fn ref_roundtrip_omits_empty_alias() {
        // No custom display name: `name` is not written out, keeping the YAML / wire shape stable
        let a: RequestAction = serde_yaml::from_str("type: ref\nlibrary_id: tpl-1").unwrap();
        assert_eq!(a.name(), "");
        let s = serde_yaml::to_string(&a).unwrap();
        assert!(
            !s.contains("name"),
            "an empty alias must not be written out: {s}"
        );
        let back: RequestAction = serde_yaml::from_str(&s).unwrap();
        assert!(back.is_ref());
        assert_eq!(back.name(), "");
    }

    #[test]
    fn ref_disabled_parsed() {
        let a: RequestAction =
            serde_yaml::from_str("type: ref\nlibrary_id: tpl-1\nenabled: false").unwrap();
        assert!(!a.is_enabled());
    }

    #[test]
    fn normalize_keeps_ref_in_place() {
        // A reference is as position-sensitive as a script: normalization must not move / drop it
        let actions = vec![
            RequestAction::from_ref("tpl-1"),
            RequestAction::from_script("after();"),
        ];

        let pre = normalize_pre_actions(&actions, None, &[], None);
        assert_eq!(pre.len(), 3);
        assert!(pre[0].is_interpolate());
        assert!(pre[1].is_ref());
        assert_eq!(pre[2].script_code(), Some("after();"));

        let merged = merge_pre_actions(&actions, None, &[RequestAction::from_script("b();")], None);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].script_code(), Some("b();"));
        assert!(merged[1].is_interpolate());
        assert!(merged[2].is_ref());
        assert_eq!(merged[3].script_code(), Some("after();"));
    }
}
