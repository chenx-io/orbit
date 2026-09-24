//! Execution host for AI tools (Tauri side): binds the `orbit_ai::ToolHost` contract to real data and the execution engine.
//!
//! Division of labor (boundary with `orbit-ai`):
//! - **Mode gating and confirmation** are handled solely by the Agent (this file does no second-guessing and assumes the invoked tools are already approved);
//! - This file owns "how to change data", "how to run", "how to write plans", and "explaining results in plain language";
//! - Write operations first pass strong-typed validation via `orbit_ai::tools::validate`, then emit a `DataChanged` event after persisting,
//!   so the frontend reloads its snapshot accordingly.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use orbit_ai::plan::PlanArtifact;
use orbit_ai::proposal::{Proposal, ProposalAction};
use orbit_ai::session::SessionStore;
use orbit_ai::tools::validate as vd;
use orbit_ai::tools::{ToolHost, ToolOutcome};
use orbit_ai::{AiError, AiResult, EventBus};
use orbit_config::{ActionTemplate, RequestAction};
use orbit_data::model::{
    ActionTemplateEntry, ApiRequest, Collection, CollectionItem, HttpRequest, Scenario,
    ScenarioDataSet, ScenarioFolder, TestSuite, Workspace,
};
use orbit_data::{DataService, FileStorage};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::commands::ai_plan;

/// Hard cap for a single load test (AI-triggered load tests must be bounded, to avoid an accidental overload taking down the target service).
pub const MAX_AI_VUS: u32 = 100;
/// Duration cap for a single load test (seconds).
pub const MAX_AI_DURATION_SECS: u64 = 120;
/// Character cap for the response body fed back to the model.
const BODY_EXCERPT_CHARS: usize = 4_000;

/// Everything about one write operation: argument validation -> validated entity -> persist action -> diff for display.
///
/// Separating "prepare" from "persist" ensures **validation errors happen before persisting** (error text is fed back to the model to correct),
/// rather than writing bad data and rolling back afterwards.
enum WriteOp {
    CreateCollection(Collection),
    CreateRequest {
        request: ApiRequest,
        collection_id: String,
        parent_id: Option<String>,
    },
    UpdateRequest(ApiRequest),
    /// Create / update a script-library entry (a workspace-level reusable action).
    SaveActionTemplate(ActionTemplateEntry),
    /// Delete a script-library entry (requests referencing it become dangling; requests keep working but an error is logged).
    DeleteActionTemplate(String),
    CreateScenarioFolder(ScenarioFolder),
    CreateScenario(Scenario),
    UpdateScenario(Scenario),
    CreateSuite(TestSuite),
    CreateDataSet(ScenarioDataSet),
}

struct Prepared {
    scope: &'static str,
    title: String,
    target: String,
    action: ProposalAction,
    before: Option<Value>,
    after: Value,
    op: WriteOp,
}

/// Tauri host.
pub struct TauriToolHost {
    pub data: Arc<DataService<FileStorage>>,
    pub data_sources: Arc<orbit_datasource::DataSourceRegistry>,
    pub cookie_jar: Arc<tokio::sync::Mutex<orbit_engine::cookie_jar::CookieJar>>,
    pub workspace_id: String,
    pub app_data_dir: std::path::PathBuf,
    pub bus: EventBus,
    /// Session store (plan-mode plans are persisted along with the session).
    pub sessions: SessionStore,
    /// Session id this turn belongs to.
    pub session_id: String,
}

impl TauriToolHost {
    fn ws(&self) -> String {
        self.workspace_id.clone()
    }

    fn tool_err(name: &str, msg: impl Into<String>) -> AiError {
        AiError::Tool {
            name: name.to_string(),
            message: msg.into(),
        }
    }

    fn notify_changed(&self, scope: &str) {
        self.bus.push(orbit_ai::AiEvent::DataChanged {
            scope: scope.into(),
        });
    }

    // ─── Read ───────────────────────────────────────────

    fn collections_tree(&self) -> Value {
        let ws = self.ws();
        let collections = self.data.collections_in(&ws);
        let tree: Vec<Value> = collections
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "items": self.items_json(&c.items),
                })
            })
            .collect();
        json!({ "collections": tree })
    }

    fn items_json(&self, items: &[CollectionItem]) -> Vec<Value> {
        items
            .iter()
            .map(|item| match item {
                CollectionItem::Folder { id, name, items } => json!({
                    "type": "folder",
                    "id": id,
                    "name": name,
                    "items": self.items_json(items),
                }),
                CollectionItem::Request { id, request_id } => {
                    let (name, protocol, method, url) = match self.data.request(request_id) {
                        Some(r) => (
                            r.name().to_string(),
                            r.protocol().to_string(),
                            match &r {
                                ApiRequest::Http(h) => h.method.clone(),
                                _ => String::new(),
                            },
                            match &r {
                                ApiRequest::Http(h) => ai_plan::resolve_request_url(h),
                                _ => String::new(),
                            },
                        ),
                        None => (
                            "(deleted)".into(),
                            String::new(),
                            String::new(),
                            String::new(),
                        ),
                    };
                    json!({
                        "type": "request",
                        "id": id,
                        "requestId": request_id,
                        "name": name,
                        "protocol": protocol,
                        "method": method,
                        "url": url,
                    })
                }
                other => json!({ "type": "grpc", "id": other.id() }),
            })
            .collect()
    }

    /// Walk the collection tree collecting "request id -> owning collection/parent node".
    fn request_owners(&self) -> HashMap<String, (String, Option<String>)> {
        fn walk(
            items: &[CollectionItem],
            collection_id: &str,
            parent: Option<&str>,
            out: &mut HashMap<String, (String, Option<String>)>,
        ) {
            for item in items {
                match item {
                    CollectionItem::Request { request_id, .. } => {
                        out.insert(
                            request_id.clone(),
                            (collection_id.to_string(), parent.map(str::to_string)),
                        );
                    }
                    CollectionItem::Folder { id, items, .. } => {
                        walk(items, collection_id, Some(id), out);
                    }
                    _ => {}
                }
            }
        }
        let mut out = HashMap::new();
        for c in self.data.collections_in(&self.ws()) {
            walk(&c.items, &c.id, None, &mut out);
        }
        out
    }

    fn list_requests(&self, collection_id: Option<&str>) -> Value {
        let ws = self.ws();
        let owners = self.request_owners();
        let mut rows = Vec::new();
        for (id, req) in self.data.requests() {
            let owner = owners.get(&id);
            if let Some(filter) = collection_id {
                if owner.map(|(c, _)| c.as_str()) != Some(filter) {
                    continue;
                }
            }
            let (method, url) = match &req {
                ApiRequest::Http(h) => (h.method.clone(), ai_plan::resolve_request_url(h)),
                other => (String::new(), other.name().to_string()),
            };
            rows.push(json!({
                "id": id,
                "name": req.name(),
                "protocol": req.protocol(),
                "method": method,
                "url": url,
                "collectionId": owner.map(|(c, _)| c.clone()),
                "hasPrereqScript": req.prereq_script().is_some_and(|s| !s.trim().is_empty()),
                "hasPostreqScript": req.postreq_script().is_some_and(|s| !s.trim().is_empty()),
            }));
        }
        let _ = ws;
        json!({ "requests": rows })
    }

    fn environments_json(&self) -> Value {
        let ws = self.ws();
        let active = self.data.active_env_in(&ws);
        let envs: Vec<Value> = self
            .data
            .environments_in(&ws)
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "name": e.name,
                    "variables": e.variables.keys().cloned().collect::<Vec<_>>(),
                    "secrets": e.secrets.keys().cloned().collect::<Vec<_>>(),
                    "active": Some(&e.id) == active.as_ref(),
                })
            })
            .collect();
        let global = self.data.global_variables_in(&ws);
        json!({
            "environments": envs,
            "globalVariables": global.keys().cloned().collect::<Vec<_>>(),
            "globalSecrets": self.data.global_secrets_in(&ws).keys().cloned().collect::<Vec<_>>(),
        })
    }

    fn models_json(&self) -> Value {
        let ws = self.ws();
        let rows: Vec<Value> = self
            .data
            .models_in(&ws)
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "name": m.name,
                    "description": m.description,
                    "fields": m.fields.iter().map(|f| json!({
                        "name": f.name,
                        "type": f.r#type,
                        "required": f.required.unwrap_or(false),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "models": rows })
    }

    fn scenarios_json(&self) -> Value {
        let ws = self.ws();
        let folders = self.data.scenario_folders_in(&ws);
        let scenarios: Vec<Value> = self
            .data
            .scenarios_in(&ws)
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "name": s.name,
                    "folderId": s.folder_id,
                    "folderName": s.folder_id.as_deref()
                        .and_then(|f| folders.iter().find(|x| x.id == f))
                        .map(|f| f.name.clone()),
                    "priority": s.priority,
                    "stepCount": s.steps.len(),
                    "envId": s.env_id,
                    "useDataSet": s.use_data_set.unwrap_or(false),
                })
            })
            .collect();
        let suites: Vec<Value> = self
            .data
            .scenario_suites_in(&ws)
            .iter()
            .map(|s| json!({ "id": s.id, "name": s.name, "runMode": s.run_mode, "memberCount": s.member_ids.len() }))
            .collect();
        let folders_json: Vec<Value> = folders
            .iter()
            .map(|f| json!({ "id": f.id, "name": f.name, "parentId": f.parent_id }))
            .collect();
        json!({ "scenarios": scenarios, "suites": suites, "folders": folders_json })
    }

    fn history_json(&self, limit: usize) -> Value {
        let ws = self.ws();
        let rows: Vec<Value> = self
            .data
            .history_in(&ws)
            .into_iter()
            .take(limit.clamp(1, 100))
            .map(|h| {
                json!({
                    "name": h.name,
                    "method": h.method,
                    "url": h.url,
                    "status": h.status,
                    "durationMs": h.duration,
                    "at": h.timestamp,
                })
            })
            .collect();
        json!({ "history": rows })
    }

    /// Read automation run-report summaries (`<app_data>/scenario_reports/*.json`).
    fn reports_json(&self, limit: usize) -> Value {
        let dir = self.app_data_dir.join("scenario_reports");
        let mut rows: Vec<Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        rows.push(json!({
                            "targetName": v.get("targetName"),
                            "status": v.get("status"),
                            "startedAt": v.get("startedAt"),
                            "durationMs": v.get("durationMs"),
                            "totalPass": v.get("totalPass"),
                            "totalFail": v.get("totalFail"),
                            "runMode": v.get("runMode"),
                        }));
                    }
                }
            }
        }
        rows.sort_by_key(|r| {
            std::cmp::Reverse(r.get("startedAt").and_then(|v| v.as_i64()).unwrap_or(0))
        });
        rows.truncate(limit.clamp(1, 50));
        json!({ "reports": rows })
    }

    /// Script-library entry list (with full actions): used by `list_action_templates` and action validation.
    fn action_templates(&self) -> Vec<ActionTemplate> {
        self.data.action_templates_in(&self.ws())
    }

    /// External lists needed for action validation: available data-source ids + workspace entry ids.
    ///
    /// If an AI-fabricated id (data source / library entry) were let through, it would only fail at **execution time**,
    /// showing the user "query failed" instead of "the id is wrong" - so we block them before persisting and feed back the available lists.
    fn action_env(&self) -> vd::ActionEnv {
        vd::ActionEnv::new(
            self.data.data_sources().into_iter().map(|c| c.id).collect(),
            self.action_templates().into_iter().map(|t| t.id).collect(),
        )
    }

    /// Count how many requests reference this library entry (to tell the user the impact before deleting / updating).
    fn template_usages(&self, template_id: &str) -> usize {
        self.request_owners()
            .keys()
            .filter(|id| {
                self.data.request(id).is_some_and(|req| {
                    req.pre_resolve_actions()
                        .iter()
                        .chain(req.pre_actions())
                        .chain(req.post_actions())
                        .any(|a| {
                            matches!(
                                a,
                                RequestAction::Ref { library_id, .. } if library_id == template_id
                            )
                        })
                })
            })
            .count()
    }

    /// `list_action_templates` response body: id / name / description / kind / full action / reference count.
    fn action_templates_json(&self) -> Value {
        let rows: Vec<Value> = self
            .action_templates()
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "description": t.description,
                    "kind": t.action.kind(),
                    "action": serde_json::to_value(&t.action).unwrap_or(Value::Null),
                    "usages": self.template_usages(&t.id),
                })
            })
            .collect();
        json!({ "templates": rows })
    }

    /// `list_data_sources` response body: **only id / name / kind**, never connection strings or credentials.
    fn data_sources_json(&self) -> Value {
        let rows: Vec<Value> = self
            .data
            .data_sources()
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "kind": c.kind.to_string(),
                })
            })
            .collect();
        json!({ "dataSources": rows })
    }

    // ─── Write-operation preparation (shared by preview and call) ────────────────

    fn prepare(&self, name: &str, args: &Value) -> AiResult<Option<Prepared>> {
        let ws = self.ws();
        let prepared = match name {
            "create_collection" => {
                let name_arg = vd::require_str(args, "name")?;
                let entity = vd::normalize_collection(
                    &json!({ "name": name_arg, "description": vd::opt_str(args, "description") }),
                    &ws,
                )
                .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "collection",
                    title: format!("Create collection: {}", entity.name),
                    target: format!("Workspace {}", self.workspace_name()),
                    action: ProposalAction::CreateCollection,
                    before: None,
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::CreateCollection(entity),
                }
            }
            "create_request" => {
                let collection_id = vd::require_str(args, "collectionId")?;
                let parent_id = vd::opt_str(args, "parentId");
                let raw = args
                    .get("request")
                    .ok_or_else(|| Self::tool_err(name, "missing request argument"))?;
                let entity = vd::normalize_request_with_env(raw, &self.action_env())
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                let collection = self
                    .data
                    .collections_in(&ws)
                    .into_iter()
                    .find(|c| c.id == collection_id)
                    .ok_or_else(|| {
                        Self::tool_err(
                            name,
                            format!("collection {collection_id} does not exist; call list_collections first"),
                        )
                    })?;
                if let Some(pid) = parent_id.as_deref() {
                    if !collection_has_node(&collection.items, pid) {
                        return Err(Self::tool_err(
                            name,
                            format!("folder {pid} is not in that collection"),
                        ));
                    }
                }
                Prepared {
                    scope: "request",
                    title: format!("Create request: {}", entity.name()),
                    target: format!("Collection {}", collection.name),
                    action: ProposalAction::CreateRequest {
                        collection_id: collection_id.clone(),
                        parent_id: parent_id.clone(),
                    },
                    before: None,
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::CreateRequest {
                        request: entity,
                        collection_id,
                        parent_id,
                    },
                }
            }
            "update_request" => {
                let request_id = vd::require_str(args, "requestId")?;
                let patch = args
                    .get("patch")
                    .ok_or_else(|| Self::tool_err(name, "missing patch argument"))?;
                let existing = self.data.request(&request_id).ok_or_else(|| {
                    Self::tool_err(
                        name,
                        format!(
                            "request {request_id} does not exist: the id must come from a list_requests / get_request result, \
                             or this turn's create_request summary; do not spell it from memory"
                        ),
                    )
                })?;
                let mut merged = serde_json::to_value(&existing)?;
                vd::merge_patch(&mut merged, patch);
                merged["id"] = json!(request_id);
                let entity = vd::normalize_request_with_env(&merged, &self.action_env())
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "request",
                    title: format!("Update request: {}", entity.name()),
                    target: format!("Request {}", existing.name()),
                    action: ProposalAction::UpdateRequest {
                        request_id: request_id.clone(),
                    },
                    before: Some(serde_json::to_value(&existing)?),
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::UpdateRequest(entity),
                }
            }
            // ── Action-level editing (a single action in the pre / post list) ──
            //
            // Why not let the model resend the whole preActions table: patch uses JSON Merge Patch, and arrays are **replaced wholesale**,
            // so one missing field loses content. Here the host reads the current action, edits it at the given index, then persists the whole thing.
            "update_action" | "insert_action" | "delete_action" | "move_action" => {
                let request_id = vd::require_str(args, "requestId")?;
                let list = vd::ActionList::parse(&vd::require_str(args, "list")?)?;
                let existing = self.data.request(&request_id).ok_or_else(|| {
                    Self::tool_err(
                        name,
                        format!(
                            "request {request_id} does not exist: the id must come from a list_requests / get_request result"
                        ),
                    )
                })?;
                let mut value = serde_json::to_value(&existing)?;
                // Index convention: matches the "execution order" list returned by get_request (including the built-in interpolation node)
                let field = list.field();
                let index_of = |key: &str| -> AiResult<usize> {
                    vd::require_index(args, key).map_err(|e| Self::tool_err(name, e.user_message()))
                };
                let target_of = |index: usize| value.get(field).and_then(|v| v.get(index)).cloned();

                let (op, target) = match name {
                    "update_action" => {
                        let index = index_of("index")?;
                        let patch = args
                            .get("patch")
                            .cloned()
                            .ok_or_else(|| Self::tool_err(name, "missing patch argument"))?;
                        (vd::ActionOp::Update { index, patch }, target_of(index))
                    }
                    "insert_action" => {
                        let raw = args
                            .get("action")
                            .cloned()
                            .ok_or_else(|| Self::tool_err(name, "missing action argument"))?;
                        // Normalize first (tolerating a bare string / missing type), while blocking "inserting the built-in interpolation node"
                        let action = vd::normalize_single_action(&raw)
                            .map_err(|e| Self::tool_err(name, e.user_message()))?;
                        (
                            vd::ActionOp::Insert {
                                index: vd::opt_u64(args, "index").map(|v| v as usize),
                                action: action.clone(),
                            },
                            Some(action),
                        )
                    }
                    "delete_action" => {
                        let index = index_of("index")?;
                        (vd::ActionOp::Delete { index }, target_of(index))
                    }
                    _ => {
                        let from = index_of("from")?;
                        let to = index_of("to")?;
                        (vd::ActionOp::Move { from, to }, target_of(from))
                    }
                };
                let what = target
                    .as_ref()
                    .map(vd::action_summary)
                    .unwrap_or_else(|| "action".to_string());
                let title = match name {
                    "update_action" => format!("Update {}: {what}", list.label()),
                    "insert_action" => format!("Insert {}: {what}", list.label()),
                    "delete_action" => format!("Delete {}: {what}", list.label()),
                    _ => format!("Move {}: {what}", list.label()),
                };
                vd::edit_actions(&mut value, list, op)
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                value["id"] = json!(request_id);
                let entity = vd::normalize_request_with_env(&value, &self.action_env())
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "request",
                    title,
                    target: format!("Request {}", existing.name()),
                    action: ProposalAction::UpdateRequest {
                        request_id: request_id.clone(),
                    },
                    before: Some(serde_json::to_value(&existing)?),
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::UpdateRequest(entity),
                }
            }
            // ── Script-library entries (workspace-level reusable actions) ──
            "save_action_template" => {
                let template_id = vd::opt_str(args, "templateId");
                let input = vd::normalize_action_template(
                    &vd::require_str(args, "name")?,
                    vd::opt_str(args, "description"),
                    args.get("action")
                        .ok_or_else(|| Self::tool_err(name, "missing action argument"))?,
                )
                .map_err(|e| Self::tool_err(name, e.user_message()))?;
                let templates = self.action_templates();
                let existing = template_id
                    .as_ref()
                    .and_then(|id| templates.iter().find(|t| &t.id == id).cloned());
                if let (Some(id), None) = (&template_id, &existing) {
                    return Err(Self::tool_err(
                        name,
                        format!("library entry {id} does not exist (use list_action_templates to get a real id)"),
                    ));
                }
                // On create append to the end (sort value = max + 1); on update keep it unchanged
                let sort_index = existing.as_ref().map(|t| t.sort_index).unwrap_or_else(|| {
                    templates
                        .iter()
                        .map(|t| t.sort_index + 1)
                        .max()
                        .unwrap_or(0)
                });
                let mut template = match &existing {
                    Some(t) => t.clone(),
                    None => ActionTemplate::new(
                        uuid::Uuid::new_v4().to_string(),
                        input.name.clone(),
                        input.action.clone(),
                    ),
                };
                template.name = input.name.clone();
                template.description = input.description.clone();
                template.action = input.action.clone();
                template.sort_index = sort_index;
                let verb = if existing.is_some() {
                    "Update"
                } else {
                    "Create"
                };
                Prepared {
                    scope: "actionlib",
                    title: format!("{verb} script-library entry: {}", template.name),
                    target: format!("Script library (workspace {})", self.workspace_name()),
                    action: ProposalAction::SaveActionTemplate {
                        template_id: template_id.clone(),
                    },
                    before: existing.as_ref().map(serde_json::to_value).transpose()?,
                    after: serde_json::to_value(&template)?,
                    op: WriteOp::SaveActionTemplate(ActionTemplateEntry::new(ws.clone(), template)),
                }
            }
            "delete_action_template" => {
                let id = vd::require_str(args, "templateId")?;
                let existing = self
                    .action_templates()
                    .into_iter()
                    .find(|t| t.id == id)
                    .ok_or_else(|| {
                        Self::tool_err(
                            name,
                            format!("library entry {id} does not exist (use list_action_templates to get a real id)"),
                        )
                    })?;
                let usages = self.template_usages(&id);
                Prepared {
                    scope: "actionlib",
                    title: format!("Delete script-library entry: {}", existing.name),
                    target: format!("Script library ({usages} requests referencing it)"),
                    action: ProposalAction::DeleteActionTemplate {
                        template_id: id.clone(),
                    },
                    before: Some(serde_json::to_value(&existing)?),
                    // "The whole thing is gone": the change card reports Removed per field, not a vague one-line "changed"
                    after: json!({}),
                    op: WriteOp::DeleteActionTemplate(id),
                }
            }
            "create_scenario_folder" => {
                let folder = vd::normalize_scenario_folder(
                    &json!({
                        "name": vd::require_str(args, "name")?,
                        "parentId": vd::opt_str(args, "parentId"),
                    }),
                    &ws,
                )
                .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "scenario",
                    title: format!("Create scenario folder: {}", folder.name),
                    target: "Automation scenario library".into(),
                    action: ProposalAction::CreateScenarioFolder,
                    before: None,
                    after: serde_json::to_value(&folder)?,
                    op: WriteOp::CreateScenarioFolder(folder),
                }
            }
            "create_scenario" => {
                let mut scenario_value = args
                    .get("scenario")
                    .cloned()
                    .ok_or_else(|| Self::tool_err(name, "missing scenario argument"))?;
                if let Some(folder) = vd::opt_str(args, "folderId") {
                    scenario_value["folderId"] = json!(folder);
                }
                let entity = vd::normalize_scenario(&scenario_value, &ws)
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                let count = entity.steps.len();
                Prepared {
                    scope: "scenario",
                    title: format!("Create scenario: {} ({} steps)", entity.name, count),
                    target: "Automation scenario library".into(),
                    action: ProposalAction::CreateScenario {
                        folder_id: entity.folder_id.clone(),
                    },
                    before: None,
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::CreateScenario(entity),
                }
            }
            "update_scenario" => {
                let scenario_id = vd::require_str(args, "scenarioId")?;
                let patch = args
                    .get("patch")
                    .ok_or_else(|| Self::tool_err(name, "missing patch argument"))?;
                let existing = self
                    .data
                    .scenarios_in(&ws)
                    .into_iter()
                    .find(|s| s.id == scenario_id)
                    .ok_or_else(|| {
                        Self::tool_err(
                            name,
                            format!(
                                "scenario {scenario_id} does not exist: the id must come from a list_scenarios result, \
                                 or this turn's create_scenario summary"
                            ),
                        )
                    })?;
                let mut merged = serde_json::to_value(&existing)?;
                vd::merge_patch(&mut merged, patch);
                merged["id"] = json!(scenario_id);
                let entity = vd::normalize_scenario(&merged, &ws)
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "scenario",
                    title: format!("Update scenario: {}", entity.name),
                    target: format!("Scenario {}", existing.name),
                    action: ProposalAction::UpdateScenario {
                        scenario_id: scenario_id.clone(),
                    },
                    before: Some(serde_json::to_value(&existing)?),
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::UpdateScenario(entity),
                }
            }
            "create_suite" => {
                let raw = args
                    .get("suite")
                    .cloned()
                    .ok_or_else(|| Self::tool_err(name, "missing suite argument"))?;
                let entity = vd::normalize_suite(&raw, &ws)
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "scenario",
                    title: format!(
                        "Create test suite: {} ({} scenarios)",
                        entity.name,
                        entity.member_ids.len()
                    ),
                    target: "Automation scenario library".into(),
                    action: ProposalAction::CreateSuite,
                    before: None,
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::CreateSuite(entity),
                }
            }
            "create_data_set" => {
                let raw = args
                    .get("dataSet")
                    .cloned()
                    .ok_or_else(|| Self::tool_err(name, "missing dataSet argument"))?;
                let entity = vd::normalize_data_set(&raw, &ws)
                    .map_err(|e| Self::tool_err(name, e.user_message()))?;
                Prepared {
                    scope: "scenario",
                    title: format!(
                        "Create data set: {} ({} rows)",
                        entity.name, entity.row_count
                    ),
                    target: "Automation scenario library".into(),
                    action: ProposalAction::CreateDataSet,
                    before: None,
                    after: serde_json::to_value(&entity)?,
                    op: WriteOp::CreateDataSet(entity),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(prepared))
    }

    fn workspace_name(&self) -> String {
        self.data
            .workspaces()
            .into_iter()
            .find(|w: &Workspace| w.id == self.workspace_id)
            .map(|w| w.name)
            .unwrap_or_else(|| self.workspace_id.clone())
    }

    /// Persist.
    async fn apply(&self, op: &WriteOp) -> AiResult<()> {
        match op {
            WriteOp::CreateCollection(c) => self
                .data
                .upsert_collection(c.clone())
                .await
                .map_err(db_err)?,
            WriteOp::CreateRequest {
                request,
                collection_id,
                parent_id,
            } => {
                self.data
                    .upsert_request(request.clone())
                    .await
                    .map_err(db_err)?;
                let ws = self.ws();
                let mut collections = self.data.collections_in(&ws);
                if let Some(c) = collections.iter_mut().find(|c| c.id == *collection_id) {
                    let inserted =
                        insert_request_node(&mut c.items, request.id(), parent_id.as_deref());
                    if !inserted {
                        tracing::warn!(
                            target: "orbit_ai",
                            "request {} was written but could not be attached to collection {} (parent node {:?} not found)",
                            request.id(),
                            collection_id,
                            parent_id
                        );
                    }
                    self.data
                        .upsert_collection(c.clone())
                        .await
                        .map_err(db_err)?;
                }
            }
            WriteOp::UpdateRequest(r) => {
                self.data.upsert_request(r.clone()).await.map_err(db_err)?
            }
            WriteOp::SaveActionTemplate(entry) => self
                .data
                .upsert_action_template(entry.clone())
                .await
                .map_err(db_err)?,
            WriteOp::DeleteActionTemplate(id) => {
                self.data.remove_action_template(id).await.map_err(db_err)?
            }
            WriteOp::CreateScenarioFolder(f) => self
                .data
                .upsert_scenario_folder(f.clone())
                .await
                .map_err(db_err)?,
            WriteOp::CreateScenario(s) | WriteOp::UpdateScenario(s) => {
                self.data.upsert_scenario(s.clone()).await.map_err(db_err)?
            }
            WriteOp::CreateSuite(s) => self
                .data
                .upsert_scenario_suite(s.clone())
                .await
                .map_err(db_err)?,
            WriteOp::CreateDataSet(d) => self
                .data
                .upsert_scenario_data_set(d.clone())
                .await
                .map_err(db_err)?,
        }
        Ok(())
    }

    // ─── Execution ───────────────────────────────────────────

    fn env_vars(&self, environment_id: Option<&str>) -> HashMap<String, String> {
        let ws = self.ws();
        // Merge order: global vars < environment vars < environment secrets (secrets only participate in interpolation and never enter the prompt)
        let mut vars = self.data.global_variables_in(&ws);
        let env_id = environment_id
            .map(str::to_string)
            .or_else(|| self.data.active_env_in(&ws));
        if let Some(env) = env_id.and_then(|id| {
            self.data
                .environments_in(&ws)
                .into_iter()
                .find(|e| e.id == id)
        }) {
            vars.extend(env.variables.clone());
            vars.extend(env.secrets.clone());
        }
        let mut globals = self.data.global_secrets_in(&ws);
        globals.extend(vars);
        globals
    }

    /// Actually send an HTTP request (through the unified pipeline: interpolation + scripts + assertions).
    async fn run_http_request(
        &self,
        http: &HttpRequest,
        environment_id: Option<&str>,
        cancel: &CancellationToken,
    ) -> AiResult<Value> {
        let mut headers = ai_plan::resolve_request_headers(http);
        // Text bodies (json / xml / raw) use `body_value` (an un-interpolated template) so the engine can, during interpolation,
        // replace the `{{var}}` placeholders, matching the frontend single-send / scenario paths.
        // Structured bodies (urlencoded / form-data) reuse bytes already assembled by the caller - their placeholders
        // are currently not interpolated (a known limitation, pending a unified move to request templates).
        let mode_key = ai_plan::body_mode_key(http.body_mode);
        let (payload, body_value) = if matches!(mode_key, "json" | "xml" | "raw") {
            (
                Vec::new(),
                Some(serde_yaml::Value::String(ai_plan::active_body(http))),
            )
        } else {
            let (payload, extra_ct) = ai_plan::resolve_request_body(http)
                .map_err(|m| Self::tool_err("run_request", m))?;
            if let Some(ct) = extra_ct {
                headers.insert("Content-Type".into(), ct);
            }
            (payload, None)
        };
        let env_vars = self.env_vars(environment_id);
        // Script-library table: AI tool execution has no frontend-supplied payload, so it is taken from the host's snapshot for the current workspace
        // (symmetric with the data-source registry); a missing entry degrades to an error log without breaking the request.
        let library = self.data.action_templates_in(&self.ws());
        let spec = orbit_engine::pipeline::PipelineSpec {
            protocol: detect_protocol(&http.url).to_string(),
            target: ai_plan::resolve_request_url(http),
            operation: http.method.clone(),
            headers,
            body: payload,
            body_value,
            timeout: Some(std::time::Duration::from_secs(30)),
            // Pre-actions: a single ordered list (including the built-in interpolation node); the previous "pre-interpolation actions" are merged before the anchor
            pre_actions: orbit_engine::pipeline::actions_to_pipeline(
                &orbit_config::merge_pre_actions(
                    &http.pre_actions,
                    None,
                    &http.pre_resolve_actions,
                    None,
                ),
                &library,
            ),
            post_actions: orbit_engine::pipeline::actions_to_pipeline(&http.post_actions, &library),
            pre_scripts: http
                .prereq_script
                .clone()
                .filter(|s| !s.trim().is_empty())
                .into_iter()
                .collect(),
            post_scripts: http
                .postreq_script
                .clone()
                .filter(|s| !s.trim().is_empty())
                .into_iter()
                .collect(),
            checks: http.assertions.clone(),
            // Key point: variable interpolation is left to the engine (same source as flow_runner); neither the frontend nor the tool side pre-replaces
            interpolate: true,
            ..Default::default()
        };
        let mut rt = orbit_engine::pipeline::PipelineRuntime::new(
            Box::new(orbit_protocol::http::HttpClient::new()),
            Box::new(orbit_codec::json::JsonCodec),
        );
        rt.with_datasources(Some(self.data_sources.clone()));
        let mut vars: HashMap<String, String> = HashMap::new();
        let outcome = {
            let mut jar = self.cookie_jar.lock().await;
            orbit_engine::pipeline::execute_pipeline(
                &mut rt,
                spec,
                &mut vars,
                &env_vars,
                cancel,
                Some(&mut jar),
            )
            .await
        };
        Ok(self.summarize_pipeline(&outcome))
    }

    fn summarize_pipeline(&self, outcome: &orbit_engine::pipeline::PipelineOutcome) -> Value {
        let failed: Vec<Value> = outcome
            .tests
            .iter()
            .filter(|t| !t.passed)
            .map(|t| json!({ "name": t.name, "message": t.message }))
            .collect();
        match &outcome.response {
            Some(resp) => {
                let body = String::from_utf8_lossy(&resp.payload).to_string();
                let mut headers: Vec<(String, String)> = resp.headers.clone().into_iter().collect();
                headers.sort();
                json!({
                    "status": resp.status_code,
                    "durationMs": resp.duration_ms,
                    "sizeBytes": resp.payload.len(),
                    "headers": headers.into_iter().take(20).collect::<Vec<_>>(),
                    "bodyExcerpt": vd::clip_text(&body, BODY_EXCERPT_CHARS),
                    "assertions": outcome.tests.iter().map(|t| json!({
                        "name": t.name,
                        "passed": t.passed,
                        "message": t.message,
                        "hard": t.is_hard,
                    })).collect::<Vec<_>>(),
                    "failedAssertions": failed,
                    "scriptLogs": outcome.post_logs.iter().take(20).map(|l| l.message.clone()).collect::<Vec<_>>(),
                    "varsSet": outcome.vars_set,
                })
            }
            None => json!({
                "error": outcome.error.as_ref().map(|e| e.message()),
                "failedAssertions": failed,
            }),
        }
    }

    /// Run an automation scenario (via LoadRunner, the same executor as the automation module).
    async fn run_scenario(&self, scenario_id: &str) -> AiResult<Value> {
        let ws = self.ws();
        let scenario = self
            .data
            .scenarios_in(&ws)
            .into_iter()
            .find(|s| s.id == scenario_id)
            .ok_or_else(|| {
                Self::tool_err(
                    "run_scenario",
                    format!("scenario {scenario_id} does not exist; call list_scenarios first"),
                )
            })?;
        let requests: HashMap<String, ApiRequest> = self.data.requests().into_iter().collect();
        let vars = self.env_vars(scenario.env_id.as_deref());
        let yaml = ai_plan::build_scenario_plan_yaml(&scenario, &requests, &vars)
            .map_err(|m| Self::tool_err("run_scenario", m))?;
        let plan = orbit_config::from_str(&yaml)
            .map_err(|e| Self::tool_err("run_scenario", format!("plan parse failed: {e}")))?;
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(64);
        // Step progress goes into the AI event buffer, so the frontend drawer shows which step the scenario is on
        let bus = self.bus.clone();
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                bus.push(orbit_ai::AiEvent::Progress { message: msg });
            }
        });
        let runner = orbit_server::load::LoadRunner::new();
        let result = runner
            .run_plan_with_events_opts(plan, 1, "10s", None, tx, false)
            .await
            .map_err(|e| Self::tool_err("run_scenario", e))?;
        Ok(result)
    }

    /// Run a load test (bounded: concurrency <= [`MAX_AI_VUS`], duration <= [`MAX_AI_DURATION_SECS`]).
    async fn run_load_test(
        &self,
        request_id: &str,
        vus: u32,
        duration_secs: u64,
        ramp_up_secs: u64,
    ) -> AiResult<Value> {
        let request = self.data.request(request_id).ok_or_else(|| {
            Self::tool_err(
                "run_load_test",
                format!("request {request_id} does not exist"),
            )
        })?;
        let ApiRequest::Http(http) = request else {
            return Err(Self::tool_err(
                "run_load_test",
                "load tests currently support HTTP requests only",
            ));
        };
        let vus = vus.clamp(1, MAX_AI_VUS);
        let duration = duration_secs.clamp(1, MAX_AI_DURATION_SECS);
        let ramp = ramp_up_secs.min(duration);
        let yaml = ai_plan::build_load_plan_yaml(&http, vus, duration, ramp)
            .map_err(|m| Self::tool_err("run_load_test", m))?;
        let plan = orbit_config::from_str(&yaml)
            .map_err(|e| Self::tool_err("run_load_test", format!("plan parse failed: {e}")))?;
        let runner = orbit_server::load::LoadRunner::new();
        let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
        runner
            .run_plan(
                plan,
                vus,
                &ai_plan::format_duration_ms(duration * 1000),
                Some(abort),
            )
            .await
            .map_err(|e| Self::tool_err("run_load_test", e))
    }
}

fn db_err(e: orbit_data::DataError) -> AiError {
    AiError::Tool {
        name: "data".into(),
        message: e.to_string(),
    }
}

/// Check whether the node id exists in the collection tree.
fn collection_has_node(items: &[CollectionItem], id: &str) -> bool {
    items.iter().any(|item| match item {
        CollectionItem::Folder { id: fid, items, .. } => {
            fid == id || collection_has_node(items, id)
        }
        other => other.id() == id,
    })
}

/// Insert a request node into the collection tree (`parent_id = None` -> root level). Returns whether the insert succeeded.
fn insert_request_node(
    items: &mut Vec<CollectionItem>,
    request_id: &str,
    parent_id: Option<&str>,
) -> bool {
    let node = CollectionItem::Request {
        id: format!("node-{}", uuid::Uuid::new_v4()),
        request_id: request_id.to_string(),
    };
    match parent_id {
        None => {
            items.push(node);
            true
        }
        Some(pid) => {
            for item in items.iter_mut() {
                if let CollectionItem::Folder { id, items, .. } = item {
                    if id == pid {
                        items.push(node);
                        return true;
                    }
                    if insert_request_node(items, request_id, Some(pid)) {
                        return true;
                    }
                }
            }
            false
        }
    }
}

/// Infer the protocol from the URL (consistent with `proxy.rs`).
pub fn detect_protocol(url: &str) -> &'static str {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        "websocket"
    } else if url.starts_with("tcp://") {
        "tcp"
    } else if url.starts_with("udp://") {
        "udp"
    } else if url.starts_with("sse://") {
        "sse"
    } else {
        "http"
    }
}

/// Summarize a load-test result into a model-readable sentence + structured payload.
pub fn summarize_load_result(value: &Value) -> (String, Value) {
    let summary = value.get("summary").cloned().unwrap_or(Value::Null);
    let get = |k: &str| summary.get(k).cloned().unwrap_or(Value::Null);
    if value.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = value
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("load test failed");
        return (format!("Load test failed: {msg}"), value.clone());
    }
    let text = format!(
        "Load test complete: {} requests, RPS {}, P95 {}ms, P99 {}ms, error rate {}%",
        get("total_requests"),
        get("rps"),
        get("p95_ms"),
        get("p99_ms"),
        get("error_rate"),
    );
    (text, value.clone())
}

/// Summarize a scenario run result into a model-readable sentence + structured payload.
pub fn summarize_scenario_result(value: &Value) -> (String, Value) {
    if value.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = value
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("run failed");
        return (format!("Scenario run failed: {msg}"), value.clone());
    }
    let cases = value
        .get("cases")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let passed = cases
        .iter()
        .filter(|c| c.get("status").and_then(|s| s.as_str()) == Some("passed"))
        .count();
    let detail: Vec<Value> = cases
        .iter()
        .take(50)
        .map(|c| {
            json!({
                "name": c.get("name"),
                "status": c.get("status"),
                "durationMs": c.get("durationMs"),
                "error": c.get("error"),
            })
        })
        .collect();
    let (text, payload) = if cases.is_empty() {
        // The engine returns aggregate counts (total_requests / total_failures), with no per-step detail
        let summary = value.get("summary").cloned().unwrap_or(Value::Null);
        let total = summary
            .get("total_requests")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let failures = summary
            .get("total_failures")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let thresholds = value.get("thresholds").cloned().unwrap_or(Value::Null);
        let all_passed = value
            .get("all_thresholds_passed")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        (
            if failures == 0 {
                format!("Scenario run complete: all {total} steps passed")
            } else {
                format!("Scenario run complete: {failures} of {total} steps failed")
            },
            json!({
                "passed": total.saturating_sub(failures),
                "failed": failures,
                "total": total,
                "summary": summary,
                "thresholds": thresholds,
                "allThresholdsPassed": all_passed,
            }),
        )
    } else {
        (
            format!(
                "Scenario run complete: {}/{} steps passed",
                passed,
                cases.len()
            ),
            json!({ "passed": passed, "total": cases.len(), "cases": detail }),
        )
    };
    (text, payload)
}

#[async_trait]
impl ToolHost for TauriToolHost {
    async fn call(&self, name: &str, args: &Value) -> AiResult<ToolOutcome> {
        match name {
            // ── Read-only ──
            "list_collections" => Ok(ToolOutcome::read(
                format!(
                    "{} collections in total",
                    self.data.collections_in(&self.ws()).len()
                ),
                self.collections_tree(),
            )),
            "list_requests" => {
                let data = self.list_requests(vd::opt_str(args, "collectionId").as_deref());
                let count = data
                    .get("requests")
                    .and_then(|r| r.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                Ok(ToolOutcome::read(
                    format!("{count} requests in total"),
                    data,
                ))
            }
            "get_request" => {
                let id = vd::require_str(args, "requestId")?;
                let req = self
                    .data
                    .request(&id)
                    .ok_or_else(|| Self::tool_err(name, format!("request {id} does not exist")))?;
                // Return an **execution-order** view: the action list is already normalized (the built-in interpolation node is in place, legacy single-script fields merged in),
                // so the indices the model computes from it line up exactly with update_action / delete_action and the like.
                let mut value = serde_json::to_value(&req)?;
                if let Some(obj) = value.as_object_mut() {
                    vd::canonicalize_actions(obj)
                        .map_err(|e| Self::tool_err(name, e.user_message()))?;
                }
                Ok(ToolOutcome::read(
                    format!("request {} ({})", req.name(), req.protocol()),
                    value,
                ))
            }
            "list_environments" => {
                let data = self.environments_json();
                Ok(ToolOutcome::read(
                    "Environment and variable-name list",
                    data,
                ))
            }
            "list_models" => Ok(ToolOutcome::read("Data model list", self.models_json())),
            "list_scenarios" => Ok(ToolOutcome::read(
                "Automation scenarios and suites list",
                self.scenarios_json(),
            )),
            "recent_history" => {
                let limit = vd::opt_u64(args, "limit").unwrap_or(20) as usize;
                Ok(ToolOutcome::read(
                    "Recent request history",
                    self.history_json(limit),
                ))
            }
            "list_reports" => {
                let limit = vd::opt_u64(args, "limit").unwrap_or(10) as usize;
                Ok(ToolOutcome::read(
                    "Recent run reports",
                    self.reports_json(limit),
                ))
            }
            "list_action_templates" => {
                let data = self.action_templates_json();
                let count = data
                    .get("templates")
                    .and_then(|t| t.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                Ok(ToolOutcome::read(
                    format!("{count} library entries in total"),
                    data,
                ))
            }
            "list_data_sources" => {
                let data = self.data_sources_json();
                let count = data
                    .get("dataSources")
                    .and_then(|t| t.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                Ok(ToolOutcome::read(
                    format!("{count} data sources in total"),
                    data,
                ))
            }
            // ── Planning (Plan mode only) ──
            "present_plan" => {
                let plan = PlanArtifact::from_args(args)?;
                let stamped = self.persist_plan(plan)?;
                let digest = stamped.digest();
                Ok(ToolOutcome::planned(digest, stamped))
            }
            // ── Writes (Agent mode persists directly) ──
            _ if orbit_ai::tools::catalog::WRITE_TOOLS.contains(&name) => {
                let prepared = self
                    .prepare(name, args)?
                    .ok_or_else(|| Self::tool_err(name, "unable to prepare write operation"))?;
                self.apply(&prepared.op).await?;
                self.notify_changed(prepared.scope);
                // Include the id in the summary: this is the **only** place the model can get the new object's id. Previously it only returned "Applied: Create request: Login",
                // so when the model wanted to edit that request next it had to fabricate an id from memory, causing cascading "request req-xxx does not exist" failures.
                let summary = match prepared.after.get("id").and_then(|v| v.as_str()) {
                    Some(id) if !id.is_empty() => {
                        format!("Applied: {} (id={id})", prepared.title)
                    }
                    _ => format!("Applied: {}", prepared.title),
                };
                let proposal = Proposal::new(
                    name,
                    prepared.title.clone(),
                    prepared.target.clone(),
                    prepared.action.clone(),
                    prepared.before.clone(),
                    prepared.after.clone(),
                );
                Ok(ToolOutcome {
                    ok: true,
                    summary,
                    payload: prepared.after,
                    proposal: Some(proposal),
                    plan: None,
                })
            }
            // ── Execution ──
            "run_request" => {
                let id = vd::require_str(args, "requestId")?;
                let env_id = vd::opt_str(args, "environmentId");
                let req = self
                    .data
                    .request(&id)
                    .ok_or_else(|| Self::tool_err(name, format!("request {id} does not exist")))?;
                let ApiRequest::Http(http) = req else {
                    return Err(Self::tool_err(
                        name,
                        "AI direct execution currently supports HTTP requests only; debug other protocols in the interface module",
                    ));
                };
                let payload = self
                    .run_http_request(&http, env_id.as_deref(), &CancellationToken::new())
                    .await?;
                let status = payload.get("status").and_then(|s| s.as_u64());
                let duration = payload.get("durationMs").and_then(|s| s.as_u64());
                let failed = payload
                    .get("failedAssertions")
                    .and_then(|f| f.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let summary = match (status, duration) {
                    (Some(s), Some(d)) => {
                        if failed > 0 {
                            format!("HTTP {s} · {d}ms · {failed} assertions failed")
                        } else {
                            format!("HTTP {s} · {d}ms · all assertions passed")
                        }
                    }
                    _ => payload
                        .get("error")
                        .and_then(|e| e.as_str())
                        .map(|e| format!("Request failed: {e}"))
                        .unwrap_or_else(|| "request executed".into()),
                };
                Ok(ToolOutcome {
                    ok: payload.get("error").is_none(),
                    summary,
                    payload,
                    proposal: None,
                    plan: None,
                })
            }
            "run_scenario" => {
                let id = vd::require_str(args, "scenarioId")?;
                let raw = self.run_scenario(&id).await?;
                let (summary, payload) = summarize_scenario_result(&raw);
                Ok(ToolOutcome {
                    ok: payload.get("error").is_none(),
                    summary,
                    payload,
                    proposal: None,
                    plan: None,
                })
            }
            "run_load_test" => {
                let id = vd::require_str(args, "requestId")?;
                let vus = vd::opt_u64(args, "vus").unwrap_or(1) as u32;
                let duration = vd::opt_u64(args, "durationSec").unwrap_or(10);
                let ramp = vd::opt_u64(args, "rampUpSec").unwrap_or(0);
                let raw = self.run_load_test(&id, vus, duration, ramp).await?;
                let (summary, payload) = summarize_load_result(&raw);
                Ok(ToolOutcome {
                    ok: payload.get("error").is_none(),
                    summary,
                    payload,
                    proposal: None,
                    plan: None,
                })
            }
            other => Err(AiError::UnknownTool(other.to_string())),
        }
    }
}

impl TauriToolHost {
    /// Write the plan into the session file (the only persist action in Plan mode).
    ///
    /// The plan must travel with the session: later model turns see tool-result summaries, while the UI sees `session.plan`;
    /// both share one source - after refreshing the drawer or reopening the app the plan is still there, and the user can click "Start implementing" at any time.
    /// Revision semantics: calling `present_plan` repeatedly within one session increments `revision` while `created_at` is preserved.
    fn persist_plan(&self, plan: PlanArtifact) -> AiResult<PlanArtifact> {
        let mut session = self.sessions.load(&self.session_id)?;
        let (revision, created_at) = match &session.plan {
            Some(prev) => (prev.revision.saturating_add(1), prev.created_at),
            None => (1, 0),
        };
        let stamped = plan.stamped(
            revision,
            created_at,
            jiff::Timestamp::now().as_millisecond(),
        );
        session.plan = Some(stamped.clone());
        session.updated_at = jiff::Timestamp::now().as_millisecond();
        self.sessions.save(&session)?;
        Ok(stamped)
    }
}
