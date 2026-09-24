//! Data model layer: plain serde data definitions aligned 1:1 with the frontend snapshot structure.
//!
//! Alignment principles:
//! - All fields use `serde(rename_all = "camelCase")`, matching the JSON shape of `web/src/data/types.ts`,
//!   so existing snapshot files load as-is (zero-cost migration) and new snapshots parse transparently in the frontend.
//! - The request model [`ApiRequest`] is a multi-protocol discriminated union: types with a fixed `protocol` (ws/grpc/tcp/udp/sse/graphql)
//!   come first, then `http` (whose `protocol` may be omitted, for legacy data), with plugin protocols (any `protocol` id, told apart by field shape alone) as the fallback.
//! - Leniency first: unknown fields are not rejected (serde ignores them by default), so future frontend fields never break reads of old snapshots.
#![allow(missing_docs)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

// ─── Snapshot shell ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u32,
    pub saved_at: i64,
    pub source: String,
    pub sync: SyncInfo,
    pub data: PersistedData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncInfo {
    pub remote_url: Option<String>,
    pub last_synced_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedData {
    // ── v2: workspace layer (project boundary) ──
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workspace_id: Option<String>,
    /// Active environment remembered per workspace
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub active_env_by_workspace: HashMap<String, Option<String>>,
    /// Global variables scoped per workspace
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub global_variables_by_workspace: HashMap<String, HashMap<String, String>>,
    /// Global secrets scoped per workspace
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub global_secrets_by_workspace: HashMap<String, HashMap<String, String>>,

    // ── Entities (each carrying its workspace_id) ──
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub requests: HashMap<String, ApiRequest>,
    #[serde(default)]
    pub models: Vec<DataModel>,
    #[serde(default)]
    pub environments: Vec<Environment>,
    /// Script library: reusable action templates (workspace-scoped; request action lists reference them via `type: ref`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_templates: Vec<ActionTemplateEntry>,
    #[serde(default)]
    pub scenarios: Vec<Scenario>,
    /// Scenario folder tree (nestable; frontend ScenarioFolder[])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_folders: Vec<ScenarioFolder>,
    /// CSV test data sets (frontend ScenarioDataSet[])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_data_sets: Vec<ScenarioDataSet>,
    /// Test suites (frontend TestSuite[])
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_suites: Vec<TestSuite>,
    #[serde(default)]
    pub plugins: Vec<PluginDescriptor>,
    #[serde(default)]
    pub history: Vec<PersistedHistoryEntry>,
    #[serde(default)]
    pub mock_rules: Vec<MockInterface>,
    /// Data source connection configs (for DB/Redis assertions; global entities shared across workspaces)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_sources: Vec<orbit_config::DataSourceConfig>,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub ui: UiPrefs,
    #[serde(default)]
    pub execution_target: ExecutionTarget,
    /// AI assistant preferences (app-level, shared across workspaces).
    ///
    /// **No secrets here**: the API Key lives separately in `<app_data_dir>/ai/credentials.yaml`
    /// (see `orbit-ai::auth::CredentialStore`) and never enters the snapshot or the git projection.
    #[serde(default)]
    pub ai: AiPrefs,

    // ── v1 legacy fields (deserialized from old snapshots only; cleared by ensure_migrated) ──
    #[serde(
        alias = "activeEnvId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub legacy_active_env_id: Option<String>,
    #[serde(
        alias = "globalVariables",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub legacy_global_variables: HashMap<String, String>,
    #[serde(
        alias = "globalSecrets",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub legacy_global_secrets: HashMap<String, String>,
}

/// Default workspace id (migration target for legacy data / entities with no owner)
pub const DEFAULT_WORKSPACE_ID: &str = "ws-default";

fn default_workspace_id() -> String {
    DEFAULT_WORKSPACE_ID.to_string()
}

impl PersistedData {
    /// v1 → v2 migration (idempotent):
    /// - No workspaces → create the default workspace; entities default to workspace_id = ws-default (serde default)
    /// - v1 global state (activeEnvId / globalVariables / globalSecrets) moves into the default workspace's per-ws maps
    /// - Missing active_workspace_id → the first workspace
    pub fn ensure_migrated(&mut self) {
        if self.workspaces.is_empty() {
            self.workspaces.push(Workspace::default_workspace());
        }
        let ws = self.workspaces[0].id.clone();
        if self.legacy_active_env_id.is_some()
            || !self.legacy_global_variables.is_empty()
            || !self.legacy_global_secrets.is_empty()
        {
            self.global_variables_by_workspace
                .entry(ws.clone())
                .or_default();
            self.global_secrets_by_workspace
                .entry(ws.clone())
                .or_default();
            for (k, v) in self.legacy_global_variables.drain() {
                self.global_variables_by_workspace
                    .get_mut(&ws)
                    .unwrap()
                    .insert(k, v);
            }
            for (k, v) in self.legacy_global_secrets.drain() {
                self.global_secrets_by_workspace
                    .get_mut(&ws)
                    .unwrap()
                    .insert(k, v);
            }
            if self.legacy_active_env_id.is_some() {
                self.active_env_by_workspace
                    .insert(ws, self.legacy_active_env_id.take());
            }
        }
        if self.active_workspace_id.is_none() && !self.workspaces.is_empty() {
            self.active_workspace_id = Some(self.workspaces[0].id.clone());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Identity color (for UI distinction)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub sort_index: i32,
}

impl Workspace {
    pub fn default_workspace() -> Self {
        Workspace {
            id: DEFAULT_WORKSPACE_ID.to_string(),
            name: "My Workspace".to_string(),
            description: None,
            color: Some("#0ea5e9".to_string()),
            created_at: 0,
            sort_index: 0,
        }
    }
}

/// Workspace data stats (shown on the selection page cards)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStats {
    pub collections: usize,
    pub models: usize,
    pub environments: usize,
    pub action_templates: usize,
    pub scenarios: usize,
    pub history: usize,
}

impl Default for PersistedData {
    fn default() -> Self {
        PersistedData {
            workspaces: vec![Workspace::default_workspace()],
            active_workspace_id: Some(DEFAULT_WORKSPACE_ID.to_string()),
            active_env_by_workspace: HashMap::new(),
            global_variables_by_workspace: HashMap::new(),
            global_secrets_by_workspace: HashMap::new(),
            collections: Vec::new(),
            requests: HashMap::new(),
            models: Vec::new(),
            environments: Vec::new(),
            action_templates: Vec::new(),
            scenarios: Vec::new(),
            scenario_folders: Vec::new(),
            scenario_data_sets: Vec::new(),
            scenario_suites: Vec::new(),
            plugins: Vec::new(),
            history: Vec::new(),
            mock_rules: Vec::new(),
            data_sources: Vec::new(),
            locale: "zh-CN".into(),
            theme: "system".into(),
            ui: UiPrefs::default(),
            execution_target: ExecutionTarget::default(),
            ai: AiPrefs::default(),
            legacy_active_env_id: None,
            legacy_global_variables: HashMap::new(),
            legacy_global_secrets: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPrefs {
    #[serde(default)]
    pub sidebar_collapsed: bool,
}

/// AI assistant preferences (app-level; secrets are not stored here).
///
/// Field notes:
/// - `provider_id`: which BYOK credential to use (an id from `orbit-ai::auth::CredentialStore`);
/// - `provider`: protocol flavor `openai` / `anthropic` (defaults to openai, works with OpenAI-compatible endpoints);
/// - Empty `model` / `base_url` fall back to that provider's defaults;
/// - `mode`: default working mode `ask` / `agent` / `plan` (the initial mode for **new sessions**;
///   each session also remembers the mode it was in, see `orbit-ai::session::AiSession::mode`).
///   Parsing and validation live in `orbit_ai::AiMode::from_tag`; a string is stored here to avoid
///   a reverse `orbit-data → orbit-ai` dependency (the same convention as `auth.type` / `priority` in this file).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiPrefs {
    /// Selected credential id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Protocol flavor: `openai` / `anthropic`.
    pub provider: String,
    /// Model name (empty = provider default).
    pub model: String,
    /// Base URL (empty = provider default).
    pub base_url: String,
    /// Max output tokens per turn (not exposed in the settings UI yet; driven by the default value).
    pub max_tokens: u32,
    /// Reply language: `zh` / `en`.
    pub language: String,
    /// Default working mode for new sessions: `ask` / `agent` / `plan`.
    pub mode: String,
    /// Max model round-trips per turn.
    pub max_rounds: u32,
}

impl Default for AiPrefs {
    fn default() -> Self {
        Self {
            provider_id: None,
            provider: "openai".to_string(),
            model: String::new(),
            base_url: String::new(),
            max_tokens: 4096,
            language: "zh".to_string(),
            mode: "agent".to_string(),
            max_rounds: 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTarget {
    pub mode: String,
    pub agent_ids: Option<Vec<String>>,
}

impl Default for ExecutionTarget {
    fn default() -> Self {
        ExecutionTarget {
            mode: "local".into(),
            agent_ids: None,
        }
    }
}

// ─── Collections / request tree ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc: Option<GrpcCollectionMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionConfig>,
    #[serde(default)]
    pub items: Vec<CollectionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CollectionItem {
    #[serde(rename = "folder")]
    Folder {
        id: String,
        name: String,
        #[serde(default)]
        items: Vec<CollectionItem>,
    },
    #[serde(rename = "request")]
    Request {
        id: String,
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "grpc-package")]
    GrpcPackage {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proto: Option<String>,
        #[serde(default)]
        services: Vec<GrpcServiceNode>,
    },
    #[serde(rename = "grpc-service")]
    GrpcService {
        id: String,
        #[serde(rename = "packageName")]
        package_name: String,
        name: String,
        #[serde(default)]
        methods: Vec<GrpcRpcNode>,
    },
    #[serde(rename = "grpc-rpc")]
    GrpcRpc {
        id: String,
        #[serde(rename = "requestId")]
        request_id: String,
    },
}

impl CollectionItem {
    /// Node id (uniform accessor across variants)
    pub fn id(&self) -> &str {
        match self {
            CollectionItem::Folder { id, .. }
            | CollectionItem::Request { id, .. }
            | CollectionItem::GrpcPackage { id, .. }
            | CollectionItem::GrpcService { id, .. }
            | CollectionItem::GrpcRpc { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcCollectionMeta {
    pub source: GrpcSource,
    #[serde(default)]
    pub packages: Vec<GrpcPackageNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum GrpcSource {
    Proto { files: Vec<ProtoFile> },
    Reflection { target: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtoFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcPackageNode {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    #[serde(default)]
    pub services: Vec<GrpcServiceNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcServiceNode {
    pub name: String,
    #[serde(default)]
    pub methods: Vec<GrpcRpcNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcRpcNode {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

// ─── Connection config ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<TcpFraming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<WsFrameType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_after: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Plugin protocol extension fields (driven by connectionConfigSchema), coexisting with the named fields
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpFraming {
    pub mode: TcpFramingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_len: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub big_endian: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TcpFramingMode {
    Delimiter,
    Fixed,
    ReadUntilClose,
    LengthPrefix,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TlsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insecure_skip_verify: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert: Option<String>,
}

// ─── Generic key-values / auth / body ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct KeyValue {
    pub id: String,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileSpec>,
    /// form-data row mode: text = literal value, file = file upload
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSpec {
    pub name: String,
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BodyMode {
    #[default]
    None,
    Json,
    Xml,
    FormData,
    XWwwFormUrlencoded,
    Raw,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadType {
    Text,
    Base64,
    Hex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WsFrameType {
    Text,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrpcStreamMode {
    ServerStreaming,
    ClientStreaming,
    Bidirectional,
}

// ─── Requests (multi-protocol discriminated union) ────────────────────────────────

/// Deserialization helper: validates that the `protocol` field of a built-in protocol struct holds its fixed value,
/// so lenient fields (url/headers/protocol are all strings) cannot swallow other protocols' JSON during untagged matching.
macro_rules! de_protocol_fn {
    ($name:ident, $expected:literal) => {
        fn $name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
            let s = String::deserialize(d)?;
            if s == $expected {
                Ok(s)
            } else {
                Err(serde::de::Error::custom(concat!(
                    "protocol field must be ",
                    $expected
                )))
            }
        }
    };
}

de_protocol_fn!(de_ws_protocol, "websocket");
de_protocol_fn!(de_grpc_protocol, "grpc");
de_protocol_fn!(de_tcp_protocol, "tcp");
de_protocol_fn!(de_udp_protocol, "udp");
de_protocol_fn!(de_sse_protocol, "sse");
de_protocol_fn!(de_graphql_protocol, "graphql");

/// Protocol-agnostic unified request. Match order: HTTP first (its required `method` field reliably excludes other protocols),
/// then the built-in protocols with a fixed `protocol`, with plugin protocols (any protocol id, told apart by field shape) as fallback.
/// Note: built-in protocols must not precede HTTP — their lenient fields (url/headers/protocol are all strings) would swallow HTTP JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiRequest {
    Http(HttpRequest),
    Ws(WsRequest),
    Grpc(GrpcRequest),
    Tcp(TcpRequest),
    Udp(UdpRequest),
    Sse(SseRequest),
    Graphql(GraphqlRequest),
    Plugin(PluginRequest),
}

impl ApiRequest {
    pub fn protocol(&self) -> &str {
        match self {
            ApiRequest::Ws(_) => "websocket",
            ApiRequest::Grpc(_) => "grpc",
            ApiRequest::Tcp(_) => "tcp",
            ApiRequest::Udp(_) => "udp",
            ApiRequest::Sse(_) => "sse",
            ApiRequest::Graphql(_) => "graphql",
            ApiRequest::Http(_) => "http",
            ApiRequest::Plugin(p) => p.protocol.as_str(),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            ApiRequest::Ws(r) => &r.id,
            ApiRequest::Grpc(r) => &r.id,
            ApiRequest::Tcp(r) => &r.id,
            ApiRequest::Udp(r) => &r.id,
            ApiRequest::Sse(r) => &r.id,
            ApiRequest::Graphql(r) => &r.id,
            ApiRequest::Http(r) => &r.id,
            ApiRequest::Plugin(r) => &r.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            ApiRequest::Ws(r) => &r.name,
            ApiRequest::Grpc(r) => &r.name,
            ApiRequest::Tcp(r) => &r.name,
            ApiRequest::Udp(r) => &r.name,
            ApiRequest::Sse(r) => &r.name,
            ApiRequest::Graphql(r) => &r.name,
            ApiRequest::Http(r) => &r.name,
            ApiRequest::Plugin(r) => &r.name,
        }
    }

    /// Pre-request script (field shared by all protocols)
    pub fn prereq_script(&self) -> Option<&str> {
        match self {
            ApiRequest::Ws(r) => r.prereq_script.as_deref(),
            ApiRequest::Grpc(r) => r.prereq_script.as_deref(),
            ApiRequest::Tcp(r) => r.prereq_script.as_deref(),
            ApiRequest::Udp(r) => r.prereq_script.as_deref(),
            ApiRequest::Sse(r) => r.prereq_script.as_deref(),
            ApiRequest::Graphql(r) => r.prereq_script.as_deref(),
            ApiRequest::Http(r) => r.prereq_script.as_deref(),
            ApiRequest::Plugin(r) => r.prereq_script.as_deref(),
        }
    }

    /// Post-request script (field shared by all protocols)
    pub fn postreq_script(&self) -> Option<&str> {
        match self {
            ApiRequest::Ws(r) => r.postreq_script.as_deref(),
            ApiRequest::Grpc(r) => r.postreq_script.as_deref(),
            ApiRequest::Tcp(r) => r.postreq_script.as_deref(),
            ApiRequest::Udp(r) => r.postreq_script.as_deref(),
            ApiRequest::Sse(r) => r.postreq_script.as_deref(),
            ApiRequest::Graphql(r) => r.postreq_script.as_deref(),
            ApiRequest::Http(r) => r.postreq_script.as_deref(),
            ApiRequest::Plugin(r) => r.postreq_script.as_deref(),
        }
    }

    /// Pre-resolution action list (field shared by all protocols)
    pub fn pre_resolve_actions(&self) -> &[orbit_config::RequestAction] {
        match self {
            ApiRequest::Ws(r) => &r.pre_resolve_actions,
            ApiRequest::Grpc(r) => &r.pre_resolve_actions,
            ApiRequest::Tcp(r) => &r.pre_resolve_actions,
            ApiRequest::Udp(r) => &r.pre_resolve_actions,
            ApiRequest::Sse(r) => &r.pre_resolve_actions,
            ApiRequest::Graphql(r) => &r.pre_resolve_actions,
            ApiRequest::Http(r) => &r.pre_resolve_actions,
            ApiRequest::Plugin(r) => &r.pre_resolve_actions,
        }
    }

    /// Pre-request action list (single list: scripts / DB queries / built-in interpolation node; order is execution order)
    pub fn pre_actions(&self) -> &[orbit_config::RequestAction] {
        match self {
            ApiRequest::Ws(r) => &r.pre_actions,
            ApiRequest::Grpc(r) => &r.pre_actions,
            ApiRequest::Tcp(r) => &r.pre_actions,
            ApiRequest::Udp(r) => &r.pre_actions,
            ApiRequest::Sse(r) => &r.pre_actions,
            ApiRequest::Graphql(r) => &r.pre_actions,
            ApiRequest::Http(r) => &r.pre_actions,
            ApiRequest::Plugin(r) => &r.pre_actions,
        }
    }

    /// Post-request action list (field shared by all protocols)
    pub fn post_actions(&self) -> &[orbit_config::RequestAction] {
        match self {
            ApiRequest::Ws(r) => &r.post_actions,
            ApiRequest::Grpc(r) => &r.post_actions,
            ApiRequest::Tcp(r) => &r.post_actions,
            ApiRequest::Udp(r) => &r.post_actions,
            ApiRequest::Sse(r) => &r.post_actions,
            ApiRequest::Graphql(r) => &r.post_actions,
            ApiRequest::Http(r) => &r.post_actions,
            ApiRequest::Plugin(r) => &r.post_actions,
        }
    }

    /// Back-compat normalization: merges the previous two-phase field (`pre_resolve_actions`) into the single list `pre_actions`,
    /// placing it **before** the built-in interpolation node (otherwise existing "pre-resolution actions" would turn into post-interpolation semantics), then clears the compat field.
    ///
    /// - **Read-only, never written again**: after merging, `pre_resolve_actions` is empty so serialization drops the key naturally;
    /// - **Idempotent**: returns immediately when the compat field is empty (safe to call both on load and on upsert).
    pub fn merge_pre_resolve_actions(&mut self) {
        let mut lists = self.actions_mut();
        // Fixed order: pre-resolution (compat) / post-resolution (single list) / post
        if lists.len() < 2 || lists[0].is_empty() {
            return;
        }
        let compat = std::mem::take(&mut *lists[0]);
        let base = std::mem::take(&mut *lists[1]);
        *lists[1] = orbit_config::merge_pre_actions(&base, None, &compat, None);
    }

    /// Action lists for each protocol (mutable access, used to cascade-clean dangling references).
    ///
    /// Fixed order: pre-resolution (compat field) / post-resolution (single list) / post.
    pub fn actions_mut(&mut self) -> Vec<&mut Vec<orbit_config::RequestAction>> {
        match self {
            ApiRequest::Ws(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Grpc(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Tcp(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Udp(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Sse(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Graphql(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Http(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
            ApiRequest::Plugin(r) => vec![
                &mut r.pre_resolve_actions,
                &mut r.pre_actions,
                &mut r.post_actions,
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub query_params: Vec<KeyValue>,
    #[serde(default)]
    pub path_params: Vec<KeyValue>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub body_mode: BodyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_by_mode: Option<HashMap<String, String>>,
    #[serde(default)]
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(default)]
    pub form_params: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_file: Option<FileSpec>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub cookies: Vec<CookieItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses: Option<Vec<ResponseDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Post-request assertion config (built-in + DB/Redis, same shape as orbit-config Check; extract first, then assert)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<orbit_config::Check>,
}

impl Default for HttpRequest {
    fn default() -> Self {
        HttpRequest {
            id: String::new(),
            name: String::new(),
            protocol: Some("http".into()),
            method: "GET".into(),
            url: String::new(),
            headers: Vec::new(),
            query_params: Vec::new(),
            path_params: Vec::new(),
            body: String::new(),
            body_mode: BodyMode::None,
            body_by_mode: None,
            content_type: String::new(),
            request_format: None,
            response_format: None,
            form_params: Vec::new(),
            binary_file: None,
            auth: AuthConfig {
                r#type: "none".into(),
                ..Default::default()
            },
            cookies: Vec::new(),
            prereq_script: None,
            postreq_script: None,
            pre_resolve_actions: Vec::new(),
            pre_actions: Vec::new(),
            post_actions: Vec::new(),
            responses: None,
            model_id: None,
            assertions: Vec::new(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig {
            r#type: "none".into(),
            token: None,
            username: None,
            password: None,
            key: None,
            value: None,
            add_to: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieItem {
    pub id: String,
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDef {
    pub id: String,
    pub name: String,
    pub status: u16,
    pub content_type: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_ws_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub messages: Vec<WsMessageSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_after: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsMessageSpec {
    pub id: String,
    #[serde(default)]
    pub payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<WsFrameType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_script: Option<String>,
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_grpc_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<String>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<GrpcStreamMode>,
    #[serde(default)]
    pub metadata: Vec<KeyValue>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_tcp_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<TcpFraming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<WsMessageSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UdpRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_udp_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<WsMessageSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_sse_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_events: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlRequest {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "de_graphql_protocol")]
    pub protocol: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRequest {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prereq_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postreq_script: Option<String>,
    /// **Compat read-only field (deprecated)**: the "pre-resolution actions" list from the previous two-phase change.
    ///
    /// Merged into `pre_actions` (before the built-in interpolation node) and cleared when the snapshot loads,
    /// see [`ApiRequest::merge_pre_resolve_actions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_resolve_actions: Vec<orbit_config::RequestAction>,
    /// Pre-request action list (= post-resolution actions; applied to the final payload, so rewrites are the final bytes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_actions: Vec<orbit_config::RequestAction>,
    /// Post-request action list (executed in order after the response arrives)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_actions: Vec<orbit_config::RequestAction>,
}

// ─── Data models (structured as JSON Schema) ────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataModel {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaField {
    pub id: String,
    pub name: String,
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<SchemaField>>,
}

impl Default for SchemaField {
    fn default() -> Self {
        SchemaField {
            id: String::new(),
            name: String::new(),
            r#type: "string".into(),
            format: None,
            required: None,
            example: None,
            description: None,
            enum_values: None,
            ref_model_id: None,
            children: None,
        }
    }
}

// ─── Script library (reusable action templates) ──────────────────────────────

/// Library entry plus workspace ownership.
///
/// The entry body ([`orbit_config::ActionTemplate`]) deliberately has no notion of a workspace: that way the engine and export layer
/// can take a library list without depending on this crate (same layering as `orbit_config::DataSourceConfig`).
/// Serialized shape matches the frontend: `workspaceId` plus the entry fields **flattened** (isomorphic with [`Environment`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTemplateEntry {
    /// Owning workspace
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    /// Entry body
    #[serde(flatten)]
    pub template: orbit_config::ActionTemplate,
}

impl ActionTemplateEntry {
    /// Builds an entry owned by the given workspace
    pub fn new(workspace_id: impl Into<String>, template: orbit_config::ActionTemplate) -> Self {
        ActionTemplateEntry {
            workspace_id: workspace_id.into(),
            template,
        }
    }
}

// ─── Environments ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default)]
    pub secrets: HashMap<String, String>,
}

// ─── History (persisted form: metadata only, no response bodies) ───────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedHistoryEntry {
    pub id: String,
    pub request_id: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration: Option<u64>,
    pub size: Option<u64>,
    pub timestamp: i64,
}

// ─── Automation scenarios ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
    /// Owning folder (default = root)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// Priority p0–p3 (default p2)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// Runtime environment id (default = unspecified, no environment variables injected)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_id: Option<String>,
    /// Bound CSV test data set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_set_id: Option<String>,
    /// Whether data-driven mode is enabled (default false)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_data_set: Option<bool>,
    /// Iteration count (default 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u32>,
    /// Failure policy stop / continue / next-loop (default stop)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_error: Option<String>,
    /// Whether to record request details (run-config switch; default false)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_request_details: Option<bool>,
}

/// Scenario folder (nestable; frontend ScenarioFolder)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioFolder {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    /// Parent folder (default = root level)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
}

/// CSV test data set (frontend ScenarioDataSet; columns = variables, rows = one iteration)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioDataSet {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    /// Raw CSV text, the first row is the header
    #[serde(default)]
    pub csv: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub row_count: usize,
    /// Read mode sequential / random / shuffle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub updated_at: i64,
}

/// Test suite: a static composition of scenarios (frontend TestSuite)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSuite {
    pub id: String,
    pub name: String,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Suite runtime environment (overrides each member scenario's own environment)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_id: Option<String>,
    /// Run mode serial / parallel
    #[serde(default = "default_run_mode")]
    pub run_mode: String,
    /// Parallel concurrency (effective when run_mode = parallel, 1–10)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// Member scenario ids (executed in array order)
    #[serde(default)]
    pub member_ids: Vec<String>,
    #[serde(default)]
    pub updated_at: i64,
}

fn default_run_mode() -> String {
    "serial".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ScenarioStep {
    Request(StepRequest),
    Loop(StepLoop),
    Condition(StepCondition),
    Wait(StepWait),
    Group(StepGroup),
    Setvar(StepSetVar),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepBase {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRequest {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepLoop {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub children: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCondition {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default)]
    pub expr: String,
    #[serde(default)]
    pub children: Vec<ScenarioStep>,
    #[serde(default)]
    pub else_children: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepWait {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default)]
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepGroup {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default)]
    pub children: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepSetVar {
    #[serde(flatten)]
    pub base: StepBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_value: Option<String>,
}

// ─── Plugin descriptors (aligned with orbit-plugin::PluginDescriptor) ────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_config_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_config_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_dir: Option<String>,
}

// ─── Mock rules ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockInterface {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default = "default_workspace_id")]
    pub workspace_id: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub expectations: Vec<MockExpectation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockExpectation {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<MockCondition>,
    #[serde(default)]
    pub ip_condition: MockIpCondition,
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockCondition {
    pub location: String,
    pub name: String,
    pub op: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockIpCondition {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ip: String,
}
