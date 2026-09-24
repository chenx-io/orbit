//! Validation and normalization of model output into strongly typed domain entities.
//!
//! Principle: **JSON produced by the model only passes if it deserializes into the authoritative types of `orbit_data::model`**.
//! On failure the error message is fed back to the model (a human-readable description) so it can correct itself and retry,
//! avoiding dirty data being written into the snapshot.

use orbit_config::{ActionTemplate, RequestAction};
use orbit_data::model::{
    ApiRequest, Collection, HttpRequest, Scenario, ScenarioDataSet, ScenarioFolder, TestSuite,
};
use serde_json::{json, Value};

use crate::error::{AiError, AiResult};

/// Lenient lookup of an argument key.
///
/// The tool schema declares camelCase (`collectionId`), but the model occasionally sends snake_case
/// (`collection_id`). A key mismatch shows up as hard-to-diagnose errors like "missing required parameter", so
/// both spellings are accepted.
fn lookup<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = args.get(key) {
        return Some(v);
    }
    let mut snake = String::with_capacity(key.len() + 4);
    for (i, c) in key.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    args.get(&snake)
}

/// Read a required string from the tool arguments.
pub fn require_str(args: &Value, key: &str) -> AiResult<String> {
    lookup(args, key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AiError::Invalid(format!("missing required parameter `{key}`")))
}

/// Read an optional string.
pub fn opt_str(args: &Value, key: &str) -> Option<String> {
    lookup(args, key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Read an optional bool.
pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    lookup(args, key).and_then(|v| v.as_bool())
}

/// Read an optional u64.
pub fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    lookup(args, key).and_then(|v| v.as_u64())
}

/// Read a non-negative integer argument such as an "index".
///
/// Lenient about numeric strings (the model occasionally writes `"index": "1"`): compared with a hard-to-diagnose "missing required parameter" error,
/// accepting an obviously unambiguous spelling is the better trade. Negative / fractional values are always rejected, clearly stating that this is an index.
pub fn require_index(args: &Value, key: &str) -> AiResult<usize> {
    let raw = lookup(args, key)
        .ok_or_else(|| AiError::Invalid(format!("missing required parameter `{key}`")))?;
    if let Some(n) = raw.as_u64() {
        return Ok(n as usize);
    }
    if let Some(text) = raw.as_str() {
        if let Ok(n) = text.trim().parse::<u64>() {
            return Ok(n as usize);
        }
    }
    Err(AiError::Invalid(format!(
        "`{key}` must be a non-negative integer index starting from 0, got {raw}"
    )))
}

/// RFC 7386 JSON Merge Patch。
///
/// Used by `update_request` / `update_scenario`: the model only provides the fields to change,
/// which are merged onto the **complete object** before strong typing (avoiding the partial-patch problem of untagged unions).
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

/// Field names that must be normalized as key/value arrays.
const KV_FIELDS: &[&str] = &["headers", "queryParams", "pathParams", "formParams"];

/// Field names that must be normalized as action arrays (single pre list / post).
///
/// Correspond one-to-one with the frontend `preActions` / `postActions` and the YAML `pre_actions` / `post_actions`.
/// `preResolveActions` is the compatibility field from the previous two-phase design: it is still normalized, then merged into `preActions`
/// (see [`merge_pre_actions_json`]), and no longer kept separately.
const ACTION_FIELDS: &[&str] = &["preResolveActions", "preActions", "postActions"];

// ─── Actions (pre / post) ────────────────────────────────

/// Type identifier of the builtin interpolation node (always present and unique in the pre list, guaranteed by both engine and frontend).
const INTERPOLATE_TYPE: &str = "interpolate";
/// Type identifier of a script library reference.
const REF_TYPE: &str = "ref";

/// Action shape hint (the "actionable correction" fed back to the model on a parse/validation failure).
const ACTION_SHAPE_HINT: &str = "An action takes only four forms: \
script {\"type\":\"script\",\"code\":\"…\"}, \
database query {\"type\":\"db\",\"datasource\":\"datasource id\",\"sql\":\"…\"}\
(optional command/args/target/extract_var/columns/row/retry), \
script library reference {\"type\":\"ref\",\"library_id\":\"library item id\"}, \
builtin interpolation node {\"type\":\"interpolate\"} (always present and unique; cannot be added / modified / deleted / moved).";

/// External environment needed for action validation (things the AI side knows but `orbit-ai` must not query itself).
///
/// `Option` instead of an empty array distinguishes "currently none" from "not provided by the caller":
/// - `Some(vec![])`: the current workspace has no datasource / library item at all -> any reference is an error;
/// - `None`: the caller did not provide this information -> skip that validation (keeping existing callers' behavior unchanged).
#[derive(Debug, Clone, Default)]
pub struct ActionEnv {
    /// Available datasource ids (a db action's `datasource` must match one).
    pub datasource_ids: Option<Vec<String>>,
    /// Script library item ids (a ref action's `library_id` must match one); `None` = references are not allowed.
    pub library_ids: Option<Vec<String>>,
}

impl ActionEnv {
    /// The host provides both lists (full validation).
    pub fn new(datasource_ids: Vec<String>, library_ids: Vec<String>) -> Self {
        Self {
            datasource_ids: Some(datasource_ids),
            library_ids: Some(library_ids),
        }
    }
}

/// Action list selector: pre (including the builtin interpolation node) or post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionList {
    /// Pre-actions (a single ordered list + the builtin interpolation node).
    Pre,
    /// Post-actions (run after the response is received).
    Post,
}

impl ActionList {
    /// Parse the `list` argument from the model (lenient spellings: pre/post, preActions/postActions).
    pub fn parse(raw: &str) -> AiResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pre" | "preactions" | "pre-actions" | "before-interpolation" => Ok(Self::Pre),
            "post" | "postactions" | "post-actions" => Ok(Self::Post),
            other => Err(AiError::Invalid(format!(
                "list must be either \"pre\" (pre-actions list, including the builtin interpolation node) or \"post\" (post-actions list), \
                 got `{other}`"
            ))),
        }
    }

    /// JSON field name (corresponds to the frontend `preActions` / `postActions` and the YAML `pre_actions` / `post_actions`).
    pub fn field(self) -> &'static str {
        match self {
            Self::Pre => "preActions",
            Self::Post => "postActions",
        }
    }

    /// Label used in error messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pre => "pre-actions",
            Self::Post => "post-actions",
        }
    }
}

/// Action-level edit operation (one call does one thing: easier for the model to reason about, and the proposal granularity stays clear).
#[derive(Debug, Clone)]
pub enum ActionOp {
    /// Insert one action (`index` omitted = append to the end of the list).
    Insert {
        /// Target index (0-based; omitted = append to the end).
        index: Option<usize>,
        /// The action to insert (script / db query / library item reference).
        action: Value,
    },
    /// Update the action at `index` (patch uses JSON Merge Patch semantics; only provide the fields to change).
    Update {
        /// Target index (0-based).
        index: usize,
        /// Patch containing only the fields to change (`null` = delete that field).
        patch: Value,
    },
    /// Delete the action at `index`.
    Delete {
        /// Target index (0-based).
        index: usize,
    },
    /// Move the action at `from` to `to` (crossing the builtin interpolation node changes the pre-/post-interpolation semantics).
    Move {
        /// Source index (0-based).
        from: usize,
        /// Target index (0-based).
        to: usize,
    },
}

/// Normalize the action fields of the request JSON into "the copy the engine actually executes", and drop the read-only compatibility fields.
///
/// Why normalize instead of reading/writing as-is:
/// - Before executing, the engine runs [`orbit_config::normalize_pre_actions`] (adds/dedupes the builtin anchor, merges the legacy script fields).
///   If the model sees a **non-normalized** list, the indices it computes will be off relative to the ones used at execution time;
/// - When legacy fields (`prereqScript` / `postreqScript` / `preResolveActions`) coexist with the new lists,
///   which one wins depends on the normalization rules - letting the model edit based on "the list as it appears" is guaranteed to hit the wrong position.
///
/// The rules reuse `orbit_config` directly (same source as engine and frontend, avoiding drift across three places):
/// - Pre: `preResolveActions + anchor + preActions`; a missing anchor is inserted **first**, and on duplicates only the first is kept;
/// - Post: if `postActions` is non-empty, `postreqScript` is ignored; if it is empty and the legacy script is non-empty, the latter is wrapped into a single script action.
///
/// Idempotent: once compatibility fields are cleared and the anchor exists, repeated calls change nothing.
pub fn canonicalize_actions(obj: &mut serde_json::Map<String, Value>) -> AiResult<()> {
    // Lenient read: fill in bare strings / missing type first (same tolerance as the create/update paths)
    for field in ACTION_FIELDS {
        normalize_action_array(obj, field);
    }
    let prereq = obj
        .get("prereqScript")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let postreq = obj
        .get("postreqScript")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let compat = parse_actions_field("preResolveActions", &json_array(obj, "preResolveActions"))?;
    let base = parse_actions_field("preActions", &json_array(obj, "preActions"))?;
    let post = parse_actions_field("postActions", &json_array(obj, "postActions"))?;

    let pre = orbit_config::normalize_pre_actions(&base, prereq.as_ref(), &compat, None);
    let post = orbit_config::normalize_actions(&post, postreq.as_ref());

    // Edit means migrate: compatibility fields are never written out again (the frontend does the same migration on first edit)
    obj.remove("preResolveActions");
    obj.remove("prereqScript");
    obj.remove("postreqScript");
    obj.insert("preActions".into(), serde_json::to_value(&pre)?);
    if post.is_empty() {
        obj.remove("postActions");
    } else {
        obj.insert("postActions".into(), serde_json::to_value(&post)?);
    }
    Ok(())
}

/// Validate the **external references** in the action lists: script library references (`library_id`) and datasources (`datasource`).
///
/// Neither can be judged inside `orbit-ai` (the library item and datasource lists are provided by the host), so we error out explicitly and
/// give the available list instead of letting the model invent ids from memory - an invented id only blows up at execution time, and what the user sees is
/// "query failed" rather than "the id was wrong".
pub fn validate_action_env(obj: &serde_json::Map<String, Value>, env: &ActionEnv) -> AiResult<()> {
    for field in ACTION_FIELDS {
        let items = json_array(obj, field);
        for (i, item) in items.iter().enumerate() {
            let position = i + 1;
            match action_type(item)? {
                REF_TYPE => {
                    let id = item
                        .get("library_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    if id.is_empty() {
                        return Err(AiError::Invalid(format!(
                            "{field} action #{position} is a script library reference but is missing library_id. {ACTION_SHAPE_HINT}"
                        )));
                    }
                    match &env.library_ids {
                        Some(ids) if ids.iter().any(|k| k == id) => {}
                        Some(ids) => {
                            return Err(AiError::Invalid(format!(
                                "{field} action #{position} references a nonexistent script library item `{id}`. {}",
                                library_hint(ids)
                            )));
                        }
                        None => {
                            return Err(AiError::Invalid(format!(
                                "{field} action #{position} uses a script library reference, but the current context forbids referencing library items: \
                                 write the full action inline instead. {ACTION_SHAPE_HINT}"
                            )));
                        }
                    }
                }
                "db" => {
                    if let Some(known) = &env.datasource_ids {
                        let ds = item
                            .get("datasource")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim();
                        if !known.iter().any(|k| k == ds) {
                            return Err(AiError::Invalid(format!(
                                "{field} db action #{position} references a nonexistent datasource `{ds}`. {}",
                                datasource_hint(known)
                            )));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Perform one action-level edit on the request JSON.
///
/// First [`canonicalize_actions`] to get a list consistent with execution semantics, then locate by index; an out-of-range index, touching
/// the builtin interpolation node, or changing an action's type is rejected explicitly together with the **actual contents of the current list** (so the model can self-correct).
/// External references (library items / datasources) are checked by [`validate_action_env`] during final validation.
pub fn edit_actions(request: &mut Value, list: ActionList, op: ActionOp) -> AiResult<()> {
    let obj = request
        .as_object_mut()
        .ok_or_else(|| AiError::Invalid("request must be a JSON object".into()))?;
    canonicalize_actions(obj)?;

    let field = list.field();
    let label = list.label();
    let mut items = json_array(obj, field);
    match op {
        ActionOp::Insert { index, action } => {
            let mut action = action;
            normalize_action_item(&mut action);
            if action_type(&action)? == INTERPOLATE_TYPE {
                return Err(AiError::Invalid(format!(
                    "The builtin \"interpolate\" node always exists and is unique, so it cannot be inserted: it is maintained by the system and its position defines the pre-/post-interpolation boundary. \
                     If you need it elsewhere, use move_action to move other actions instead. {ACTION_SHAPE_HINT}"
                )));
            }
            let at = index.unwrap_or(items.len());
            if at > items.len() {
                return Err(bounds_error(label, at, &items, "insert"));
            }
            items.insert(at, action);
        }
        ActionOp::Update { index, patch } => {
            let current = items
                .get(index)
                .ok_or_else(|| bounds_error(label, index, &items, "update"))?
                .clone();
            if action_type(&current)? == INTERPOLATE_TYPE {
                return Err(anchor_error(label, "update"));
            }
            let mut merged = current.clone();
            merge_patch(&mut merged, &patch);
            normalize_action_item(&mut merged);
            let kind = action_type(&merged)?;
            if kind != action_type(&current)? {
                return Err(AiError::Invalid(format!(
                    "An action type cannot be switched via a patch (action #{} is `{}` but the patch gives `{kind}`). \
                     To change the type, delete_action first and then insert_action, or pass insert_action a complete action.",
                    index + 1,
                    action_type(&current)?
                )));
            }
            items[index] = merged;
        }
        ActionOp::Delete { index } => {
            let current = items
                .get(index)
                .ok_or_else(|| bounds_error(label, index, &items, "delete"))?;
            if action_type(current)? == INTERPOLATE_TYPE {
                return Err(anchor_error(label, "delete"));
            }
            items.remove(index);
        }
        ActionOp::Move { from, to } => {
            let moved = items
                .get(from)
                .ok_or_else(|| bounds_error(label, from, &items, "move"))?
                .clone();
            if action_type(&moved)? == INTERPOLATE_TYPE {
                return Err(anchor_error(label, "move"));
            }
            if to >= items.len() {
                return Err(bounds_error(label, to, &items, "move to"));
            }
            items.remove(from);
            items.insert(to, moved);
        }
    }

    obj.insert(field.into(), Value::Array(items));
    Ok(())
}

/// Lenient normalization of a single action (bare string / missing type -> canonical form), plus a shape check.
///
/// Shared with the insert path in [`edit_actions`]: when `insert_action` is given an action directly it must also tolerate
/// the natural spelling `{"code": "…"}` (missing type).
pub fn normalize_single_action(value: &Value) -> AiResult<Value> {
    let mut action = value.clone();
    normalize_action_item(&mut action);
    let kind = action_type(&action)?;
    if kind == INTERPOLATE_TYPE {
        return Err(AiError::Invalid(format!(
            "The builtin \"interpolate\" node is maintained by the system and cannot be passed in as an action. {ACTION_SHAPE_HINT}"
        )));
    }
    Ok(action)
}

/// Parse an action object from the model arguments (reused by insert_action / update_action_template).
pub fn require_object_action(args: &Value, key: &str) -> AiResult<Value> {
    let raw = lookup(args, key)
        .cloned()
        .ok_or_else(|| AiError::Invalid(format!("missing required parameter `{key}`")))?;
    normalize_single_action(&raw)
}

/// Action type identifier (when `type` is missing, a default is **not** filled in here - that is [`normalize_action_item`]'s job).
fn action_type(action: &Value) -> AiResult<&str> {
    action.get("type").and_then(|v| v.as_str()).ok_or_else(|| {
        AiError::Invalid(format!(
            "The action is missing a `type` field. {ACTION_SHAPE_HINT}"
        ))
    })
}

/// Read an action array field (missing / not an array -> empty array).
fn json_array(obj: &serde_json::Map<String, Value>, field: &str) -> Vec<Value> {
    obj.get(field)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Parse the actions one by one (the error message carries the position so the model can locate it).
fn parse_actions_field(field: &str, items: &[Value]) -> AiResult<Vec<RequestAction>> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            serde_json::from_value::<RequestAction>(item.clone()).map_err(|e| {
                AiError::Invalid(format!(
                    "{field} action #{} cannot be parsed: {e}. {ACTION_SHAPE_HINT}",
                    i + 1
                ))
            })
        })
        .collect()
}

/// Human-readable summary of a single action (used for proposal titles / error messages): type + name + content snippet.
pub fn action_summary(action: &Value) -> String {
    let kind = action.get("type").and_then(|v| v.as_str()).unwrap_or("?");
    let name = action
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let detail = match kind {
        INTERPOLATE_TYPE => "builtin interpolate node".to_string(),
        "db" => clip_text(
            action
                .get("sql")
                .or_else(|| action.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            40,
        ),
        "script" => clip_text(
            action.get("code").and_then(|v| v.as_str()).unwrap_or(""),
            40,
        ),
        REF_TYPE => format!(
            "library item {}",
            action
                .get("library_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
        ),
        other => other.to_string(),
    };
    let head = match (kind, name.is_empty()) {
        (INTERPOLATE_TYPE, _) => "builtin interpolate node".to_string(),
        ("db", true) => "db query".to_string(),
        ("db", false) => format!("db query \"{name}\""),
        ("script", true) => "script".to_string(),
        ("script", false) => format!("script \"{name}\""),
        (REF_TYPE, true) => "script library reference".to_string(),
        (REF_TYPE, false) => format!("script library reference \"{name}\""),
        (other, _) => other.to_string(),
    };
    // The interpolate node's detail is its own name; avoid "builtin interpolate node (builtin interpolate node)"
    if kind == INTERPOLATE_TYPE || detail.is_empty() || detail == head {
        head
    } else {
        format!("{head} ({detail})")
    }
}

/// Human-readable summary of the current list (fed back to the model in error messages so it can relocate).
fn describe_actions(items: &[Value]) -> String {
    if items.is_empty() {
        return "(empty)".into();
    }
    items
        .iter()
        .enumerate()
        .map(|(i, a)| format!("#{i}={}", action_summary(a)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn bounds_error(label: &str, index: usize, items: &[Value], verb: &str) -> AiError {
    AiError::Invalid(format!(
        "{label} list has no index {index} (index is 0-based; the usable range when {verb} is 0..={}). Current list: {}. \
         Call get_request first to read the latest action list, then retry.",
        items.len(),
        describe_actions(items)
    ))
}

fn anchor_error(label: &str, verb: &str) -> AiError {
    AiError::Invalid(format!(
        "The builtin \"interpolate\" node cannot be {verb}: it defines the pre-/post-interpolation boundary (actions before it = pre-interpolation, after it = post-interpolation), \
         and is maintained by the system. Other actions in the {label} list can be edited normally."
    ))
}

fn library_hint(ids: &[String]) -> String {
    if ids.is_empty() {
        "The script library of the current workspace is empty: create an item with save_action_template first, or write the complete action inline.".into()
    } else {
        let shown: Vec<&str> = ids.iter().take(10).map(String::as_str).collect();
        format!(
            "Available library item ids (use list_action_templates to see names and contents): {}",
            shown.join(" / ")
        )
    }
}

fn datasource_hint(ids: &[String]) -> String {
    if ids.is_empty() {
        "The current workspace has no datasource yet: add one in the \"Data Sources\" module before creating db actions.".into()
    } else {
        let shown: Vec<&str> = ids.iter().take(10).map(String::as_str).collect();
        format!(
            "Available datasource ids (use list_data_sources to see names): {}",
            shown.join(" / ")
        )
    }
}

/// Normalize an action array: tolerate the model's natural habit of writing only the script text.
///
/// The domain model requires `{type:"script",code}` / `{type:"db",...}`, but the model often writes
/// `["pm.environment.set('a','1')"]` (a bare string) or `{code:"..."}` (missing type) -
/// both are silently dropped by untagged/serde, so the script vanishes into thin air with no error at all.
fn normalize_action_array(obj: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(items) = obj.get_mut(field).and_then(|v| v.as_array_mut()) else {
        return;
    };
    for item in items.iter_mut() {
        normalize_action_item(item);
    }
}

/// Lenient normalization of a single action (shared by [`normalize_action_array`] and [`edit_actions`]):
/// bare string -> script action; object missing `type` -> treated as a script action.
///
/// The domain model requires an explicit `type`, but the model often writes `["pm.environment.set('a','1')"]` or `{"code":"…"}`
/// - both are silently dropped by serde, so the script vanishes into thin air with no error at all.
fn normalize_action_item(item: &mut Value) {
    if let Some(code) = item.as_str() {
        *item = json!({ "type": "script", "code": code });
        return;
    }
    if let Some(map) = item.as_object_mut() {
        if !map.contains_key("type") {
            map.insert("type".into(), json!("script"));
        }
    }
}

/// Normalize key/value arrays: tolerate the object spelling and fill in missing `id`s.
///
/// Why this is required:
/// 1. `KeyValue.id` is required in the domain model (the frontend uses it as the React key and update target),
///    but **the model should only care about key/value/enabled** - forcing it to invent internal ids is pure torment;
/// 2. `ApiRequest` is an untagged union; when a field is missing serde can only report
///    the information-free error "data did not match any variant of untagged enum ApiRequest",
///    which the model cannot use to correct itself. The observed consequence is severe: after failing twice in a row the model voluntarily "degrades to a minimal definition"
///    (keeping only name/method/url), so the request the user gets has no body and no assertions.
/// 3. The object spelling (`{"Content-Type": "application/json"}`) is equally natural, so accept it as well.
fn normalize_kv_array(obj: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(value) = obj.get_mut(field) else {
        return;
    };
    match value {
        // object spelling -> array spelling
        Value::Object(map) => {
            let pairs: Vec<Value> = map
                .iter()
                .map(|(k, v)| {
                    json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "key": k,
                        "value": v.as_str().unwrap_or_default(),
                        "enabled": true,
                    })
                })
                .collect();
            *value = Value::Array(pairs);
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                let Some(entry) = item.as_object_mut() else {
                    continue;
                };
                if entry
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    entry.insert("id".into(), json!(uuid::Uuid::new_v4().to_string()));
                }
            }
        }
        _ => {}
    }
}

/// Fields in the assertion array that are **stored as strings**.
///
/// In the domain model these fields are `String`, but the model (and people writing JSON by hand) naturally gives numbers or booleans.
/// Note that `value` for `status` / `size_lt` is itself a numeric type and **must not** be stringified.
const ASSERTION_STRING_KEYS: &[&str] = &["value", "expected", "pattern", "path", "schema"];

/// Common aliases of the assertion `comparator` -> official name (official list = [`crate::syntax::COMPARATOR_NAMES`]).
///
/// Why this must be corrected: the engine's `parse_comparator` **silently degrades unknown names to `equal`**,
/// so the model writing `contain` (missing the s) or `eq` (Postman habit) raises no error, it just **quietly changes the semantics** -
/// these "looks like it passed" assertions are the most dangerous. Recognized aliases are always pulled back to the official name; unrecognized ones are rejected outright with the official list.
fn normalize_comparator(name: &str) -> Option<&'static str> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "" | "eq" | "equal" | "equals" => "equal",
        "ne" | "neq" | "not_equal" | "notequal" => "not_equal",
        "contains" | "contain" | "includes" => "contains",
        "not_contains" | "notcontain" | "excludes" => "not_contains",
        "exists" | "exist" | "present" => "exists",
        "matches" | "match" | "regex" | "regexp" => "matches",
        "gt" | "greater_than" | "greaterthan" => "gt",
        "lt" | "less_than" | "lessthan" => "lt",
        _ => return None,
    })
}

/// Normalize the assertion array, translating the model's "natural phrasing" into the concrete types the engine requires.
///
/// Three real pitfalls:
/// 1. `duration_lt.value` is a string parsed by `parse_duration` - **a bare number is treated as seconds**
///    (`pipeline.rs` then multiplies by 1000 to get milliseconds), so the model writing `3000` (meaning 3 seconds) is read as 3000 seconds,
///    and the assertion never fails. So numbers always get an `ms` unit appended;
/// 2. fields like `body_contains.value` / `expected` are strings, so numbers/booleans need explicit conversion;
/// 3. a misspelled `comparator` name is **silently treated as `equal`** by the engine (see [`normalize_comparator`]).
fn normalize_assertions(obj: &mut serde_json::Map<String, Value>) -> AiResult<()> {
    let Some(Value::Array(items)) = obj.get_mut("assertions") else {
        return Ok(());
    };
    for item in items.iter_mut() {
        let Some(check) = item.as_object_mut() else {
            continue;
        };
        let kind = check
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(raw) = check.get("comparator").and_then(|v| v.as_str()) {
            match normalize_comparator(raw) {
                Some(official) => {
                    check.insert("comparator".into(), json!(official));
                }
                None => {
                    return Err(AiError::Invalid(format!(
                        "assertion comparator `{raw}` is not an official name; the engine silently treats it as `equal` (the assertion quietly changes meaning).\
                         Allowed values: {}.",
                        crate::syntax::COMPARATOR_NAMES.join(" / ")
                    )));
                }
            }
        }
        if kind == "duration_lt" {
            let ms = match check.get("value") {
                Some(Value::Number(n)) => Some(format!("{n}ms")),
                _ => None,
            };
            if let Some(value) = ms {
                check.insert("value".into(), json!(value));
            }
            continue;
        }
        // value for status / size_lt is a numeric type, keep it as-is
        let keep_numeric = matches!(kind.as_str(), "status" | "size_lt");
        for key in ASSERTION_STRING_KEYS {
            if keep_numeric && *key == "value" {
                continue;
            }
            let replacement = match check.get(*key) {
                Some(Value::Number(n)) => Some(json!(n.to_string())),
                Some(Value::Bool(b)) => Some(json!(b.to_string())),
                _ => None,
            };
            if let Some(v) = replacement {
                check.insert((*key).to_string(), v);
            }
        }
    }
    Ok(())
}

/// Infer `bodyMode` from the request body content (HTTP only).
///
/// Why this is needed: `bodyMode` defaults to `none`, and the engine then drops the request body outright - producing
/// the hardest-to-diagnose form: "the request definition clearly has a body, yet it runs empty". When body is given without
/// bodyMode, infer from content: starting with `{` / `[` counts as JSON (**does not require it to actually parse**,
/// since JSON containing `{{var}}` placeholders cannot be parsed anyway, yet it really is a JSON body).
fn infer_body_mode(obj: &mut serde_json::Map<String, Value>) {
    let body = obj
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if body.is_empty() {
        return;
    }
    let declared = obj
        .get("bodyMode")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    if declared != "none" {
        return;
    }
    let looks_json = body.starts_with('{') || body.starts_with('[');
    obj.insert(
        "bodyMode".into(),
        json!(if looks_json { "json" } else { "raw" }),
    );
    if looks_json
        && obj
            .get("contentType")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        obj.insert("contentType".into(), json!("application/json"));
    }
}

/// Field explanation given to the model on validation failure (reused in several places to avoid divergent wording).
///
/// The official `comparator` list is taken directly from [`crate::syntax::COMPARATOR_NAMES`] (single source of truth),
/// kept consistent with the prompt's "syntax reference" and the anti-drift test.
fn request_shape_hint() -> String {
    format!(
        "An HTTP request needs at least method and url; \
         each headers/queryParams/pathParams/formParams item needs at least key and value (the internal id is auto-filled); \
         websocket/grpc/tcp/udp/sse/graphql need protocol and url; \
         bodyMode can only be none/json/xml/form-data/x-www-form-urlencoded/raw/binary; \
         an assertion's type can only be status / body_contains / duration_lt / jsonpath / jmespath / regex / \
         size_lt / xpath / jsonschema / header / css_selector / db / redis; \
         an assertion's comparator can only be {}",
        crate::syntax::COMPARATOR_NAMES.join(" / ")    )
}

/// Normalize and validate a request definition (no external environment: script library references are explicitly rejected).
pub fn normalize_request(value: &Value) -> AiResult<ApiRequest> {
    normalize_request_with_env(value, &ActionEnv::default())
}

/// Normalize and validate a request definition, and validate external references in actions (library items / data sources) against `env`.
///
/// The host (Tauri) should always use this version: only it knows which library items and data sources exist in the current workspace.
pub fn normalize_request_with_env(value: &Value, env: &ActionEnv) -> AiResult<ApiRequest> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("request must be a JSON object".into()))?;

    let protocol = obj
        .get("protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("http")
        .to_string();

    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    if obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        obj.insert("name".into(), json!("Untitled request"));
    }
    if protocol == "http" {
        obj.entry("method").or_insert_with(|| json!("GET"));
        obj.entry("url").or_insert_with(|| json!(""));
    }
    for field in KV_FIELDS {
        normalize_kv_array(&mut obj, field);
    }
    // First canonicalize the action lists into "the one the engine actually executes" (with the built-in anchor and legacy script fields merged in),
    // then run request-level validation: this way the indices the model gives always match the indices at execution time.
    canonicalize_actions(&mut obj)?;
    validate_action_env(&obj, env)?;
    normalize_assertions(&mut obj)?;
    if protocol == "http" {
        infer_body_mode(&mut obj);
    }

    let text = serde_json::to_string(&Value::Object(obj))?;
    let parsed: ApiRequest = match serde_json::from_str(&text) {
        Ok(parsed) => parsed,
        Err(e) => {
            return Err(AiError::Invalid(hint_error(
                protocol,
                &text,
                &e.to_string(),
            )))
        }
    };

    if protocol == "http" {
        // We must explicitly require the Http variant: `ApiRequest` ends with a **permissive variant** prepared for plugin protocols
        // (url/headers/protocol all optional); when HttpRequest parsing fails it swallows the whole blob of JSON,
        // so all we can report afterwards is misleading conclusions like "url cannot be empty" (we have hit this in practice). Here we surface the real cause.
        let ApiRequest::Http(http) = &parsed else {
            let detail = serde_json::from_str::<HttpRequest>(&text)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "field shape does not match an HTTP request".to_string());
            return Err(AiError::Invalid(format!(
                "request definition is invalid (protocol=http): {detail}. {}.",
                request_shape_hint()
            )));
        };
        if http.url.trim().is_empty() {
            return Err(AiError::Invalid("HTTP request url cannot be empty".into()));
        }
    } else if parsed.protocol() != protocol {
        return Err(AiError::Invalid(format!(
            "protocol is declared as `{protocol}`, but the field shape is parsed as `{}`; \
             please fill in the protocol-specific fields (or remove the extra fields).",
            parsed.protocol()
        )));
    }
    Ok(parsed)
}

/// Content of a script library item (`id` / `sort_index` are decided by the host: generated on create, max value on append).
#[derive(Debug, Clone)]
pub struct ActionTemplateInput {
    /// Library item name (display name, also the action name after reference expansion).
    pub name: String,
    /// Description (may be empty).
    pub description: Option<String>,
    /// The concrete action (can only be a script or database query).
    pub action: RequestAction,
}

/// Validate and normalize the content of a script library item.
///
/// Library items **only allow script / database actions** ([`ActionTemplate::is_valid`]): a built-in interpolation node would break
/// the anchor uniqueness of the action list, and nested references would make parsing recursive - both are explicitly rejected with corrective guidance.
pub fn normalize_action_template(
    name: &str,
    description: Option<String>,
    action: &Value,
) -> AiResult<ActionTemplateInput> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AiError::Invalid("library item name cannot be empty".into()));
    }
    let action_value = normalize_single_action(action)?;
    let parsed: RequestAction = serde_json::from_value(action_value).map_err(|e| {
        AiError::Invalid(format!(
            "library item action cannot be parsed: {e}. {ACTION_SHAPE_HINT}"
        ))
    })?;
    // Reuse the domain model's judgment to avoid rewriting "which actions can go into the library" here
    let probe = ActionTemplate::new("tpl-check", name, parsed.clone());
    if !probe.is_valid() {
        return Err(AiError::Invalid(format!(
            "a library item can only contain a script or database query: it cannot be a built-in interpolation node, nor reference another library item. {ACTION_SHAPE_HINT}"
        )));
    }
    Ok(ActionTemplateInput {
        name: name.to_string(),
        description: description.filter(|d| !d.trim().is_empty()),
        action: parsed,
    })
}

/// Build the "parse failed" error message: prefer a diagnostic that names the field (HTTP case).
fn hint_error(protocol: String, text: &str, generic: &str) -> String {
    let detail = if protocol == "http" {
        serde_json::from_str::<HttpRequest>(text)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| generic.to_string())
    } else {
        generic.to_string()
    };
    format!(
        "request definition is invalid: {detail}. {}.",
        request_shape_hint()
    )
}

/// Normalize and validate a collection.
pub fn normalize_collection(value: &Value, workspace_id: &str) -> AiResult<Collection> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("collection must be a JSON object".into()))?;
    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    obj.insert("workspaceId".into(), json!(workspace_id));
    obj.entry("items").or_insert_with(|| json!([]));
    if obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(AiError::Invalid("collection needs a name".into()));
    }
    serde_json::from_value(Value::Object(obj))
        .map_err(|e| AiError::Invalid(format!("collection is invalid: {e}")))
}

/// Normalize and validate a scenario.
pub fn normalize_scenario(value: &Value, workspace_id: &str) -> AiResult<Scenario> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("scenario must be a JSON object".into()))?;
    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    obj.insert("workspaceId".into(), json!(workspace_id));
    if obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(AiError::Invalid("scenario needs a name".into()));
    }
    let steps = obj
        .get_mut("steps")
        .ok_or_else(|| AiError::Invalid("scenario needs a steps array".into()))?;
    fill_step_defaults(steps)?;

    serde_json::from_value(Value::Object(obj)).map_err(|e| {
        AiError::Invalid(format!(
            "scenario is invalid: {e}. A step type can only be request/loop/condition/wait/group/setvar; \
             a request step needs requestId."
        ))
    })
}

/// Recursively fill in step id / name (`StepBase` has no serde default).
fn fill_step_defaults(steps: &mut Value) -> AiResult<()> {
    let arr = steps
        .as_array_mut()
        .ok_or_else(|| AiError::Invalid("steps must be an array".into()))?;
    for step in arr.iter_mut() {
        let obj = step
            .as_object_mut()
            .ok_or_else(|| AiError::Invalid("a step must be a JSON object".into()))?;
        obj.entry("id")
            .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
        obj.entry("name").or_insert_with(|| json!(""));
        for key in ["children", "elseChildren"] {
            if let Some(child) = obj.get_mut(key) {
                if child.is_array() {
                    fill_step_defaults(child)?;
                }
            }
        }
    }
    Ok(())
}

/// Normalize and validate a test suite.
pub fn normalize_suite(value: &Value, workspace_id: &str) -> AiResult<TestSuite> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("suite must be a JSON object".into()))?;
    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    obj.insert("workspaceId".into(), json!(workspace_id));
    obj.entry("runMode").or_insert_with(|| json!("serial"));
    obj.entry("memberIds").or_insert_with(|| json!([]));
    if obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(AiError::Invalid("suite needs a name".into()));
    }
    if obj
        .get("memberIds")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true)
    {
        return Err(AiError::Invalid(
            "suite needs memberIds (a list of scenario ids; use list_scenarios first to get them)"
                .into(),
        ));
    }
    serde_json::from_value(Value::Object(obj))
        .map_err(|e| AiError::Invalid(format!("suite is invalid: {e}")))
}

/// Normalize and validate a scenario folder.
pub fn normalize_scenario_folder(value: &Value, workspace_id: &str) -> AiResult<ScenarioFolder> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("folder must be a JSON object".into()))?;
    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    obj.insert("workspaceId".into(), json!(workspace_id));
    if obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return Err(AiError::Invalid("scenario folder needs a name".into()));
    }
    serde_json::from_value(Value::Object(obj))
        .map_err(|e| AiError::Invalid(format!("scenario folder is invalid: {e}")))
}

/// Normalize and validate a CSV data set (`columns`/`row_count` are derived from the csv automatically).
pub fn normalize_data_set(value: &Value, workspace_id: &str) -> AiResult<ScenarioDataSet> {
    let mut obj = value
        .as_object()
        .cloned()
        .ok_or_else(|| AiError::Invalid("dataSet must be a JSON object".into()))?;
    obj.entry("id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    obj.insert("workspaceId".into(), json!(workspace_id));
    let csv = obj
        .get("csv")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if csv.is_empty() {
        return Err(AiError::Invalid(
            "data set needs csv text (the first row is the header)".into(),
        ));
    }
    let (columns, row_count, mode) = analyze_csv(&csv);
    obj.insert("csv".into(), json!(csv));
    obj.insert("columns".into(), json!(columns));
    obj.insert("rowCount".into(), json!(row_count));
    obj.entry("mode").or_insert_with(|| json!(mode));
    obj.insert(
        "updatedAt".into(),
        json!(jiff::Timestamp::now().as_millisecond()),
    );
    serde_json::from_value(Value::Object(obj))
        .map_err(|e| AiError::Invalid(format!("data set is invalid: {e}")))
}

/// Parse CSV: returns (header column names, data row count, default read mode).
pub fn analyze_csv(csv: &str) -> (Vec<String>, usize, String) {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let columns = lines
        .next()
        .map(|header| {
            header
                .split(',')
                .map(|c| c.trim().trim_matches('"').to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let row_count = lines.count();
    (columns, row_count, "sequential".to_string())
}

/// Truncate long text the model may not need (response bodies etc.), keeping the head and a length hint.
pub fn clip_text(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!("{head}\n…(original length {count} chars, truncated)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_data::model::BodyMode;
    use serde_json::json;

    #[test]
    fn merge_patch_overrides_and_removes_nulls() {
        let mut target = json!({"name":"a","url":"https://x","auth":{"type":"none","token":"t"}});
        merge_patch(
            &mut target,
            &json!({"name":"b","auth":{"token":null,"type":"bearer"}}),
        );
        assert_eq!(target["name"], "b");
        assert_eq!(target["url"], "https://x");
        assert_eq!(target["auth"]["type"], "bearer");
        assert!(target["auth"].get("token").is_none());
    }

    #[test]
    fn http_request_gets_default_id_name_and_passes() {
        let req =
            normalize_request(&json!({"method":"POST","url":"https://api.x.com/login"})).unwrap();
        assert_eq!(req.name(), "Untitled request");
        assert!(!req.id().is_empty());
        assert_eq!(req.protocol(), "http");
    }

    #[test]
    fn http_request_without_url_is_rejected_with_hint() {
        let err = normalize_request(&json!({"method":"GET","url":""})).unwrap_err();
        match err {
            AiError::Invalid(msg) => assert!(msg.contains("url")),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn protocol_mismatch_is_reported() {
        // grpc is declared but the http required field (method) is given, untagged matches Http first
        let err = normalize_request(&json!({
            "protocol":"grpc",
            "method":"GET",
            "url":"http://x",
        }))
        .unwrap_err();
        match err {
            AiError::Invalid(msg) => assert!(msg.contains("protocol"), "{msg}"),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn grpc_request_parses_when_shaped_correctly() {
        let req = normalize_request(&json!({
            "protocol":"grpc",
            "name":"SayHello",
            "url":"http://localhost:50051",
            "service":"greeter.Greeter",
        }))
        .unwrap();
        assert_eq!(req.protocol(), "grpc");
    }

    #[test]
    fn scenario_steps_are_normalized_recursively() {
        let s = normalize_scenario(
            &json!({
                "name":"place-order flow",
                "steps":[
                    {"type":"request","requestId":"r1"},
                    {"type":"loop","count":3,"children":[{"type":"wait","ms":100}]}
                ]
            }),
            "ws-1",
        )
        .unwrap();
        assert_eq!(s.workspace_id, "ws-1");
        assert_eq!(s.steps.len(), 2);
        match &s.steps[1] {
            orbit_data::model::ScenarioStep::Loop(l) => {
                assert!(!l.children.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn scenario_without_name_is_rejected() {
        assert!(normalize_scenario(&json!({"steps":[]}), "ws").is_err());
    }

    #[test]
    fn scenario_unknown_step_type_is_rejected() {
        assert!(
            normalize_scenario(&json!({"name":"x","steps":[{"type":"teleport"}]}), "ws").is_err()
        );
    }

    #[test]
    fn suite_requires_member_ids() {
        assert!(normalize_suite(&json!({"name":"smoke"}), "ws").is_err());
        let s = normalize_suite(&json!({"name":"smoke","memberIds":["s1","s2"]}), "ws").unwrap();
        assert_eq!(s.run_mode, "serial");
        assert_eq!(s.member_ids.len(), 2);
    }

    #[test]
    fn dataset_derives_columns_and_row_count() {
        let ds = normalize_data_set(
            &json!({"name":"login-data","csv":"user,pass\nu1,p1\nu2,p2\n"}),
            "ws",
        )
        .unwrap();
        assert_eq!(ds.columns, vec!["user", "pass"]);
        assert_eq!(ds.row_count, 2);
        assert_eq!(ds.mode.as_deref(), Some("sequential"));
    }

    #[test]
    fn dataset_without_csv_is_rejected() {
        assert!(normalize_data_set(&json!({"name":"x","csv":"  "}), "ws").is_err());
    }

    #[test]
    fn folder_normalization_sets_workspace() {
        let f = normalize_scenario_folder(&json!({"name":"User Module"}), "ws").unwrap();
        assert_eq!(f.workspace_id, "ws");
    }

    #[test]
    fn clip_text_marks_truncation() {
        let out = clip_text(&"a".repeat(10), 4);
        assert!(out.starts_with("aaaa"));
        assert!(out.contains("truncated"));
    }

    #[test]
    fn require_str_rejects_blank() {
        assert!(require_str(&json!({"a":"  "}), "a").is_err());
        assert!(require_str(&json!({}), "a").is_err());
        assert_eq!(require_str(&json!({"a":" x "}), "a").unwrap(), "x");
    }

    #[test]
    fn arg_lookup_accepts_snake_case_aliases() {
        // The model occasionally passes args in snake_case (the Schema is camelCase): this must not trigger a "missing required parameter"
        assert_eq!(
            require_str(&json!({"collection_id":"c1"}), "collectionId").unwrap(),
            "c1"
        );
        assert_eq!(
            opt_str(&json!({"request_id":"r1"}), "requestId").as_deref(),
            Some("r1")
        );
        assert_eq!(
            opt_u64(&json!({"duration_sec":30}), "durationSec"),
            Some(30)
        );
        assert_eq!(
            opt_bool(&json!({"auto_apply":true}), "autoApply"),
            Some(true)
        );
        // camelCase takes precedence
        assert_eq!(
            require_str(
                &json!({"collectionId":"a","collection_id":"b"}),
                "collectionId"
            )
            .unwrap(),
            "a"
        );
    }

    /// Regression (reproducing a real incident): a model-produced "POST with a random JSON body" must pass validation on the first try.
    ///
    /// What happened: the headers the model produced were `{key,value,enabled}` (**no id**, because that is exactly
    /// how the tool shape hint is written), while `KeyValue.id` is required by the domain model; the untagged union then smeared the error into
    /// "data did not match any variant of untagged enum ApiRequest", so the model cannot self-correct,
    /// and after two retries it degrades to a "minimal definition" (keeping only name/method/url) - the request the user ends up with
    /// has no body and no assertions. This test locks in: such natural forms must be accepted with body/assertions/scripts intact.
    #[test]
    fn model_style_payload_without_nested_ids_is_accepted() {
        let body = r#"{
  "requestId": "{{$uuid.v4}}",
  "timestamp": {{$timestamp.ms}},
  "orderNo": "{{rand_order_no}}",
  "amount": {{rand_amount}},
  "customer": { "name": "{{rand_name}}", "isActive": {{rand_is_active}} }
}"#;
        let raw = json!({
            "protocol": "http",
            "name": "ApiFox Echo POST (random JSON)",
            "method": "POST",
            "url": "https://echo.apifox.com/post",
            "headers": [
                { "key": "Content-Type", "value": "application/json", "enabled": true },
                { "key": "X-Request-Id", "value": "{{$uuid.v4}}", "enabled": true }
            ],
            "body": body,
            "bodyMode": "json",
            "contentType": "application/json",
            "assertions": [
                { "type": "status", "value": 200 },
                { "type": "header", "name": "content-type", "comparator": "contains", "expected": "json" },
                { "type": "duration_lt", "value": 3000 }
            ],
            "prereqScript": "pm.variables.set('rand_amount', (Math.random() * 1000).toFixed(2));",
            "postreqScript": "pm.environment.set('echo_order_no', pm.response.json().json.orderNo);"
        });

        let req = match normalize_request(&raw) {
            Ok(req) => req,
            Err(e) => panic!(
                "a model-style request definition must pass on the first try, actual error: {}",
                e.user_message()
            ),
        };
        let ApiRequest::Http(http) = &req else {
            panic!("should parse as an HTTP request");
        };
        // body and bodyMode are the crux of this incident: they must never be dropped
        assert_eq!(http.body_mode, BodyMode::Json);
        assert!(http.body.contains("{{$uuid.v4}}"));
        assert!(http.body.contains("{{rand_amount}}"));
        assert_eq!(http.content_type, "application/json");
        assert_eq!(http.assertions.len(), 3);
        // The legacy single-script field is merged into the action list (editing migrates it): semantics unchanged, but indices now match execution time
        assert!(
            http.prereq_script.is_none(),
            "the legacy prereqScript should already be merged into preActions"
        );
        assert!(http.postreq_script.is_none());
        assert_eq!(http.pre_actions.len(), 2);
        assert!(
            http.pre_actions[0].is_interpolate(),
            "the anchor is inserted at the front"
        );
        assert!(http.pre_actions[1]
            .script_code()
            .unwrap()
            .contains("rand_amount"));
        assert_eq!(http.post_actions.len(), 1);
        assert!(http.post_actions[0]
            .script_code()
            .unwrap()
            .contains("echo_order_no"));
        // Key-value items missing an id should be auto-filled (otherwise the frontend gets no React key and cannot locate the update target)
        assert_eq!(http.headers.len(), 2);
        assert_eq!(http.headers[0].key, "Content-Type");
        assert!(!http.headers[0].id.is_empty(), "the id must be auto-filled");
        assert!(http.headers.iter().all(|h| !h.id.is_empty()));
    }

    #[test]
    fn body_without_mode_is_inferred_instead_of_dropped() {
        // body without bodyMode -> the engine treats it as none and drops the body (one of the hardest problems to track down)
        let raw = json!({
            "name": "x",
            "method": "POST",
            "url": "https://x",
            "body": "{\n  \"amount\": {{rand_amount}},\n  \"id\": \"{{$uuid.v4}}\"\n}"
        });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        assert_eq!(
            http.body_mode,
            BodyMode::Json,
            "if it looks like JSON, send it as JSON"
        );
        assert_eq!(
            http.content_type, "application/json",
            "fill in Content-Type along the way"
        );

        // Non-JSON text -> raw (at least do not drop the content)
        let raw = json!({ "name": "x", "method": "POST", "url": "https://x", "body": "a=1&b=2" });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        assert_eq!(http.body_mode, BodyMode::Raw);
        assert!(
            http.content_type.is_empty(),
            "raw must not invent a Content-Type"
        );

        // Do not override when it was declared explicitly
        let raw = json!({
            "name": "x", "method": "POST", "url": "https://x",
            "body": "{\"a\":1}", "bodyMode": "xml", "contentType": "application/xml"
        });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        assert_eq!(http.body_mode, BodyMode::Xml);
        assert_eq!(http.content_type, "application/xml");
    }

    #[test]
    fn numeric_duration_gets_millisecond_unit() {
        // A bare number is treated by parse_duration as seconds (then x1000 to milliseconds) -> off by 1000, so the assertion never fails
        let raw = json!({
            "name": "x", "method": "GET", "url": "https://x",
            "assertions": [
                { "type": "duration_lt", "value": 3000 },
                { "type": "status", "value": 200 },
                { "type": "size_lt", "value": 1024 },
                { "type": "jsonpath", "path": "$.code", "comparator": "eq", "expected": 0 },
                { "type": "body_contains", "value": true }
            ]
        });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        let kinds = &http.assertions;
        assert_eq!(kinds.len(), 5);
        // Numeric fields stay numbers while string fields get converted
        let rendered = serde_json::to_value(kinds).unwrap();
        assert_eq!(
            rendered[0]["value"], "3000ms",
            "the duration gets a millisecond unit appended"
        );
        assert_eq!(rendered[1]["value"], 200, "status stays a number");
        assert_eq!(rendered[2]["value"], 1024, "size_lt stays a number");
        assert_eq!(
            rendered[3]["expected"], "0",
            "expected is converted to a string"
        );
        assert_eq!(
            rendered[4]["value"], "true",
            "booleans are converted to strings"
        );
    }

    #[test]
    fn comparator_aliases_are_pulled_back_to_official_names() {
        // The engine **silently degrades an unrecognized comparator to equal**, so aliases must be corrected at the AI boundary,
        // otherwise the model writing `contain` (missing an s) quietly changes the assertion's meaning with no warning.
        let raw = json!({
            "name": "x", "method": "GET", "url": "https://x",
            "assertions": [
                // Only use assertion types that **have a comparator field** (types like body_contains lack it and serde drops it)
                { "type": "jsonpath", "path": "$.code", "comparator": "eq", "expected": "0" },
                { "type": "header", "name": "content-type", "comparator": "contain", "expected": "json" },
                { "type": "jmespath", "expression": "data.token", "comparator": "regex", "expected": "\\w+" }
            ]
        });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        let rendered = serde_json::to_value(&http.assertions).unwrap();
        assert_eq!(
            rendered[0]["comparator"], "equal",
            "eq should be pulled back to the official name"
        );
        assert_eq!(rendered[1]["comparator"], "contains");
        assert_eq!(
            rendered[2]["comparator"], "matches",
            "regex is the official alias of matches"
        );
    }

    #[test]
    fn unknown_comparator_is_rejected_with_official_list() {
        let raw = json!({
            "name": "x", "method": "GET", "url": "https://x",
            "assertions": [{ "type": "jsonpath", "path": "$.a", "comparator": "in_range", "expected": "1" }]
        });
        let err = normalize_request(&raw).unwrap_err().user_message();
        assert!(err.contains("comparator"), "{err}");
        assert!(
            err.contains("equal") && err.contains("not_equal"),
            "on rejection the official list must be given (so the model gets it right in one shot): {err}"
        );
    }

    #[test]
    fn key_value_arrays_accept_object_shorthand() {
        // `{"Content-Type": "application/json"}` is an equally natural form, so accept it too
        let raw = json!({
            "name": "x",
            "method": "GET",
            "url": "https://x",
            "headers": { "Accept": "application/json" }
        });
        let ApiRequest::Http(http) = normalize_request(&raw).unwrap() else {
            panic!("should parse as an HTTP request");
        };
        assert_eq!(http.headers.len(), 1);
        assert_eq!(http.headers[0].key, "Accept");
        assert_eq!(http.headers[0].value, "application/json");
        assert!(!http.headers[0].id.is_empty());
    }

    #[test]
    fn validation_error_names_the_offending_field() {
        // The untagged union's raw error is useless; replace it with a diagnosis that names the offending field
        let raw = json!({
            "name": "x",
            "method": "POST",
            "url": "https://x",
            "bodyMode": "jsonp"
        });
        let err = normalize_request(&raw).unwrap_err().user_message();
        assert!(err.contains("bodyMode"), "{err}");
        assert!(
            !err.contains("did not match any variant"),
            "should not throw the untagged gibberish straight at the model: {err}"
        );
    }

    /// Regression: a realistic "Apifox echo POST" request definition must pass validation.
    #[test]
    fn realistic_apifox_echo_request_passes_validation() {
        let raw = json!({
            "name": "Apifox Echo - POST",
            "method": "POST",
            "url": "https://echo.apifox.com/post",
            "headers": [
                { "id": "h1", "key": "Accept", "value": "application/json", "enabled": true }
            ],
            "queryParams": [],
            "pathParams": [],
            "bodyMode": "json",
            "contentType": "application/json",
            "body": "{\"name\":\"orbit\",\"ts\":{{$timestamp.ms}}}",
            "auth": { "type": "none" },
            "assertions": [
                { "type": "status", "value": 200 },
                { "type": "header", "name": "content-type", "comparator": "contains", "expected": "json" },
                { "type": "jsonpath", "path": "$.json.name", "comparator": "eq", "expected": "orbit" },
                { "type": "body_contains", "value": "\"method\"" }
            ],
            "prereqScript": "pm.environment.set('startedAt', String(Date.now()));",
            "postreqScript": "pm.test('echo matches', () => { pm.expect(pm.response.json().method).to.eql('POST'); });"
        });
        let req =
            normalize_request(&raw).expect("a realistic request definition must pass validation");
        assert_eq!(req.name(), "Apifox Echo - POST");
        match &req {
            ApiRequest::Http(h) => {
                assert_eq!(h.assertions.len(), 4);
                assert!(h.prereq_script.is_none());
                assert!(h.postreq_script.is_none());
                assert!(h.pre_actions[0].is_interpolate());
                assert!(h
                    .pre_actions
                    .iter()
                    .any(|a| a.script_code().is_some_and(|c| c.contains("startedAt"))));
                assert_eq!(h.post_actions.len(), 1);
            }
            other => panic!("should be an HTTP request, got {other:?}"),
        }
    }

    #[test]
    fn unknown_assertion_type_reports_valid_catalog() {
        let err = normalize_request(&json!({
            "method":"GET",
            "url":"https://x",
            "assertions":[{"type":"status_code","value":200}]
        }))
        .unwrap_err();
        match err {
            AiError::Invalid(msg) => {
                assert!(msg.contains("an assertion's type can only be"), "{msg}");
                assert!(msg.contains("jsonpath"), "{msg}");
            }
            other => panic!("unexpected {other}"),
        }
    }

    /// The pre-action array must catch the model's natural forms (bare string / missing `type`),
    /// otherwise the script is silently dropped by serde — the user sees "the script was configured but has no effect".
    ///
    /// Also locks in compatibility-field normalization: the previous version's `preResolveActions` (before interpolation) is merged into the single-list
    /// **before** the built-in interpolation node, and the compatibility key is no longer kept.
    #[test]
    fn normalizes_action_arrays_in_natural_shapes() {
        let req = normalize_request(&json!({
            "name": "sign request",
            "method": "POST",
            "url": "https://x.dev/pay",
            "preResolveActions": [
                "pm.environment.set('nonce','1')",
                { "code": "pm.environment.set('ts','2')" }
            ],
            "preActions": [{ "type": "script", "code": "void 0;" }],
            "postActions": ["pm.test('ok', () => {});"]
        }))
        .unwrap();
        let ApiRequest::Http(h) = &req else {
            panic!("should parse as an HTTP request");
        };
        // Single list: before-interpolation ×2 → built-in interpolation node → original preActions
        assert_eq!(h.pre_actions.len(), 4, "{:?}", h.pre_actions);
        assert_eq!(
            h.pre_actions[0].script_code(),
            Some("pm.environment.set('nonce','1')")
        );
        assert_eq!(
            h.pre_actions[1].script_code(),
            Some("pm.environment.set('ts','2')")
        );
        assert!(h.pre_actions[2].is_interpolate());
        assert_eq!(h.pre_actions[3].script_code(), Some("void 0;"));
        assert!(
            h.pre_resolve_actions.is_empty(),
            "the compatibility field should be merged into the single list"
        );
        assert_eq!(h.post_actions.len(), 1);
    }

    /// Idempotent: when the model supplies both an "anchor-containing single list" and compatibility fields, no action may be inserted twice.
    #[test]
    fn merge_pre_actions_is_idempotent_when_anchor_present() {
        let req = normalize_request(&json!({
            "name": "sign request",
            "method": "POST",
            "url": "https://x.dev/pay",
            "preResolveActions": ["pm.environment.set('nonce','1')"],
            "preActions": [
                { "type": "script", "code": "pm.environment.set('nonce','1')" },
                { "type": "interpolate" },
                { "type": "script", "code": "pm.request.headers.upsert({key:'X-S',value:'1'});" }
            ]
        }))
        .unwrap();
        let ApiRequest::Http(h) = &req else {
            panic!("should parse as an HTTP request");
        };
        assert_eq!(
            h.pre_actions.len(),
            3,
            "must not be inserted twice: {:?}",
            h.pre_actions
        );
        assert!(h.pre_actions[1].is_interpolate());
        assert!(h.pre_resolve_actions.is_empty());
    }

    /// Script-library references: when the host provides the library-item list, referencing is **allowed**, but the id must really exist.
    ///
    /// This used to always reject ("references are a UI concept; library item ids are local to the user"); once the script library became a workspace-level entity and
    /// the AI could list it, rejecting made no sense — the only real risk left is "inventing a non-existent id".
    #[test]
    fn library_refs_are_validated_against_available_templates() {
        let env = ActionEnv::new(Vec::new(), vec!["tpl-sign".into()]);

        // Hitting a library item -> passes, and the reference is kept as-is (the engine expands it into the item's current content before execution)
        let ok = normalize_request_with_env(
            &json!({
                "name": "place order",
                "method": "POST",
                "url": "https://x.dev/order",
                "preActions": [
                    { "type": "interpolate" },
                    { "type": "ref", "library_id": "tpl-sign", "name": "compute signature" }
                ]
            }),
            &env,
        )
        .expect("referencing an existing library item should pass");
        let ApiRequest::Http(h) = &ok else {
            panic!("should parse as an HTTP request");
        };
        assert!(h.pre_actions[1].is_ref());
        assert_eq!(h.pre_actions[1].name(), "compute signature");

        // A fabricated id -> rejected, and the available list plus how to obtain it is fed back to the model
        let err = normalize_request_with_env(
            &json!({
                "name": "place order",
                "method": "POST",
                "url": "https://x.dev/order",
                "preActions": [{ "type": "ref", "library_id": "tpl-nope" }]
            }),
            &env,
        )
        .expect_err("a non-existent library item should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("tpl-nope"), "{msg}");
        assert!(
            msg.contains("tpl-sign"),
            "should list the available library item ids: {msg}"
        );
        assert!(msg.contains("list_action_templates"), "{msg}");
    }

    /// When the library is empty, give the actionable guidance "create a library item first, or write the full action inline".
    #[test]
    fn empty_library_ref_reports_how_to_fix() {
        let err = normalize_request_with_env(
            &json!({
                "name": "x", "method": "GET", "url": "https://x",
                "preActions": [{ "type": "ref", "library_id": "tpl-1" }]
            }),
            &ActionEnv::new(Vec::new(), Vec::new()),
        )
        .expect_err("a reference into an empty library should be rejected");
        assert!(err.to_string().contains("save_action_template"), "{err}");
    }

    /// When the caller did not provide the library (legacy behavior), reject references explicitly and give the inline form.
    #[test]
    fn refs_without_library_context_are_rejected_with_inline_example() {
        let err = normalize_request(&json!({
            "name": "x", "method": "GET", "url": "https://x",
            "preActions": [{ "type": "ref", "library_id": "tpl-1" }]
        }))
        .expect_err("references should be rejected when no library was provided");
        let msg = err.to_string();
        assert!(msg.contains("ref"), "{msg}");
        assert!(
            msg.contains("script"),
            "should give the correct form: {msg}"
        );
    }

    /// A database action's `datasource` must really exist: an invented id blows up only at run time, and the user sees
    /// "query failed" rather than "the id was wrong".
    #[test]
    fn db_action_datasource_is_validated() {
        let env = ActionEnv::new(vec!["ds-mysql".into()], Vec::new());
        normalize_request_with_env(
            &json!({
                "name": "query user", "method": "GET", "url": "https://x",
                "preActions": [
                    { "type": "db", "datasource": "ds-mysql", "sql": "SELECT 1" },
                    { "type": "interpolate" }
                ]
            }),
            &env,
        )
        .expect("a matching datasource should pass");

        let err = normalize_request_with_env(
            &json!({
                "name": "query user", "method": "GET", "url": "https://x",
                "preActions": [{ "type": "db", "datasource": "ds-nope", "sql": "SELECT 1" }]
            }),
            &env,
        )
        .expect_err("a non-existent datasource should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("ds-nope"), "{msg}");
        assert!(msg.contains("ds-mysql"), "{msg}");
        assert!(msg.contains("list_data_sources"), "{msg}");
    }

    /// Legacy single-script field migration: when the list is empty it is wrapped into one action placed **after** the anchor (= after interpolation, zero behavior change),
    /// and the whole canonicalization is idempotent.
    #[test]
    fn canonicalize_folds_legacy_scripts_after_anchor() {
        let mut obj = json!({ "prereqScript": "sign()", "postreqScript": "check()" });
        canonicalize_actions(obj.as_object_mut().unwrap()).unwrap();
        assert_eq!(obj["preActions"][0]["type"], json!("interpolate"));
        assert_eq!(obj["preActions"][1]["code"], json!("sign()"));
        assert_eq!(obj["postActions"][0]["code"], json!("check()"));
        assert!(obj.get("prereqScript").is_none());
        assert!(obj.get("postreqScript").is_none());

        let once = obj.clone();
        canonicalize_actions(obj.as_object_mut().unwrap()).unwrap();
        assert_eq!(obj, once, "canonicalization must be idempotent");
    }

    /// User scenario: have the AI change the SQL of a database action in the pre list.
    #[test]
    fn edit_actions_updates_db_sql_by_index() {
        let mut req = json!({
            "name": "query user", "method": "GET", "url": "https://x",
            "preActions": [
                { "type": "interpolate" },
                { "type": "db", "datasource": "ds-mysql", "sql": "SELECT id FROM users" }
            ]
        });
        edit_actions(
            &mut req,
            ActionList::Pre,
            ActionOp::Update {
                index: 1,
                patch: json!({ "sql": "SELECT id, mobile FROM users WHERE id = '{{uid}}'" }),
            },
        )
        .unwrap();
        assert_eq!(
            req["preActions"][1]["sql"],
            json!("SELECT id, mobile FROM users WHERE id = '{{uid}}'")
        );
        // Patch semantics: fields not provided keep their original values
        assert_eq!(req["preActions"][1]["datasource"], json!("ds-mysql"));
        assert_eq!(req["preActions"][0]["type"], json!("interpolate"));
    }

    /// The built-in interpolation node cannot be modified / deleted / moved, nor inserted — it is the boundary anchor between before/after interpolation.
    #[test]
    fn edit_actions_protects_builtin_interpolate_node() {
        let base = || {
            json!({
                "name": "x", "method": "GET", "url": "https://x",
                "preActions": [{ "type": "interpolate" }, { "type": "script", "code": "a()" }]
            })
        };
        let cases = [
            (
                ActionOp::Update {
                    index: 0,
                    patch: json!({ "code": "x()" }),
                },
                "update",
            ),
            (ActionOp::Delete { index: 0 }, "delete"),
            (ActionOp::Move { from: 0, to: 1 }, "move"),
        ];
        for (op, verb) in cases {
            let mut req = base();
            let err = edit_actions(&mut req, ActionList::Pre, op).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("interpolate"), "{msg}");
            assert!(msg.contains(verb), "{msg}");
        }

        let mut req = base();
        let err = edit_actions(
            &mut req,
            ActionList::Pre,
            ActionOp::Insert {
                index: Some(1),
                action: json!({ "type": "interpolate" }),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("interpolate"), "{err}");
    }

    /// An out-of-range error must include "what the current list looks like" so the model can self-correct (far more useful than "index out of range").
    #[test]
    fn edit_actions_out_of_range_lists_current_actions() {
        let mut req = json!({
            "name": "x", "method": "GET", "url": "https://x",
            "postActions": [{ "type": "script", "name": "write var", "code": "a()" }]
        });
        let err =
            edit_actions(&mut req, ActionList::Post, ActionOp::Delete { index: 5 }).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("post-actions"), "{msg}");
        assert!(
            msg.contains("#0=script \"write var\""),
            "should list the current actions: {msg}"
        );
        assert!(msg.contains("get_request"), "{msg}");
    }

    /// A patch must not switch an action's type: that would necessarily produce a half-baked action (e.g. "has sql but no datasource").
    #[test]
    fn edit_actions_refuses_type_change_via_patch() {
        let mut req = json!({
            "name": "x", "method": "GET", "url": "https://x",
            "postActions": [{ "type": "script", "code": "a()" }]
        });
        let err = edit_actions(
            &mut req,
            ActionList::Post,
            ActionOp::Update {
                index: 0,
                patch: json!({ "type": "db", "datasource": "ds-1" }),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("delete_action"), "{err}");
    }

    /// Editing an action migrates the legacy fields as well, and the anchor always stays unique (a non-empty list means the legacy fields are semantically ignored).
    #[test]
    fn edit_actions_migrates_legacy_fields_and_keeps_single_anchor() {
        let mut req = json!({
            "name": "x", "method": "POST", "url": "https://x",
            "prereqScript": "pm.environment.set('a','1')",
            "preActions": [{ "type": "script", "code": "b()" }]
        });
        edit_actions(
            &mut req,
            ActionList::Pre,
            ActionOp::Insert {
                index: None,
                action: json!("c()"),
            },
        )
        .unwrap();
        assert!(
            req.get("prereqScript").is_none(),
            "the legacy field should be cleaned up"
        );
        let items = req["preActions"].as_array().unwrap();
        assert_eq!(
            items.len(),
            3,
            "anchor + original action + new action: {items:?}"
        );
        assert_eq!(items[0]["type"], json!("interpolate"));
        assert_eq!(items[1]["code"], json!("b()"));
        assert_eq!(
            items[2]["code"],
            json!("c()"),
            "a bare string should be canonicalized into a script action"
        );
    }

    /// A library item's content allows only scripts / database queries: both the anchor and nested references must be rejected.
    #[test]
    fn action_template_rejects_anchor_and_nested_ref() {
        let ok = normalize_action_template(
            "compute signature",
            None,
            &json!({ "type": "script", "code": "sign()" }),
        )
        .expect("a script library item should pass");
        assert_eq!(ok.name, "compute signature");
        assert_eq!(ok.description, None);

        let err = normalize_action_template("anchor", None, &json!({ "type": "interpolate" }))
            .unwrap_err();
        assert!(err.to_string().contains("interpolate"), "{err}");

        let err = normalize_action_template(
            "nesting",
            None,
            &json!({ "type": "ref", "library_id": "t-1" }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("library item"), "{err}");

        let err =
            normalize_action_template("  ", None, &json!({ "type": "script", "code": "x()" }))
                .unwrap_err();
        assert!(err.to_string().contains("name"), "{err}");
    }
}
