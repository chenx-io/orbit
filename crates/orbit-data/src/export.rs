//! Export orchestration: export range → data collection → ApiSpec → document.
//!
//! Takes over the duties of the old frontend `collect.ts` + `buildApiSpec.ts`; after moving into Rust:
//! - The frontend only passes the "export range" (collectionId + itemId) and the latest snapshot
//! - This layer collects requests (tree walk + breadcrumb) and referenced models (transitive references) from the snapshot,
//!   assembles an [`orbit_config::exchange::ApiSpec`], then calls exchange to render the document
//!
//! Fixes a bug in the old frontend reference implementation: an array field referencing a model should export
//! `{type: "array", items: {$ref}}` instead of a bare `{$ref}`.

use std::collections::{HashMap, HashSet};

use orbit_config::exchange::{ApiSpec, EndpointSpec, ModelSpec, ResponseSpec};
use orbit_config::{HttpRequestConfig, RequestSpec};
use serde_json::{json, Map, Value};

use crate::error::DataError;
use crate::model::{ApiRequest, CollectionItem, DataModel, PersistedData, SchemaField, Snapshot};

/// Export range:
/// - `workspace_id` set and `collection_id` empty → export all collections of that workspace
/// - `collection_id` set → a collection (item_id omitted = whole collection, folder = folder, request = single request)
#[derive(Debug, Clone, Default)]
pub struct ExportRange {
    pub workspace_id: Option<String>,
    pub collection_id: String,
    pub item_id: Option<String>,
}

/// Collection result: request + breadcrumb (ancestor folder names, excluding the collection name)
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub request: ApiRequest,
    pub breadcrumb: Vec<String>,
}

// ─── Collect ──────────────────────────────────────────────────

/// Collects exportable endpoints for a range (HTTP only; multi-protocol requests do not enter HTTP docs).
pub fn collect_export_requests(data: &PersistedData, range: &ExportRange) -> Vec<ExportRequest> {
    let mut out = Vec::new();

    // Workspace-level export: all collections under that workspace (breadcrumb = folder hierarchy inside the collection)
    if let Some(ws_id) = &range.workspace_id {
        if range.collection_id.is_empty() {
            for col in data.collections.iter().filter(|c| &c.workspace_id == ws_id) {
                walk(&col.items, &data.requests, &[], &mut out);
            }
            return out;
        }
    }

    let Some(col) = data
        .collections
        .iter()
        .find(|c| c.id == range.collection_id)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match &range.item_id {
        None => walk(&col.items, &data.requests, &[], &mut out),
        Some(item_id) => {
            let Some(chain) = find_chain(&col.items, item_id) else {
                return out;
            };
            match chain.last() {
                Some(CollectionItem::Request { request_id, .. }) => {
                    if let Some(req) = data.requests.get(request_id) {
                        if req.protocol() == "http" {
                            out.push(ExportRequest {
                                request: req.clone(),
                                breadcrumb: ancestor_folders(&chain),
                            });
                        }
                    }
                }
                Some(CollectionItem::Folder { items, name, .. }) => {
                    let mut crumb = ancestor_folders(&chain);
                    crumb.push(name.clone());
                    walk(items, &data.requests, &crumb, &mut out);
                }
                _ => {} // grpc node, no exportable HTTP request
            }
        }
    }
    out
}

fn find_chain<'a>(items: &'a [CollectionItem], target: &str) -> Option<Vec<&'a CollectionItem>> {
    for it in items {
        if it.id() == target {
            return Some(vec![it]);
        }
        if let CollectionItem::Folder { items: sub, .. } = it {
            if let Some(mut chain) = find_chain(sub, target) {
                chain.insert(0, it);
                return Some(chain);
            }
        }
    }
    None
}

/// Ancestor folder names before the target node in the chain (excluding the target itself)
fn ancestor_folders(chain: &[&CollectionItem]) -> Vec<String> {
    chain[..chain.len().saturating_sub(1)]
        .iter()
        .filter_map(|it| match it {
            CollectionItem::Folder { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn walk(
    items: &[CollectionItem],
    requests: &HashMap<String, ApiRequest>,
    crumb: &[String],
    out: &mut Vec<ExportRequest>,
) {
    for it in items {
        match it {
            CollectionItem::Request { request_id, .. } => {
                if let Some(req) = requests.get(request_id) {
                    if req.protocol() == "http" {
                        out.push(ExportRequest {
                            request: req.clone(),
                            breadcrumb: crumb.to_vec(),
                        });
                    }
                }
            }
            CollectionItem::Folder {
                items: sub, name, ..
            } => {
                let mut c = crumb.to_vec();
                c.push(name.clone());
                walk(sub, requests, &c, out);
            }
            _ => {} // grpc node
        }
    }
}

/// Collects the "referenced data models" of the exported endpoints (including transitive references: recurse along refModelId, deduplicated).
pub fn collect_referenced_models(items: &[ExportRequest], models: &[DataModel]) -> Vec<DataModel> {
    let by_id: HashMap<&str, &DataModel> = models.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut result: Vec<DataModel> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<&str> = Vec::new();

    for item in items {
        let model_id = match &item.request {
            ApiRequest::Http(h) => h.model_id.as_deref(),
            _ => None,
        };
        if let Some(id) = model_id {
            if !seen.contains(id) && by_id.contains_key(id) {
                seen.insert(id.to_string());
                queue.push(id);
            }
        }
    }
    while let Some(id) = queue.pop() {
        let Some(m) = by_id.get(id) else { continue };
        for f in &m.fields {
            if let Some(rid) = &f.ref_model_id {
                if !seen.contains(rid) && by_id.contains_key(rid.as_str()) {
                    seen.insert(rid.clone());
                    queue.push(rid);
                }
            }
        }
    }
    for m in models {
        if seen.contains(&m.id) {
            result.push(m.clone());
        }
    }
    result
}

// ─── Assemble ApiSpec ──────────────────────────────────────────

/// Collection result + models → ApiSpec (exchange input).
pub fn build_api_spec(title: &str, items: &[ExportRequest], models: &[DataModel]) -> ApiSpec {
    let endpoints: Vec<EndpointSpec> = items.iter().map(|e| endpoint_to_spec(e, models)).collect();
    let model_specs: Vec<ModelSpec> = models
        .iter()
        .map(|m| ModelSpec {
            name: m.name.clone(),
            schema: model_to_schema(m, models),
        })
        .collect();
    let mut spec = ApiSpec::new(title);
    spec.endpoints = endpoints;
    spec.models = model_specs;
    spec
}

fn endpoint_to_spec(e: &ExportRequest, models: &[DataModel]) -> EndpointSpec {
    let ApiRequest::Http(h) = &e.request else {
        unreachable!("collect_export_requests already filtered down to HTTP requests")
    };
    let model_name = h
        .model_id
        .as_deref()
        .and_then(|id| models.iter().find(|m| m.id == id).map(|m| m.name.clone()));

    let mut ep = EndpointSpec::new(
        &h.name,
        RequestSpec::Http(Box::new(HttpRequestConfig {
            method: h.method.clone(),
            url: h.url.clone(),
            headers: kv_map(&h.headers),
            body: if h.body.is_empty() {
                None
            } else {
                Some(serde_yaml::Value::String(h.body.clone()))
            },
            timeout: "30s".to_string(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        })),
    );
    ep.group = e.breadcrumb.clone();
    ep.model_ref = model_name;
    ep.pre_script = h.prereq_script.clone();
    ep.post_script = h.postreq_script.clone();
    // Action lists survive the export round-trip (the legacy single-script fields are still mapped separately above)
    ep.pre_resolve_actions = h.pre_resolve_actions.clone();
    ep.pre_actions = h.pre_actions.clone();
    ep.post_actions = h.post_actions.clone();
    ep.responses = h
        .responses
        .as_ref()
        .map(|rs| {
            rs.iter()
                .map(|r| ResponseSpec {
                    status: r.status,
                    name: r.name.clone(),
                    body: r.body.clone(),
                    schema: r.schema.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    ep
}

/// Ordered key-values → Record (skips disabled / empty keys; mirrors the old frontend kvMap)
fn kv_map(items: &[crate::model::KeyValue]) -> HashMap<String, String> {
    items
        .iter()
        .filter(|kv| kv.enabled && !kv.key.is_empty())
        .map(|kv| (kv.key.clone(), kv.value.clone()))
        .collect()
}

// ─── Models → JSON Schema ────────────────────────────────────

/// Data model → JSON Schema (OpenAPI style, with $ref references and required).
pub fn model_to_schema(model: &DataModel, models: &[DataModel]) -> Value {
    let required: Vec<String> = model
        .fields
        .iter()
        .filter(|f| f.required.unwrap_or(false))
        .map(|f| f.name.clone())
        .collect();
    let mut obj = Map::new();
    obj.insert("type".into(), json!("object"));
    if let Some(d) = &model.description {
        obj.insert("description".into(), json!(d));
    }
    obj.insert(
        "properties".into(),
        Value::Object(fields_to_properties(&model.fields, models)),
    );
    if !required.is_empty() {
        obj.insert("required".into(), json!(required));
    }
    Value::Object(obj)
}

fn fields_to_properties(fields: &[SchemaField], models: &[DataModel]) -> Map<String, Value> {
    let mut props = Map::new();
    for f in fields {
        props.insert(f.name.clone(), field_to_schema(f, models));
    }
    props
}

fn schema_of_children(fields: &[SchemaField], models: &[DataModel]) -> Value {
    let required: Vec<String> = fields
        .iter()
        .filter(|f| f.required.unwrap_or(false))
        .map(|f| f.name.clone())
        .collect();
    let mut obj = Map::new();
    obj.insert("type".into(), json!("object"));
    obj.insert(
        "properties".into(),
        Value::Object(fields_to_properties(fields, models)),
    );
    if !required.is_empty() {
        obj.insert("required".into(), json!(required));
    }
    Value::Object(obj)
}

fn field_to_schema(f: &SchemaField, models: &[DataModel]) -> Value {
    // Model reference: non-array fields get a direct $ref; array fields emit items.$ref (fixes the old frontend's direct $ref bug)
    if let Some(rid) = &f.ref_model_id {
        if let Some(target) = models.iter().find(|m| &m.id == rid) {
            let r = format!("#/components/schemas/{}", target.name);
            if f.r#type != "array" {
                return json!({ "$ref": r });
            }
            let mut obj = Map::new();
            obj.insert("type".into(), json!("array"));
            obj.insert("items".into(), json!({ "$ref": r }));
            return Value::Object(obj);
        }
    }

    let mut obj = Map::new();
    obj.insert("type".into(), json!(f.r#type));
    if let Some(fmt) = &f.format {
        obj.insert("format".into(), json!(fmt));
    }
    if let Some(d) = &f.description {
        obj.insert("description".into(), json!(d));
    }
    if let Some(ex) = &f.example {
        if !ex.is_empty() {
            obj.insert("example".into(), json!(ex));
        }
    }
    if let Some(ev) = &f.enum_values {
        if !ev.is_empty() {
            obj.insert("enum".into(), json!(ev));
        }
    }
    match f.r#type.as_str() {
        "array" => {
            let items = match &f.children {
                Some(children) if !children.is_empty() => schema_of_children(children, models),
                _ => json!({ "type": "string" }),
            };
            obj.insert("items".into(), items);
        }
        "object" => {
            if let Some(children) = &f.children {
                if !children.is_empty() {
                    obj.insert(
                        "properties".into(),
                        Value::Object(fields_to_properties(children, models)),
                    );
                }
            }
        }
        _ => {}
    }
    Value::Object(obj)
}

// ─── Unified entry point ──────────────────────────────────────────────

/// Snapshot + export range → document text (JSON). The title is decided by the caller (collection / folder / request name).
/// For workspace-level exports the workspace description is written into the document metadata (openapi info.description / postman collection description).
pub fn export_document(
    snapshot: &Snapshot,
    format: &str,
    title: &str,
    range: &ExportRange,
) -> Result<String, DataError> {
    let data = &snapshot.data;
    let requests = collect_export_requests(data, range);
    let models = collect_referenced_models(&requests, &data.models);
    let mut spec = build_api_spec(title, &requests, &models);
    // Workspace-level export: attach the workspace description (stays None when missing; exporters omit it automatically)
    if let Some(ws_id) = &range.workspace_id {
        spec.description = data
            .workspaces
            .iter()
            .find(|w| &w.id == ws_id)
            .and_then(|w| w.description.clone());
    }
    orbit_config::exchange::export(format, &spec).map_err(|e| DataError::Validation(e.to_string()))
}
