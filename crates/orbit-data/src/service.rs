//! Data service: snapshot lifecycle management + domain CRUD + import/export entry point.
//!
//! [`DataService`] is the single entry point for data management (shared by Tauri commands / server API / WASM):
//! - On open it loads the snapshot from [`Storage`] and validates the version; with no snapshot it builds default data and persists it
//! - Writes mutate the in-memory snapshot and persist immediately (callers may batch: several upserts, then one `save_now`)
//!
//! Export orchestration (openapi/swagger/postman etc.) reads data directly in this layer → `orbit-config::exchange`,
//!   the frontend only passes the "export range" and no longer assembles an ApiSpec

use std::sync::RwLock;

use crate::error::DataError;
use crate::model::{
    ActionTemplateEntry, ApiRequest, Collection, DataModel, Environment, PersistedData, Scenario,
    Snapshot,
};
use crate::storage::Storage;

/// Data service (generic storage backend)
pub struct DataService<S: Storage> {
    storage: S,
    snapshot: RwLock<Snapshot>,
    /// Highest snapshot version this build supports (forward compatible: higher versions are refused)
    max_schema_version: u32,
    /// Whether storage already held a snapshot at startup (false = first launch; the caller seeds data and pushes the first copy)
    had_initial: std::sync::atomic::AtomicBool,
}

impl<S: Storage> DataService<S> {
    /// Opens the data service: load snapshot (version check) → in-memory state.
    /// With no stored snapshot it builds a default in-memory snapshot (**not persisted**, `had_initial() == false`,
    /// and the caller seeds data and then pushes the first copy via [`Self::load_from_json`]).
    pub async fn open(storage: S, max_schema_version: u32) -> Result<Self, DataError> {
        let (snapshot, had_initial) = match storage.load().await? {
            Some(snap) => {
                if snap.schema_version > max_schema_version {
                    return Err(DataError::VersionMismatch {
                        expected: max_schema_version,
                        found: snap.schema_version,
                    });
                }
                (normalize_snapshot(snap, max_schema_version), true)
            }
            None => (default_snapshot(max_schema_version), false),
        };
        Ok(DataService {
            storage,
            snapshot: RwLock::new(snapshot),
            max_schema_version,
            had_initial: std::sync::atomic::AtomicBool::new(had_initial),
        })
    }

    /// Current snapshot (cloned, for serialization / export)
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.read().unwrap().clone()
    }

    /// Whether storage already held a snapshot at startup (false = first launch)
    pub fn had_initial(&self) -> bool {
        self.had_initial.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Serializes the current snapshot to JSON (frontend pull / export)
    pub fn to_json(&self) -> Result<String, DataError> {
        Ok(serde_json::to_string(&self.snapshot())?)
    }

    /// Loads a snapshot from a JSON string (frontend push): deserialize + version check + replace in memory + persist.
    /// After the replacement `had_initial` becomes true (authoritative data now exists).
    pub async fn load_from_json(&self, json: &str) -> Result<(), DataError> {
        let snap = deserialize_snapshot(json, self.max_schema_version)?;
        self.replace_snapshot(snap).await
    }

    /// Load with optimistic locking (frontend push): `base_saved_at` is the latest save time seen by the pusher;
    /// rejected when the local (backend) snapshot is newer than the base (guards against multi-window / future multi-device overwrites).
    pub async fn load_from_json_checked(
        &self,
        json: &str,
        base_saved_at: i64,
    ) -> Result<(), DataError> {
        let snap = deserialize_snapshot(json, self.max_schema_version)?;
        {
            let cur = self.snapshot.read().unwrap();
            // First launch (no stored data) skips the conflict check; with stored data, a local snapshot newer than the base is rejected
            if self.had_initial() && cur.saved_at > base_saved_at {
                return Err(DataError::Conflict {
                    local: cur.saved_at,
                    base: base_saved_at,
                });
            }
        }
        self.replace_snapshot(snap).await
    }

    async fn replace_snapshot(&self, snap: Snapshot) -> Result<(), DataError> {
        {
            let mut cur = self.snapshot.write().unwrap();
            *cur = snap;
            self.had_initial
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.save_now().await
    }

    /// Data view (cloned)
    pub fn data(&self) -> PersistedData {
        self.snapshot().data
    }

    /// Writes the in-memory snapshot to storage immediately
    pub async fn save_now(&self) -> Result<(), DataError> {
        let snapshot = self.snapshot();
        self.storage.save(&snapshot).await
    }

    /// Clears storage (in-memory state kept; the caller should then call replace_data to build new data)
    pub async fn clear_storage(&self) -> Result<(), DataError> {
        self.storage.clear().await
    }

    /// Replaces all data (import a snapshot / reset back to seed)
    pub async fn replace_data(&self, data: PersistedData) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data = data;
            snap.data.ensure_migrated();
            snap.schema_version = self.max_schema_version;
            snap.saved_at = now_ts();
        }
        self.save_now().await
    }

    /// Current maximum supported version
    pub fn max_schema_version(&self) -> u32 {
        self.max_schema_version
    }

    /// Reference to the storage backend (for rebuilding the service / migration)
    pub fn storage(&self) -> &S {
        &self.storage
    }

    // ─── Workspace ────────────────────────────────────────

    pub fn workspaces(&self) -> Vec<crate::model::Workspace> {
        self.snapshot().data.workspaces
    }

    pub fn active_workspace_id(&self) -> Option<String> {
        self.snapshot().data.active_workspace_id
    }

    pub async fn add_workspace(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
    ) -> Result<crate::model::Workspace, DataError> {
        let ws = {
            let mut snap = self.snapshot.write().unwrap();
            let ws = crate::model::Workspace {
                id: format!("ws-{}", uuid_like()),
                name: name.trim().to_string(),
                description: description.map(str::to_string),
                color: Some(color.unwrap_or("#0ea5e9").to_string()),
                created_at: now_ts(),
                sort_index: snap.data.workspaces.len() as i32,
            };
            snap.data.workspaces.push(ws.clone());
            ws
        };
        self.save_now().await?;
        Ok(ws)
    }

    pub async fn rename_workspace(&self, id: &str, name: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(ws) = snap.data.workspaces.iter_mut().find(|w| w.id == id) {
                ws.name = name.trim().to_string();
            }
        }
        self.save_now().await
    }

    /// Removes a workspace (cascading cleanup of all its data: collections / models / environments / scenarios / history / mocks / state)
    pub async fn remove_workspace(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.workspaces.retain(|w| w.id != id);
            snap.data.collections.retain(|c| c.workspace_id != id);
            snap.data.models.retain(|m| m.workspace_id != id);
            snap.data.environments.retain(|e| e.workspace_id != id);
            snap.data.action_templates.retain(|t| t.workspace_id != id);
            snap.data.scenarios.retain(|s| s.workspace_id != id);
            snap.data.scenario_folders.retain(|f| f.workspace_id != id);
            snap.data
                .scenario_data_sets
                .retain(|d| d.workspace_id != id);
            snap.data.scenario_suites.retain(|s| s.workspace_id != id);
            snap.data.history.retain(|h| h.workspace_id != id);
            snap.data.mock_rules.retain(|m| m.workspace_id != id);
            snap.data.active_env_by_workspace.remove(id);
            snap.data.global_variables_by_workspace.remove(id);
            snap.data.global_secrets_by_workspace.remove(id);
            if snap.data.active_workspace_id.as_deref() == Some(id) {
                snap.data.active_workspace_id = snap.data.workspaces.first().map(|w| w.id.clone());
            }
        }
        self.save_now().await
    }

    pub async fn set_active_workspace(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.active_workspace_id = Some(id.to_string());
        }
        self.save_now().await
    }

    /// Workspace data stats (card display: collections / models / environments / scenarios / history)
    pub fn workspace_stats(&self, id: &str) -> crate::model::WorkspaceStats {
        let data = self.snapshot().data;
        crate::model::WorkspaceStats {
            collections: data
                .collections
                .iter()
                .filter(|c| c.workspace_id == id)
                .count(),
            models: data.models.iter().filter(|m| m.workspace_id == id).count(),
            environments: data
                .environments
                .iter()
                .filter(|e| e.workspace_id == id)
                .count(),
            action_templates: data
                .action_templates
                .iter()
                .filter(|t| t.workspace_id == id)
                .count(),
            scenarios: data
                .scenarios
                .iter()
                .filter(|s| s.workspace_id == id)
                .count(),
            history: data.history.iter().filter(|h| h.workspace_id == id).count(),
        }
    }

    // ─── Domain queries (per workspace) ──────────────────────────

    pub fn collections_in(&self, ws_id: &str) -> Vec<Collection> {
        self.snapshot()
            .data
            .collections
            .into_iter()
            .filter(|c| c.workspace_id == ws_id)
            .collect()
    }

    pub fn requests(&self) -> Vec<(String, ApiRequest)> {
        self.snapshot()
            .data
            .requests
            .into_iter()
            .collect::<Vec<_>>()
    }

    pub fn request(&self, id: &str) -> Option<ApiRequest> {
        self.snapshot().data.requests.get(id).cloned()
    }

    pub fn models_in(&self, ws_id: &str) -> Vec<DataModel> {
        self.snapshot()
            .data
            .models
            .into_iter()
            .filter(|m| m.workspace_id == ws_id)
            .collect()
    }

    pub fn environments_in(&self, ws_id: &str) -> Vec<Environment> {
        self.snapshot()
            .data
            .environments
            .into_iter()
            .filter(|e| e.workspace_id == ws_id)
            .collect()
    }

    /// Script library entries in the workspace (**workspace ownership stripped**, for direct use by the engine / export).
    pub fn action_templates_in(&self, ws_id: &str) -> Vec<orbit_config::ActionTemplate> {
        self.snapshot()
            .data
            .action_templates
            .into_iter()
            .filter(|t| t.workspace_id == ws_id)
            .map(|t| t.template)
            .collect()
    }

    pub fn scenarios_in(&self, ws_id: &str) -> Vec<Scenario> {
        self.snapshot()
            .data
            .scenarios
            .into_iter()
            .filter(|s| s.workspace_id == ws_id)
            .collect()
    }

    pub fn mock_rules_in(&self, ws_id: &str) -> Vec<crate::model::MockInterface> {
        self.snapshot()
            .data
            .mock_rules
            .into_iter()
            .filter(|m| m.workspace_id == ws_id)
            .collect()
    }

    pub fn history_in(&self, ws_id: &str) -> Vec<crate::model::PersistedHistoryEntry> {
        self.snapshot()
            .data
            .history
            .into_iter()
            .filter(|h| h.workspace_id == ws_id)
            .collect()
    }

    pub fn global_variables_in(&self, ws_id: &str) -> std::collections::HashMap<String, String> {
        self.snapshot()
            .data
            .global_variables_by_workspace
            .get(ws_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn global_secrets_in(&self, ws_id: &str) -> std::collections::HashMap<String, String> {
        self.snapshot()
            .data
            .global_secrets_by_workspace
            .get(ws_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn active_env_in(&self, ws_id: &str) -> Option<String> {
        self.snapshot()
            .data
            .active_env_by_workspace
            .get(ws_id)
            .cloned()
            .flatten()
    }

    pub async fn set_active_env(&self, ws_id: &str, id: Option<String>) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data
                .active_env_by_workspace
                .insert(ws_id.to_string(), id);
        }
        self.save_now().await
    }

    pub async fn set_global_variables(
        &self,
        ws_id: &str,
        vars: std::collections::HashMap<String, String>,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data
                .global_variables_by_workspace
                .insert(ws_id.to_string(), vars);
        }
        self.save_now().await
    }

    pub async fn set_global_secrets(
        &self,
        ws_id: &str,
        secrets: std::collections::HashMap<String, String>,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data
                .global_secrets_by_workspace
                .insert(ws_id.to_string(), secrets);
        }
        self.save_now().await
    }

    // ─── Writes (upsert / remove; call save_now afterwards to persist) ────

    pub async fn upsert_collection(&self, collection: Collection) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap
                .data
                .collections
                .iter_mut()
                .find(|c| c.id == collection.id)
            {
                *existing = collection;
            } else {
                snap.data.collections.push(collection);
            }
        }
        self.save_now().await
    }

    pub async fn remove_collection(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.collections.retain(|c| c.id != id);
        }
        self.save_now().await
    }

    pub async fn upsert_request(&self, request: ApiRequest) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            // Old clients may still send the previous version's "pre-resolution actions" field: merge into the single list before persisting (idempotent)
            let mut request = request;
            request.merge_pre_resolve_actions();
            snap.data.requests.insert(request.id().to_string(), request);
        }
        self.save_now().await
    }

    pub async fn remove_request(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.requests.remove(id);
        }
        self.save_now().await
    }

    pub async fn upsert_model(&self, model: DataModel) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap.data.models.iter_mut().find(|m| m.id == model.id) {
                *existing = model;
            } else {
                snap.data.models.push(model);
            }
        }
        self.save_now().await
    }

    pub async fn remove_model(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.models.retain(|m| m.id != id);
        }
        self.save_now().await
    }

    pub async fn upsert_environment(&self, env: Environment) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap.data.environments.iter_mut().find(|e| e.id == env.id) {
                *existing = env;
            } else {
                snap.data.environments.push(env);
            }
        }
        self.save_now().await
    }

    pub async fn remove_environment(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.environments.retain(|e| e.id != id);
            // Clear the active-environment memory for this env across all workspaces
            for v in snap.data.active_env_by_workspace.values_mut() {
                if v.as_deref() == Some(id) {
                    *v = None;
                }
            }
        }
        self.save_now().await
    }

    /// Adds / updates a script library entry (writes it into the snapshot and persists).
    pub async fn upsert_action_template(
        &self,
        entry: ActionTemplateEntry,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap
                .data
                .action_templates
                .iter_mut()
                .find(|t| t.template.id == entry.template.id)
            {
                *existing = entry;
            } else {
                snap.data.action_templates.push(entry);
            }
        }
        self.save_now().await
    }

    /// Removes a script library entry.
    ///
    /// **References inside requests are deliberately left alone**: the remaining `type: ref` is a **dangling reference** that the editor
    /// surfaces as "re-select / convert to a copy", and the execution layer logs an error — silent cleanup would make
    /// users silently lose their own orchestration intent (unlike the reference-cleaning policy of `remove_data_source`,
    /// because a deleted action cannot be expressed by an equivalent empty action).
    pub async fn remove_action_template(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.action_templates.retain(|t| t.template.id != id);
        }
        self.save_now().await
    }

    /// Adds/updates a data source config (writes it into the snapshot and persists).
    pub async fn upsert_data_source(
        &self,
        cfg: orbit_config::DataSourceConfig,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap.data.data_sources.iter_mut().find(|d| d.id == cfg.id) {
                *existing = cfg;
            } else {
                snap.data.data_sources.push(cfg);
            }
        }
        self.save_now().await
    }

    /// Removes a data source config.
    ///
    /// Also strips the dangling references to it from every request's action lists (otherwise "data source not found" only surfaces at execution time).
    pub async fn remove_data_source(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.data_sources.retain(|d| d.id != id);
            for request in snap.data.requests.values_mut() {
                for actions in request.actions_mut() {
                    actions.retain(|action| {
                        !matches!(
                            action,
                            orbit_config::RequestAction::Db { datasource, .. }
                                if datasource == id
                        )
                    });
                }
            }
        }
        self.save_now().await
    }

    /// Reads all data source configs (for runtime registration).
    pub fn data_sources(&self) -> Vec<orbit_config::DataSourceConfig> {
        self.snapshot().data.data_sources
    }

    pub async fn upsert_scenario(&self, scenario: Scenario) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap.data.scenarios.iter_mut().find(|s| s.id == scenario.id) {
                *existing = scenario;
            } else {
                snap.data.scenarios.push(scenario);
            }
        }
        self.save_now().await
    }

    pub async fn remove_scenario(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.scenarios.retain(|s| s.id != id);
        }
        self.save_now().await
    }

    // ─── Scenario folders / data sets / suites (fine-grained writes needed for AI-generated scenarios) ──

    /// Reads the scenario folders of a workspace.
    pub fn scenario_folders_in(&self, ws_id: &str) -> Vec<crate::model::ScenarioFolder> {
        self.snapshot()
            .data
            .scenario_folders
            .into_iter()
            .filter(|f| f.workspace_id == ws_id)
            .collect()
    }

    pub async fn upsert_scenario_folder(
        &self,
        folder: crate::model::ScenarioFolder,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap
                .data
                .scenario_folders
                .iter_mut()
                .find(|f| f.id == folder.id)
            {
                *existing = folder;
            } else {
                snap.data.scenario_folders.push(folder);
            }
        }
        self.save_now().await
    }

    /// Removes a scenario folder; the scenarios under it and the deleted folder's children are **promoted to the parent folder** (no data loss).
    pub async fn remove_scenario_folder(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            let parent = snap
                .data
                .scenario_folders
                .iter()
                .find(|f| f.id == id)
                .and_then(|f| f.parent_id.clone());
            snap.data.scenario_folders.retain(|f| f.id != id);
            for folder in snap.data.scenario_folders.iter_mut() {
                if folder.parent_id.as_deref() == Some(id) {
                    folder.parent_id = parent.clone();
                }
            }
            for scenario in snap.data.scenarios.iter_mut() {
                if scenario.folder_id.as_deref() == Some(id) {
                    scenario.folder_id = parent.clone();
                }
            }
        }
        self.save_now().await
    }

    /// Reads the CSV data sets of a workspace.
    pub fn scenario_data_sets_in(&self, ws_id: &str) -> Vec<crate::model::ScenarioDataSet> {
        self.snapshot()
            .data
            .scenario_data_sets
            .into_iter()
            .filter(|d| d.workspace_id == ws_id)
            .collect()
    }

    pub async fn upsert_scenario_data_set(
        &self,
        data_set: crate::model::ScenarioDataSet,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap
                .data
                .scenario_data_sets
                .iter_mut()
                .find(|d| d.id == data_set.id)
            {
                *existing = data_set;
            } else {
                snap.data.scenario_data_sets.push(data_set);
            }
        }
        self.save_now().await
    }

    /// Removes a data set and unbinds the scenarios referencing it (`data_set_id` cleared, data-driven mode turned off).
    pub async fn remove_scenario_data_set(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.scenario_data_sets.retain(|d| d.id != id);
            for scenario in snap.data.scenarios.iter_mut() {
                if scenario.data_set_id.as_deref() == Some(id) {
                    scenario.data_set_id = None;
                    scenario.use_data_set = Some(false);
                }
            }
        }
        self.save_now().await
    }

    /// Reads the test suites of a workspace.
    pub fn scenario_suites_in(&self, ws_id: &str) -> Vec<crate::model::TestSuite> {
        self.snapshot()
            .data
            .scenario_suites
            .into_iter()
            .filter(|s| s.workspace_id == ws_id)
            .collect()
    }

    pub async fn upsert_scenario_suite(
        &self,
        suite: crate::model::TestSuite,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            if let Some(existing) = snap
                .data
                .scenario_suites
                .iter_mut()
                .find(|s| s.id == suite.id)
            {
                *existing = suite;
            } else {
                snap.data.scenario_suites.push(suite);
            }
        }
        self.save_now().await
    }

    pub async fn remove_scenario_suite(&self, id: &str) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.scenario_suites.retain(|s| s.id != id);
        }
        self.save_now().await
    }

    // ─── AI preferences (app-level) ────────────────────────────────

    /// Reads AI preferences.
    pub fn ai_prefs(&self) -> crate::model::AiPrefs {
        self.snapshot().data.ai
    }

    /// Overwrites AI preferences (the semantics are a **full replacement**; the caller supplies the complete object).
    pub async fn set_ai_prefs(&self, prefs: crate::model::AiPrefs) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.ai = prefs;
        }
        self.save_now().await
    }

    // ─── History ──────────────────

    pub fn history(&self) -> Vec<crate::model::PersistedHistoryEntry> {
        self.snapshot().data.history
    }

    /// Appends a history entry (inserted at the head; the caller is responsible for truncation)
    pub async fn add_history(
        &self,
        entry: crate::model::PersistedHistoryEntry,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.history.insert(0, entry);
        }
        self.save_now().await
    }

    pub async fn clear_history(&self) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.history.clear();
        }
        self.save_now().await
    }

    pub async fn set_history(
        &self,
        entries: Vec<crate::model::PersistedHistoryEntry>,
    ) -> Result<(), DataError> {
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.data.history = entries;
        }
        self.save_now().await
    }
}

/// Deserializes a snapshot and runs the v1 → v2 migration (the version check belongs to the caller)
fn deserialize_snapshot(json: &str, max_schema_version: u32) -> Result<Snapshot, DataError> {
    let snap: Snapshot = serde_json::from_str(json)?;
    if snap.schema_version > max_schema_version {
        return Err(DataError::VersionMismatch {
            expected: max_schema_version,
            found: snap.schema_version,
        });
    }
    Ok(normalize_snapshot(snap, max_schema_version))
}

/// Migration normalization: fill in workspaces + bump schema_version to the current version
fn normalize_snapshot(mut snap: Snapshot, max_schema_version: u32) -> Snapshot {
    snap.data.ensure_migrated();
    // Back-compat normalization: the previous two-phase "pre-resolution actions" merge into the single list (idempotent; the field is no longer written out after merging)
    for request in snap.data.requests.values_mut() {
        request.merge_pre_resolve_actions();
    }
    snap.schema_version = max_schema_version;
    snap
}

/// Short random id (workspaces etc.)
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!("{:x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn default_snapshot(max_schema_version: u32) -> Snapshot {
    Snapshot {
        schema_version: max_schema_version,
        saved_at: now_ts(),
        source: "local".into(),
        sync: crate::model::SyncInfo {
            remote_url: None,
            last_synced_at: None,
        },
        data: PersistedData::default(),
    }
}
