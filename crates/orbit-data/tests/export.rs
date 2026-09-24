//! Export orchestration tests: scope collection (collection/folder/single request), model transitive references, ApiSpec assembly, document generation.

use orbit_data::model::{ApiRequest, AuthConfig, BodyMode, HttpRequest};
use orbit_data::{DataModel, SchemaField, Snapshot};

fn sample_snapshot() -> Snapshot {
    let req1 = ApiRequest::Http(HttpRequest {
        id: "r1".into(),
        name: "Create user".into(),
        protocol: Some("http".into()),
        method: "POST".into(),
        url: "https://api.example.com/users".into(),
        headers: vec![
            orbit_data::KeyValue {
                id: "h1".into(),
                key: "Content-Type".into(),
                value: "application/json".into(),
                enabled: true,
                ..Default::default()
            },
            orbit_data::KeyValue {
                id: "h2".into(),
                key: "X-Disabled".into(),
                value: "no".into(),
                enabled: false,
                ..Default::default()
            },
        ],
        query_params: vec![],
        body: "{\"name\":\"a\"}".into(),
        body_mode: BodyMode::Json,
        content_type: "application/json".into(),
        form_params: vec![],
        binary_file: None,
        auth: AuthConfig {
            r#type: "none".into(),
            ..Default::default()
        },
        cookies: vec![],
        prereq_script: Some("pm.environment.set('t','1');".into()),
        postreq_script: Some("pm.test('ok',()=>{});".into()),
        model_id: Some("m1".into()),
        responses: Some(vec![orbit_data::ResponseDef {
            id: "resp1".into(),
            name: "OK".into(),
            status: 200,
            content_type: "application/json".into(),
            body: "{\"id\":1}".into(),
            description: None,
            schema: None,
        }]),
        ..Default::default()
    });
    let req2 = ApiRequest::Http(HttpRequest {
        id: "r2".into(),
        name: "WS not exported".into(),
        protocol: Some("http".into()),
        method: "GET".into(),
        url: "https://api.example.com/ws".into(),
        ..Default::default()
    });

    let data = orbit_data::PersistedData {
        collections: vec![orbit_data::Collection {
            id: "c1".into(),
            name: "Demo".into(),
            workspace_id: "ws-default".into(),
            kind: None,
            grpc: None,
            connection: None,
            items: vec![
                orbit_data::CollectionItem::Folder {
                    id: "f1".into(),
                    name: "Users".into(),
                    items: vec![
                        orbit_data::CollectionItem::Request {
                            id: "i1".into(),
                            request_id: "r1".into(),
                        },
                        orbit_data::CollectionItem::Request {
                            id: "i2".into(),
                            request_id: "r2".into(),
                        },
                    ],
                },
                orbit_data::CollectionItem::Request {
                    id: "i3".into(),
                    request_id: "r3".into(),
                },
            ],
        }],
        requests: {
            let mut m = std::collections::HashMap::new();
            m.insert("r1".into(), req1);
            m.insert("r2".into(), req2);
            // Non-HTTP protocol request (ws): should not enter export collection
            m.insert(
                "r3".into(),
                ApiRequest::Ws(orbit_data::WsRequest {
                    id: "r3".into(),
                    name: "WS connection".into(),
                    protocol: "websocket".into(),
                    url: "ws://localhost".into(),
                    headers: vec![],
                    messages: vec![],
                    close_after: None,
                    prereq_script: None,
                    postreq_script: None,
                    pre_resolve_actions: vec![],
                    pre_actions: vec![],
                    post_actions: vec![],
                }),
            );
            m
        },
        models: vec![
            DataModel {
                id: "m1".into(),
                name: "CreateUserRequest".into(),
                workspace_id: "ws-default".into(),
                description: Some("Create user".into()),
                fields: vec![
                    SchemaField {
                        id: "f1".into(),
                        name: "name".into(),
                        r#type: "string".into(),
                        required: Some(true),
                        description: Some("Username".into()),
                        example: Some("admin".into()),
                        ..Default::default()
                    },
                    SchemaField {
                        id: "f2".into(),
                        name: "tags".into(),
                        r#type: "array".into(),
                        ref_model_id: Some("m2".into()),
                        ..Default::default()
                    },
                ],
            },
            DataModel {
                id: "m2".into(),
                name: "Tag".into(),
                workspace_id: "ws-default".into(),
                description: None,
                fields: vec![],
            },
        ],
        ..Default::default()
    };
    Snapshot {
        schema_version: 1,
        saved_at: 0,
        source: "test".into(),
        sync: orbit_data::SyncInfo {
            remote_url: None,
            last_synced_at: None,
        },
        data,
    }
}

#[test]
fn collect_whole_collection() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: None,
    };
    let items = orbit_data::collect_export_requests(&snap.data, &range);

    // Only r1 (HTTP); r2 is also collected but... wait, r2 is http so it goes in. r3 is ws, so it does not.
    // Actual: r1 (inside folder Users), r2 (inside folder Users), r3 is ws, not exported -> 2 entries
    assert_eq!(
        items.len(),
        2,
        "should collect 2 HTTP requests (r3 is ws, not exported)"
    );
    assert_eq!(items[0].breadcrumb, vec!["Users"]);
    assert_eq!(items[1].breadcrumb, vec!["Users"]);
}

#[test]
fn collect_folder() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: Some("f1".into()),
    };
    let items = orbit_data::collect_export_requests(&snap.data, &range);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].breadcrumb, vec!["Users"]);
}

#[test]
fn collect_single_request() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: Some("i1".into()),
    };
    let items = orbit_data::collect_export_requests(&snap.data, &range);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].request.name(), "Create user");
    assert_eq!(
        items[0].breadcrumb,
        vec!["Users"],
        "single-request export breadcrumb should be the ancestor folder"
    );
}

#[test]
fn referenced_models_with_transitive_ref() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: Some("i1".into()),
    };
    let items = orbit_data::collect_export_requests(&snap.data, &range);
    let models = orbit_data::collect_referenced_models(&items, &snap.data.models);
    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    // m1 (modelId) + m2 (transitive reference via m1's field refModelId)
    assert_eq!(names, vec!["CreateUserRequest", "Tag"]);
}

#[test]
fn build_api_spec_maps_fields() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: Some("i1".into()),
    };
    let items = orbit_data::collect_export_requests(&snap.data, &range);
    let models = orbit_data::collect_referenced_models(&items, &snap.data.models);
    let spec = orbit_data::build_api_spec("Demo", &items, &models);

    assert_eq!(spec.endpoints.len(), 1);
    let ep = &spec.endpoints[0];
    assert_eq!(ep.model_ref.as_deref(), Some("CreateUserRequest"));
    assert!(ep
        .pre_script
        .as_deref()
        .unwrap()
        .contains("pm.environment.set"));
    assert!(ep.post_script.as_deref().unwrap().contains("pm.test"));
    assert_eq!(ep.responses.len(), 1);
    assert_eq!(ep.responses[0].status, 200);
    // Disabled headers are filtered out
    let headers = match &ep.request {
        orbit_config::RequestSpec::Http(h) => &h.headers,
        _ => unreachable!(),
    };
    assert_eq!(headers.len(), 1);
    assert!(headers.contains_key("Content-Type"));
    assert!(!headers.contains_key("X-Disabled"));

    // Model schema: array + refModelId -> items.$ref (bug fix)
    assert_eq!(spec.models.len(), 2);
    let m1 = spec
        .models
        .iter()
        .find(|m| m.name == "CreateUserRequest")
        .unwrap();
    let tags = m1.schema["properties"]["tags"].clone();
    assert_eq!(tags["type"], serde_json::json!("array"));
    assert_eq!(
        tags["items"]["$ref"],
        serde_json::json!("#/components/schemas/Tag")
    );
}

#[test]
fn export_document_openapi_contains_model_ref_and_scripts() {
    let snap = sample_snapshot();
    let range = orbit_data::ExportRange {
        collection_id: "c1".into(),
        workspace_id: None,
        item_id: Some("i1".into()),
    };
    let out = orbit_data::export_document(&snap, "openapi", "Demo", &range).unwrap();

    // requestBody-linked model: schema.$ref (not example, so roundtrip restores model_ref)
    assert!(
        out.contains("\"$ref\": \"#/components/schemas/CreateUserRequest\""),
        "requestBody model $ref missing: {out}"
    );
    assert!(
        out.contains("x-orbit-prerequest"),
        "pre-request script missing: {out}"
    );
    assert!(
        out.contains("x-orbit-postrequest"),
        "post-request script missing: {out}"
    );
    assert!(
        out.contains("\"tags\"") || out.contains("Users"),
        "folder grouping (tags) missing: {out}"
    );
    // Model field examples are preserved (field-level example inside the schema is valid output)
    assert!(
        out.contains("\"example\": \"admin\""),
        "model field example missing: {out}"
    );
    // Response example preserved
    assert!(out.contains("\"id\": 1"), "response example missing: {out}");
}
