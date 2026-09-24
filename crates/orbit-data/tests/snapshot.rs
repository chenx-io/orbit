//! Integration tests: snapshot compatibility (frontend-shaped JSON), roundtrip, version check, corruption fallback, CRUD.

use orbit_data::model::ApiRequest;
use orbit_data::{DataService, FileStorage, MemoryStorage, Snapshot, Storage, SNAPSHOT_VERSION};

/// The snapshot shape actually emitted by the frontend persistence service (web/src/lib/persistence.ts):
/// camelCase fields, HTTP requests omitting protocol, multi-protocol discrimination, tagged collection tree and scenario steps.
const FRONTEND_SNAPSHOT: &str = r#"{
  "schemaVersion": 1,
  "savedAt": 1780000000000,
  "source": "tauri",
  "sync": { "remoteUrl": null, "lastSyncedAt": null },
  "data": {
    "collections": [
      {
        "id": "col-1",
        "name": "Demo",
        "items": [
          {
            "type": "folder",
            "id": "f1",
            "name": "Users",
            "items": [
              { "type": "request", "id": "i2", "requestId": "req-1" }
            ]
          },
          {
            "type": "grpc-package",
            "id": "p1",
            "name": "pkg",
            "services": [
              {
                "name": "Svc",
                "methods": [
                  { "name": "Rpc", "inputType": ".pkg.Req", "outputType": ".pkg.Res",
                    "clientStreaming": false, "serverStreaming": true }
                ]
              }
            ]
          }
        ]
      }
    ],
    "requests": {
      "req-1": {
        "id": "req-1",
        "name": "Login",
        "method": "POST",
        "url": "https://api.example.com/login",
        "headers": [ { "id": "h1", "key": "X-Trace", "value": "abc", "enabled": true } ],
        "queryParams": [],
        "pathParams": [],
        "body": "{\"user\":\"admin\"}",
        "bodyMode": "json",
        "contentType": "application/json",
        "formParams": [],
        "binaryFile": null,
        "auth": { "type": "bearer", "token": "{{token}}" },
        "cookies": [],
        "prereqScript": "pm.environment.set('a','1');",
        "postreqScript": "pm.test('ok', () => {});",
        "preResolveActions": [
          { "type": "script", "name": "Generate random number", "code": "pm.environment.set('nonce','1');" }
        ],
        "preActions": [
          { "type": "script", "name": "Rewrite request headers", "code": "pm.request.headers.upsert({ key: 'X-A', value: '1' });" },
          { "type": "db", "name": "Query user", "datasource": "ds-1", "sql": "SELECT 1", "extract_var": "dbOne" }
        ],
        "postActions": [
          { "type": "script", "code": "pm.test('ok', () => {});" }
        ],
        "modelId": "m-1",
        "responses": [
          { "id": "r1", "name": "OK", "status": 200, "contentType": "application/json",
            "body": "{\"code\":0}", "schema": { "type": "object" } }
        ]
      },
      "req-2": {
        "id": "req-2",
        "name": "WS",
        "protocol": "websocket",
        "url": "ws://localhost:8080",
        "headers": [],
        "messages": [
          { "id": "m1", "payload": "hello", "payloadType": "text", "waitMs": 100 }
        ],
        "closeAfter": 1
      },
      "req-3": {
        "id": "req-3",
        "name": "Plugin",
        "protocol": "pg",
        "url": "",
        "headers": [],
        "options": { "timeout": 5 }
      }
    },
    "models": [
      {
        "id": "m-1",
        "name": "LoginRequest",
        "description": "Login request",
        "fields": [
          { "id": "f1", "name": "username", "type": "string", "required": true,
            "description": "Username", "example": "admin" },
          { "id": "f2", "name": "tags", "type": "array",
            "children": [ { "id": "f3", "name": "tag", "type": "string" } ] }
        ]
      }
    ],
    "environments": [
      { "id": "e1", "name": "Development",
        "variables": { "base_url": "http://localhost:8080" },
        "secrets": { "token": "abc" } }
    ],
    "activeEnvId": "e1",
    "globalVariables": { "g": "1" },
    "globalSecrets": {},
    "scenarios": [
      {
        "id": "s1",
        "name": "Login flow",
        "steps": [
          { "id": "st1", "type": "request", "name": "Login", "requestId": "req-1" },
          { "id": "st2", "type": "wait", "name": "Wait", "ms": 500 },
          { "id": "st3", "type": "loop", "name": "Loop", "count": 3, "children": [] },
          { "id": "st4", "type": "condition", "name": "Condition", "expr": "a==1",
            "children": [], "elseChildren": [] },
          { "id": "st5", "type": "setvar", "name": "Set variable", "varKey": "x", "varValue": "1" },
          { "id": "st6", "type": "group", "name": "Group", "children": [] }
        ]
      }
    ],
    "plugins": [],
    "history": [],
    "mockRules": [],
    "locale": "zh-CN",
    "theme": "dark",
    "ui": { "sidebarCollapsed": false },
    "executionTarget": { "mode": "local", "agentIds": null }
  }
}"#;

#[test]
fn frontend_snapshot_deserializes() {
    let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT)
        .expect("failed to deserialize the frontend snapshot");

    assert_eq!(snap.schema_version, 1);
    assert_eq!(snap.data.locale, "zh-CN");
    assert_eq!(snap.data.theme, "dark");
    assert_eq!(snap.data.legacy_active_env_id.as_deref(), Some("e1"));
    assert_eq!(
        snap.data
            .legacy_global_variables
            .get("g")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(snap.data.execution_target.mode, "local");

    // Collection tree: folder -> request, grpc-package hierarchy
    let col = &snap.data.collections[0];
    assert_eq!(col.name, "Demo");
    assert_eq!(col.items.len(), 2);
    assert!(matches!(
        col.items[0],
        orbit_data::CollectionItem::Folder { .. }
    ));
    assert!(matches!(
        col.items[1],
        orbit_data::CollectionItem::GrpcPackage { .. }
    ));

    // Request discrimination: http (protocol omitted) / websocket / plugin (any protocol id)
    let req1 = snap.data.requests.get("req-1").expect("req-1 missing");
    assert_eq!(req1.protocol(), "http");
    assert_eq!(req1.name(), "Login");
    assert_eq!(req1.prereq_script(), Some("pm.environment.set('a','1');"));
    // Action lists (pre-resolve / pre / post) must enter the strongly-typed model:
    // historically orbit-data only had prereq_script / postreq_script, so the frontend preActions
    // were silently dropped during snapshot roundtrips (frontend edits even cleared the old fields, with no redundant copy as a fallback).
    assert_eq!(req1.pre_resolve_actions().len(), 1);
    assert_eq!(req1.pre_actions().len(), 2);
    assert_eq!(req1.post_actions().len(), 1);
    assert!(matches!(req1, ApiRequest::Http(h) if h.model_id.as_deref() == Some("m-1")));
    assert!(
        matches!(req1, ApiRequest::Http(h) if h.responses.as_ref().is_some_and(|r| r.len() == 1))
    );

    let req2 = snap.data.requests.get("req-2").expect("req-2 missing");
    assert_eq!(req2.protocol(), "websocket");
    assert!(matches!(req2, ApiRequest::Ws(w) if w.close_after == Some(1)));

    let req3 = snap.data.requests.get("req-3").expect("req-3 missing");
    assert_eq!(req3.protocol(), "pg");
    assert!(matches!(req3, ApiRequest::Plugin(p) if p.options.is_some()));

    // Scenario steps: discrimination of 6 kinds
    let scenario = &snap.data.scenarios[0];
    assert_eq!(scenario.steps.len(), 6);
    assert!(matches!(
        scenario.steps[1],
        orbit_data::ScenarioStep::Wait(_)
    ));
    assert!(matches!(
        scenario.steps[3],
        orbit_data::ScenarioStep::Condition(_)
    ));
    assert!(matches!(
        scenario.steps[4],
        orbit_data::ScenarioStep::Setvar(_)
    ));
}

/// Compat read-in: the previous two-phase field (`preResolveActions`) is merged into the single list **at load time**,
/// positioned **before** the built-in interpolation node; the merge is not written out - the key no longer appears after serialization.
#[test]
fn pre_resolve_actions_merge_into_single_list() {
    fn normalize_all(snap: &mut Snapshot) {
        for request in snap.data.requests.values_mut() {
            request.merge_pre_resolve_actions();
        }
    }

    let mut snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    normalize_all(&mut snap);

    let req1 = snap.data.requests.get("req-1").unwrap();
    // Single list: pre-resolve script -> built-in interpolation node -> original preActions (script -> DB)
    let list = req1.pre_actions();
    assert_eq!(
        list.len(),
        4,
        "the compat field should be merged into the single list: {list:?}"
    );
    assert_eq!(
        list[0].script_code(),
        Some("pm.environment.set('nonce','1');")
    );
    assert!(list[1].is_interpolate());
    assert_eq!(list[2].kind(), "script");
    assert_eq!(list[3].kind(), "db");
    // The compat field is now empty -> no longer written out on serialization
    assert!(req1.pre_resolve_actions().is_empty());
    let json = serde_json::to_string(req1).unwrap();
    assert!(
        !json.contains("preResolveActions"),
        "the compat field should no longer be written out: {json}"
    );

    // Idempotence (stronger): even if a client writes back the already-merged list together with the **uncleared** compat field,
    // no action is inserted twice - the presence of the anchor in the list is taken as "already merged".
    let merged = snap
        .data
        .requests
        .get("req-1")
        .unwrap()
        .pre_actions()
        .to_vec();
    let mut again: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    if let Some(ApiRequest::Http(h)) = again.data.requests.get_mut("req-1") {
        h.pre_actions = merged;
    }
    normalize_all(&mut again);
    let req1 = again.data.requests.get("req-1").unwrap();
    assert_eq!(
        req1.pre_actions().len(),
        4,
        "compat actions must not be inserted twice"
    );
    assert!(req1.pre_resolve_actions().is_empty());

    // Requests without pre-actions gain nothing from the merge (the anchor is added by the execution / UI layer)
    assert!(snap
        .data
        .requests
        .get("req-3")
        .unwrap()
        .pre_actions()
        .is_empty());
}

#[test]
fn roundtrip_preserves_all_fields() {
    let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    let json = serde_json::to_string_pretty(&snap).unwrap();
    let back: Snapshot = serde_json::from_str(&json).unwrap();

    // Key fields roundtrip losslessly
    let req1 = back.data.requests.get("req-1").unwrap();
    assert_eq!(req1.name(), "Login");
    let h = match req1 {
        ApiRequest::Http(h) => h,
        _ => panic!("req-1 should be http"),
    };
    assert_eq!(h.method, "POST");
    assert_eq!(h.body_mode, orbit_data::BodyMode::Json);
    assert_eq!(h.auth.r#type, "bearer");
    assert_eq!(h.headers.len(), 1);
    assert_eq!(h.headers[0].key, "X-Trace");
    assert_eq!(h.model_id.as_deref(), Some("m-1"));

    // Action lists roundtrip losslessly (including structured fields such as the DB action's datasource / extract_var)
    assert_eq!(h.pre_resolve_actions.len(), 1);
    assert_eq!(
        h.pre_resolve_actions[0].script_code(),
        Some("pm.environment.set('nonce','1');")
    );
    assert_eq!(h.pre_actions.len(), 2);
    assert_eq!(h.pre_actions[1].kind(), "db");
    assert_eq!(h.post_actions.len(), 1);

    let scenario = &back.data.scenarios[0];
    assert_eq!(scenario.steps.len(), 6);
    assert!(matches!(
        scenario.steps[2],
        orbit_data::ScenarioStep::Loop(_)
    ));

    // Environments and secrets
    assert_eq!(
        back.data.environments[0]
            .secrets
            .get("token")
            .map(String::as_str),
        Some("abc")
    );
}

#[test]
fn version_mismatch_rejected() {
    let mut json: serde_json::Value = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    json["schemaVersion"] = serde_json::json!(99);
    let mem = MemoryStorage::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(mem.save(&serde_json::from_value(json).unwrap()))
        .unwrap();
    let err = rt
        .block_on(DataService::<MemoryStorage>::open(mem, SNAPSHOT_VERSION))
        .err()
        .expect("a higher-version snapshot should be rejected");
    assert!(
        err.to_string().contains("version not compatible"),
        "error message mismatch: {err}"
    );
}

#[test]
fn corrupted_file_backed_up_and_ignored() {
    let (storage, dir) = orbit_data::storage::temp_file_storage();
    let path = dir.join("orbit_data.json");
    std::fs::write(&path, "{ not valid json").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let svc = rt
        .block_on(DataService::open(storage, SNAPSHOT_VERSION))
        .expect("a corrupt file should be treated as no snapshot");

    // Treated as first launch: had_initial = false, nothing persisted (wait for the first push after the frontend seeds)
    assert!(!svc.had_initial());
    assert!(svc.collections_in("ws-default").is_empty());
    // The corrupt file was renamed and archived
    assert!(dir.join("orbit_data.json.bak").exists());
    // First launch does not persist proactively
    assert!(!path.exists());

    // After the frontend pushes the first snapshot it is persisted, and had_initial flips
    let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    rt.block_on(svc.load_from_json(&serde_json::to_string(&snap).unwrap()))
        .unwrap();
    assert!(svc.had_initial());
    assert!(path.exists(), "the first push should be persisted");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_push_then_reload() {
    let (storage, dir) = orbit_data::storage::temp_file_storage();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let svc = rt
        .block_on(DataService::open(storage, SNAPSHOT_VERSION))
        .unwrap();
    // First launch: no existing data, nothing persisted
    assert!(!svc.had_initial());
    assert!(!dir.join("orbit_data.json").exists());

    // The frontend seeds and pushes the first snapshot
    let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    let json = serde_json::to_string(&snap).unwrap();
    rt.block_on(svc.load_from_json(&json)).unwrap();
    assert!(svc.had_initial());

    // to_json roundtrip: re-parsed fields are complete
    let out = svc.to_json().unwrap();
    let back: Snapshot = serde_json::from_str(&out).unwrap();
    assert_eq!(back.data.requests.len(), 3);
    assert_eq!(back.data.collections[0].name, "Demo");

    // Restart (same file): had_initial = true, data is there
    let svc2 = rt
        .block_on(DataService::open(
            FileStorage::new(dir.join("orbit_data.json")),
            SNAPSHOT_VERSION,
        ))
        .unwrap();
    assert!(svc2.had_initial());
    assert_eq!(svc2.data().requests.len(), 3);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn crud_upsert_remove_persisted() {
    let (storage, dir) = orbit_data::storage::temp_file_storage();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();

    // Write a collection + request + model + environment + scenario
    svc.upsert_collection(orbit_data::Collection {
        id: "c1".into(),
        name: "Demo".into(),
        workspace_id: "ws-default".into(),
        kind: None,
        grpc: None,
        connection: None,
        items: vec![orbit_data::CollectionItem::Request {
            id: "i1".into(),
            request_id: "r1".into(),
        }],
    })
    .await
    .unwrap();
    svc.upsert_request(ApiRequest::Http(orbit_data::HttpRequest {
        id: "r1".into(),
        name: "Login".into(),
        ..Default::default()
    }))
    .await
    .unwrap();
    svc.upsert_model(orbit_data::DataModel {
        id: "m1".into(),
        name: "Login".into(),
        workspace_id: "ws-default".into(),
        description: None,
        fields: vec![],
    })
    .await
    .unwrap();
    svc.upsert_environment(orbit_data::Environment {
        id: "e1".into(),
        name: "Development".into(),
        workspace_id: "ws-default".into(),
        variables: Default::default(),
        secrets: Default::default(),
    })
    .await
    .unwrap();
    svc.set_active_env("ws-default", Some("e1".into()))
        .await
        .unwrap();
    svc.upsert_scenario(orbit_data::Scenario {
        id: "s1".into(),
        name: "Flow".into(),
        workspace_id: "ws-default".into(),
        description: None,
        steps: vec![],
        folder_id: None,
        priority: None,
        env_id: None,
        data_set_id: None,
        use_data_set: None,
        iterations: None,
        on_error: None,
        record_request_details: None,
    })
    .await
    .unwrap();

    // Reopen (FileStorage persisted -> reload)
    let svc2 = DataService::open(
        FileStorage::new(dir.join("orbit_data.json")),
        SNAPSHOT_VERSION,
    )
    .await
    .unwrap();
    assert_eq!(svc2.collections_in("ws-default").len(), 1);
    assert_eq!(svc2.request("r1").unwrap().protocol(), "http");
    assert_eq!(svc2.models_in("ws-default").len(), 1);
    assert_eq!(svc2.environments_in("ws-default").len(), 1);
    assert_eq!(svc2.scenarios_in("ws-default").len(), 1);
    assert_eq!(svc2.active_env_in("ws-default").as_deref(), Some("e1"));

    // Delete
    svc2.remove_collection("c1").await.unwrap();
    svc2.remove_request("r1").await.unwrap();
    svc2.remove_model("m1").await.unwrap();
    svc2.remove_environment("e1").await.unwrap();
    svc2.remove_scenario("s1").await.unwrap();

    let svc3 = DataService::open(
        FileStorage::new(dir.join("orbit_data.json")),
        SNAPSHOT_VERSION,
    )
    .await
    .unwrap();
    assert!(svc3.collections_in("ws-default").is_empty());
    assert!(svc3.request("r1").is_none());
    assert!(svc3.models_in("ws-default").is_empty());
    assert!(svc3.environments_in("ws-default").is_empty());
    assert!(svc3.scenarios_in("ws-default").is_empty());
    assert_eq!(svc3.active_env_in("ws-default"), None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn replace_data_import_snapshot() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let svc = DataService::open(MemoryStorage::new(), SNAPSHOT_VERSION)
            .await
            .unwrap();
        let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
        svc.replace_data(snap.data.clone()).await.unwrap();
        assert_eq!(svc.collections_in("ws-default")[0].name, "Demo");
        assert_eq!(svc.request("req-1").unwrap().name(), "Login");
        // Can reload from storage after persisting
        let _ = svc.save_now().await;
    });
}

#[test]
fn optimistic_lock_conflict_detection() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let storage = MemoryStorage::new();
        let svc = DataService::open(storage.clone(), SNAPSHOT_VERSION)
            .await
            .unwrap();

        // First push (no existing data; forced overwrite semantics are unaffected by the optimistic lock)
        let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
        let json = serde_json::to_string(&snap).unwrap();
        svc.load_from_json_checked(&json, 0)
            .await
            .expect("first launch should not conflict");
        assert!(svc.had_initial());

        // Second push: base equals the current saved_at (normal debounced save) -> success
        let current = svc.snapshot().saved_at;
        svc.load_from_json_checked(&json, current)
            .await
            .expect("saving with an identical base should succeed");

        // After the local copy is updated, pushing the old base -> conflict
        let newer = orbit_data::Snapshot {
            saved_at: current + 1000,
            ..svc.snapshot()
        };
        svc.load_from_json(&serde_json::to_string(&newer).unwrap())
            .await
            .unwrap();
        let err = svc
            .load_from_json_checked(&json, current)
            .await
            .expect_err("a stale base should conflict");
        assert!(
            err.to_string().contains("conflict"),
            "error message mismatch: {err}"
        );
    });
}

#[tokio::test]
async fn history_module_commands() {
    let storage = MemoryStorage::new();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();

    assert!(svc.history().is_empty());
    svc.add_history(orbit_data::PersistedHistoryEntry {
        id: "h1".into(),
        request_id: "r1".into(),
        workspace_id: "ws-default".into(),
        name: "Login".into(),
        method: "POST".into(),
        url: "https://api.example.com/login".into(),
        status: Some(200),
        duration: Some(12),
        size: Some(34),
        timestamp: 1780000000000,
    })
    .await
    .unwrap();
    svc.add_history(orbit_data::PersistedHistoryEntry {
        id: "h2".into(),
        request_id: "r2".into(),
        workspace_id: "ws-default".into(),
        name: "Detail".into(),
        method: "GET".into(),
        url: "https://api.example.com/detail".into(),
        status: None,
        duration: None,
        size: None,
        timestamp: 1780000001000,
    })
    .await
    .unwrap();

    // Insert at the head: h2 first
    let history = svc.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, "h2");
    assert_eq!(history[1].name, "Login");

    // Clear
    svc.clear_history().await.unwrap();
    assert!(svc.history().is_empty());
}

#[tokio::test]
async fn global_variables_commands() {
    let storage = MemoryStorage::new();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();

    let mut vars = std::collections::HashMap::new();
    vars.insert("base_url".to_string(), "http://localhost".to_string());
    svc.set_global_variables("ws-default", vars.clone())
        .await
        .unwrap();
    assert_eq!(svc.global_variables_in("ws-default"), vars);

    let mut secrets = std::collections::HashMap::new();
    secrets.insert("token".to_string(), "secret".to_string());
    svc.set_global_secrets("ws-default", secrets.clone())
        .await
        .unwrap();
    assert_eq!(svc.global_secrets_in("ws-default"), secrets);
}

#[test]
fn memory_storage_roundtrip() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let storage = MemoryStorage::new();
        let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();
        assert!(svc.data().collections.is_empty());

        let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
        svc.replace_data(snap.data).await.unwrap();
        let loaded = svc.snapshot();
        assert_eq!(loaded.data.requests.len(), 3);
        // Data is still there after the service is rebuilt (Arc shares the same underlying storage)
        let svc2 = DataService::open(svc.storage().clone(), SNAPSHOT_VERSION)
            .await
            .unwrap();
        assert_eq!(svc2.data().requests.len(), 3);
    });
}

// ─── AI preferences and new entity writes (used by the AI assistant) ────

/// Old snapshots (without the `ai` field) must stay readable as-is and fall back to default preferences - hence SNAPSHOT_VERSION needs no bump.
#[test]
fn snapshot_without_ai_field_uses_default_prefs() {
    let snap: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    let ai = snap.data.ai;
    assert_eq!(ai.provider, "openai");
    assert_eq!(ai.language, "zh");
    assert!(ai.model.is_empty());
    assert_eq!(
        ai.mode, "agent",
        "default mode is Agent (ready to work as soon as it opens)"
    );
    assert_eq!(ai.max_rounds, 8);
    assert_eq!(ai.max_tokens, 4096);
}

#[tokio::test]
async fn ai_prefs_persist_across_reload() {
    let storage = MemoryStorage::new();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();
    let mut prefs = svc.ai_prefs();
    prefs.provider = "anthropic".into();
    prefs.model = "claude-sonnet-4-20250514".into();
    prefs.base_url = "https://api.anthropic.com/v1".into();
    prefs.mode = "plan".into();
    prefs.provider_id = Some("cred-1".into());
    svc.set_ai_prefs(prefs).await.unwrap();

    let svc2 = DataService::open(svc.storage().clone(), SNAPSHOT_VERSION)
        .await
        .unwrap();
    let back = svc2.ai_prefs();
    assert_eq!(back.provider, "anthropic");
    assert_eq!(back.model, "claude-sonnet-4-20250514");
    assert_eq!(back.provider_id.as_deref(), Some("cred-1"));
    assert_eq!(back.mode, "plan");
}

#[tokio::test]
async fn scenario_folder_suite_and_dataset_crud() {
    use orbit_data::model::{Scenario, ScenarioDataSet, ScenarioFolder, TestSuite};

    let storage = MemoryStorage::new();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();

    // Folders (two levels)
    svc.upsert_scenario_folder(ScenarioFolder {
        id: "f-root".into(),
        name: "User module".into(),
        workspace_id: "ws-default".into(),
        parent_id: None,
        collapsed: None,
    })
    .await
    .unwrap();
    svc.upsert_scenario_folder(ScenarioFolder {
        id: "f-child".into(),
        name: "Login".into(),
        workspace_id: "ws-default".into(),
        parent_id: Some("f-root".into()),
        collapsed: None,
    })
    .await
    .unwrap();
    assert_eq!(svc.scenario_folders_in("ws-default").len(), 2);

    // The scenario hangs under the child folder
    svc.upsert_scenario(Scenario {
        id: "s-1".into(),
        name: "Login success".into(),
        workspace_id: "ws-default".into(),
        description: None,
        steps: vec![],
        folder_id: Some("f-child".into()),
        priority: None,
        env_id: None,
        data_set_id: Some("ds-1".into()),
        use_data_set: Some(true),
        iterations: None,
        on_error: None,
        record_request_details: None,
    })
    .await
    .unwrap();

    // Delete the child folder: the scenario is lifted to the parent folder, no data loss
    svc.remove_scenario_folder("f-child").await.unwrap();
    let scenarios = svc.scenarios_in("ws-default");
    assert_eq!(scenarios.len(), 1);
    assert_eq!(scenarios[0].folder_id.as_deref(), Some("f-root"));

    // Dataset: the scenario is automatically unbound after deletion
    svc.upsert_scenario_data_set(ScenarioDataSet {
        id: "ds-1".into(),
        name: "Login data".into(),
        workspace_id: "ws-default".into(),
        csv: "user,pass\nu1,p1".into(),
        columns: vec!["user".into(), "pass".into()],
        row_count: 1,
        mode: Some("sequential".into()),
        updated_at: 0,
    })
    .await
    .unwrap();
    assert_eq!(svc.scenario_data_sets_in("ws-default").len(), 1);
    svc.remove_scenario_data_set("ds-1").await.unwrap();
    let scenarios = svc.scenarios_in("ws-default");
    assert!(scenarios[0].data_set_id.is_none());
    assert_eq!(scenarios[0].use_data_set, Some(false));

    // Suite
    svc.upsert_scenario_suite(TestSuite {
        id: "suite-1".into(),
        name: "Smoke".into(),
        workspace_id: "ws-default".into(),
        description: None,
        env_id: None,
        run_mode: "serial".into(),
        concurrency: None,
        member_ids: vec!["s-1".into()],
        updated_at: 0,
    })
    .await
    .unwrap();
    assert_eq!(svc.scenario_suites_in("ws-default").len(), 1);
    svc.remove_scenario_suite("suite-1").await.unwrap();
    assert!(svc.scenario_suites_in("ws-default").is_empty());
}

#[tokio::test]
async fn remove_workspace_cascades_ai_generated_entities() {
    use orbit_data::model::{ScenarioDataSet, ScenarioFolder, TestSuite};

    let storage = MemoryStorage::new();
    let svc = DataService::open(storage, SNAPSHOT_VERSION).await.unwrap();
    let ws2 = svc.add_workspace("Second", None, None).await.unwrap();
    let ws2_id = ws2.id.clone();

    svc.upsert_scenario_folder(ScenarioFolder {
        id: "f2".into(),
        name: "Folder".into(),
        workspace_id: ws2_id.clone(),
        parent_id: None,
        collapsed: None,
    })
    .await
    .unwrap();
    svc.upsert_scenario_data_set(ScenarioDataSet {
        id: "ds2".into(),
        name: "Data".into(),
        workspace_id: ws2_id.clone(),
        csv: "a\n1".into(),
        columns: vec!["a".into()],
        row_count: 1,
        mode: None,
        updated_at: 0,
    })
    .await
    .unwrap();
    svc.upsert_scenario_suite(TestSuite {
        id: "su2".into(),
        name: "Suite".into(),
        workspace_id: ws2_id.clone(),
        description: None,
        env_id: None,
        run_mode: "serial".into(),
        concurrency: None,
        member_ids: vec![],
        updated_at: 0,
    })
    .await
    .unwrap();

    svc.remove_workspace(&ws2_id).await.unwrap();
    assert!(svc.scenario_folders_in(&ws2_id).is_empty());
    assert!(svc.scenario_data_sets_in(&ws2_id).is_empty());
    assert!(svc.scenario_suites_in(&ws2_id).is_empty());
}

// ─── Script library (reusable action templates) + in-request references ──────

/// Library entries are **workspace-scoped** entities (frontend shape: `workspaceId` stamped + camelCase),
/// and both the reference entries (`type: ref`) in request action lists and the entries themselves must enter the strongly-typed model -
/// otherwise snapshot roundtrips silently drop them (a hard rule of this project).
#[test]
fn action_templates_and_refs_roundtrip() {
    let json = serde_json::json!({
        "schemaVersion": 1,
        "savedAt": 1780000000000i64,
        "source": "tauri",
        "sync": { "remoteUrl": null, "lastSyncedAt": null },
        "data": {
            "requests": {
                "req-1": {
                    "id": "req-1",
                    "name": "Place order",
                    "method": "POST",
                    "url": "https://api.example.com/order",
                    "headers": [],
                    "queryParams": [],
                    "pathParams": [],
                    "formParams": [],
                    "body": "{}",
                    "bodyMode": "json",
                    "cookies": [],
                    "preActions": [
                        { "type": "ref", "library_id": "tpl-nonce", "name": "this nonce" },
                        { "type": "interpolate" },
                        { "type": "ref", "library_id": "tpl-sign" }
                    ]
                }
            },
            "actionTemplates": [
                {
                    "workspaceId": "ws-default",
                    "id": "tpl-nonce",
                    "name": "Generate nonce",
                    "action": { "type": "script", "code": "pm.environment.set('nonce','1');" }
                },
                {
                    "workspaceId": "ws-default",
                    "id": "tpl-sign",
                    "name": "Compute signature",
                    "description": "HMAC written to X-Sign",
                    "action": { "type": "script", "code": "sign();" },
                    "sortIndex": 3
                }
            ]
        }
    });
    let snap: Snapshot = serde_json::from_value(json)
        .expect("library entries / reference entries should deserialize");

    // Library entries enter the strongly-typed model
    assert_eq!(snap.data.action_templates.len(), 2);
    let nonce = &snap.data.action_templates[0];
    assert_eq!(nonce.workspace_id, "ws-default");
    assert_eq!(nonce.template.id, "tpl-nonce");
    assert_eq!(nonce.template.action.kind(), "script");
    let sign = &snap.data.action_templates[1];
    assert_eq!(sign.template.name, "Compute signature");
    assert_eq!(
        sign.template.description.as_deref(),
        Some("HMAC written to X-Sign")
    );
    assert_eq!(sign.template.sort_index, 3);
    assert_eq!(sign.template.action.script_code(), Some("sign();"));

    // Reference entries enter the request action list (with alias; position preserved)
    let req = snap.data.requests.get("req-1").unwrap();
    assert_eq!(req.pre_actions().len(), 3);
    assert!(req.pre_actions()[0].is_ref());
    assert_eq!(req.pre_actions()[0].name(), "this nonce");
    assert!(req.pre_actions()[1].is_interpolate());
    assert_eq!(req.pre_actions()[2].kind(), "ref");
    assert_eq!(req.pre_actions()[2].name(), "");

    // roundtrip lossless: both library entries and references are present
    let out = serde_json::to_string(&snap).unwrap();
    let back: Snapshot = serde_json::from_str(&out).unwrap();
    assert_eq!(back.data.action_templates.len(), 2);
    assert_eq!(
        back.data.action_templates[1].template.name,
        "Compute signature"
    );
    assert_eq!(back.data.action_templates[1].workspace_id, "ws-default");
    let back_pre = back.data.requests.get("req-1").unwrap().pre_actions();
    assert_eq!(back_pre.len(), 3);
    assert!(back_pre[0].is_ref());
    assert_eq!(back_pre[0].name(), "this nonce");

    // Old snapshots without library entries read as empty (no SNAPSHOT_VERSION bump)
    let legacy: Snapshot = serde_json::from_str(FRONTEND_SNAPSHOT).unwrap();
    assert!(legacy.data.action_templates.is_empty());
}

/// Library entry CRUD: query by workspace, add/update, delete; deleting a workspace cascades cleanup;
/// **deleting a library entry does not clean up references** (broken references are left to the user; no silent breakage).
#[tokio::test]
async fn action_template_crud_and_workspace_cascade() {
    use orbit_config::{ActionTemplate, RequestAction};
    use orbit_data::model::ActionTemplateEntry;

    let svc = DataService::open(MemoryStorage::new(), SNAPSHOT_VERSION)
        .await
        .unwrap();
    let ws2 = svc.add_workspace("Second", None, None).await.unwrap();
    let ws2_id = ws2.id.clone();

    svc.upsert_action_template(ActionTemplateEntry::new(
        "ws-default",
        ActionTemplate::new(
            "t1",
            "Compute signature",
            RequestAction::from_script("sign();"),
        ),
    ))
    .await
    .unwrap();
    svc.upsert_action_template(ActionTemplateEntry::new(
        ws2_id.clone(),
        ActionTemplate::new(
            "t2",
            "Another library entry",
            RequestAction::from_script("nonce();"),
        ),
    ))
    .await
    .unwrap();

    // Isolated per workspace
    let ws1 = svc.action_templates_in("ws-default");
    assert_eq!(ws1.len(), 1);
    assert_eq!(ws1[0].name, "Compute signature");
    assert_eq!(svc.action_templates_in(&ws2_id).len(), 1);

    // Update (same name/id replaced in place)
    svc.upsert_action_template(ActionTemplateEntry::new(
        "ws-default",
        ActionTemplate::new(
            "t1",
            "Compute signature v2",
            RequestAction::from_script("sign2();"),
        ),
    ))
    .await
    .unwrap();
    let ws1 = svc.action_templates_in("ws-default");
    assert_eq!(
        ws1.len(),
        1,
        "the same id should be replaced in place, not appended"
    );
    assert_eq!(ws1[0].name, "Compute signature v2");

    // Delete the library entry: references are kept (dangling references are handled by the UI / execution layer)
    let mut http = orbit_data::HttpRequest {
        id: "req-1".into(),
        name: "Place order".into(),
        ..Default::default()
    };
    http.pre_actions = vec![RequestAction::from_ref("t1"), RequestAction::Interpolate];
    svc.upsert_request(ApiRequest::Http(http)).await.unwrap();
    svc.remove_action_template("t1").await.unwrap();
    assert!(svc.action_templates_in("ws-default").is_empty());
    let req = svc.request("req-1").unwrap();
    assert_eq!(
        req.pre_actions().len(),
        2,
        "deleting a library entry must not clean up references"
    );
    assert!(req.pre_actions()[0].is_ref());

    // Delete the workspace: cascade cleanup of that workspace's library entries
    svc.remove_workspace(&ws2_id).await.unwrap();
    assert!(svc.action_templates_in(&ws2_id).is_empty());
}
