//! Regression test: a frontend v2 snapshot push must succeed after the backend loads and migrates a legacy v1 file.
//! It once failed because orbit-data::SNAPSHOT_VERSION stayed at 1 and rejected frontend v2 pushes (VersionMismatch),
//! so every frontend change (workspaces included) could not be persisted.

use orbit_data::{DataService, MemoryStorage, SNAPSHOT_VERSION};

/// Synthesize a v1 snapshot (legacy frontend shape: no workspace layer, entities without workspaceId)
fn v1_snapshot_json() -> String {
    r#"{
        "schemaVersion": 1,
        "savedAt": 1787215480794,
        "source": "tauri",
        "sync": { "remoteUrl": null, "lastSyncedAt": null },
        "data": {
            "collections": [
                {
                    "id": "col-1",
                    "name": "Demo",
                    "items": [
                        { "type": "request", "id": "i1", "requestId": "req-1" }
                    ]
                }
            ],
            "requests": {
                "req-1": {
                    "id": "req-1",
                    "name": "Login",
                    "method": "POST",
                    "url": "https://api.example.com/login",
                    "headers": [],
                    "queryParams": [],
                    "pathParams": [],
                    "body": "",
                    "bodyMode": "json",
                    "contentType": "application/json",
                    "formParams": [],
                    "binaryFile": null,
                    "auth": { "type": "none" },
                    "cookies": [],
                    "prereqScript": null,
                    "postreqScript": null,
                    "responses": []
                }
            },
            "models": [],
            "environments": [],
            "activeEnvId": null,
            "globalVariables": {},
            "globalSecrets": {},
            "scenarios": [],
            "plugins": [],
            "history": [],
            "mockRules": [],
            "locale": "zh-CN",
            "theme": "dark",
            "ui": { "sidebarCollapsed": false },
            "executionTarget": { "mode": "local", "agentIds": null }
        }
    }"#
    .to_string()
}

#[test]
fn frontend_v2_push_after_v1_migration_succeeds() {
    let raw = v1_snapshot_json();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let storage = MemoryStorage::new();
        let svc = DataService::open(storage.clone(), SNAPSHOT_VERSION)
            .await
            .unwrap();

        // 1) The backend accepts the legacy v1 file and migrates it (ensure_migrated -> schema_version bumps to 2)
        svc.load_from_json(&raw)
            .await
            .expect("the backend should accept the v1 file and migrate it");
        let migrated = svc.to_json().unwrap();
        let m: serde_json::Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(
            m["schemaVersion"], 2,
            "the snapshot should be v2 after migration"
        );
        assert_eq!(
            m["data"]["workspaces"][0]["id"], "ws-default",
            "migration should create the default workspace"
        );

        // 2) Simulate the frontend: add a workspace to the migrated v2 snapshot and push it (optimistic-lock base = current savedAt)
        let mut v2 = m;
        v2["data"]["workspaces"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "ws-test-1",
                "name": "Test Workspace",
                "color": "#0ea5e9",
                "createdAt": 1787215480794_i64,
                "sortIndex": 1
            }));
        v2["data"]["activeWorkspaceId"] = serde_json::json!("ws-test-1");
        let v2json = v2.to_string();

        let base = svc.snapshot().saved_at;
        svc.load_from_json_checked(&v2json, base).await.expect(
            "the frontend v2 push should succeed (it was once rejected for version mismatch)",
        );

        // 3) Reload to verify the workspace was persisted
        let out: serde_json::Value = serde_json::from_str(&svc.to_json().unwrap()).unwrap();
        let ids: Vec<&str> = out["data"]["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["id"].as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&"ws-test-1"),
            "the new workspace should be persisted, actual {ids:?}"
        );
        // Legacy data migration assigns it to the default workspace
        assert_eq!(out["data"]["collections"][0]["workspaceId"], "ws-default");
    });
}
