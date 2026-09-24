// ─── Core domain types ──────────────────────────────────────

import { t } from "@/lib/localeDict";

export type Locale = "zh-CN" | "en-US";

/** Workspace (project boundary): its own collections/models/environments/automation/history/reports. Aligned with Postman/Apifox. */
export interface Workspace {
  id: string;
  name: string;
  description?: string;
  /** Accent color (distinguishes projects in the UI) */
  color?: string;
  createdAt: number;
  sortIndex: number;
}

/** Domain data of a single workspace (used for the store partition cache / snapshot partitioning).
 *  `requests` is a global map (ids are globally unique) — partitioning collects the requests referenced by this
 *  workspace's collections/scenarios for operations like "duplicate workspace"; day-to-day display still uses the global map. */
export interface WorkspaceData {
  collections: Collection[];
  requests: Record<string, ApiRequest>;
  models: DataModel[];
  environments: Environment[];
  /** Script library items (reusable action templates; migrate with the workspace and are stamped with a workspaceId on save) */
  actionTemplates: ActionTemplate[];
  scenarios: Scenario[];
  /** Scenario folder tree (may be nested) */
  scenarioFolders: ScenarioFolder[];
  /** CSV test data sets (columns = variables, rows = one iteration each) */
  scenarioDataSets: ScenarioDataSet[];
  /** Test suites (static groups of scenarios, referenced across folders) */
  scenarioSuites: TestSuite[];
  history: HistoryEntry[];
  activeEnvId: string | null;
  globalVariables: Record<string, string>;
  globalSecrets: Record<string, string>;
}

/** Dynamic-value data locale (locale used to generate fake data; aligned with the backend `orbit_dynamic` locale param) */
export type DataLocale = "zh" | "en" | "ja";

export type ModuleKey =
  | "api"
  | "automation"
  | "load"
  | "performance"
  | "plugins"
  | "history"
  | "analytics"
  | "distributed"
  | "datasource"
  | "actionlib";

/** Data source kind */
export type DataSourceKind = "mysql" | "postgres" | "sqlite" | "redis";

/** Data source connection config (aligned with the backend orbit-config DataSourceConfig camelCase JSON).
 *  The password is stored locally in plain text and always masked in lists / exports; `{{env:VAR}}` references are supported. */
export interface DataSource {
  id: string;
  name: string;
  kind: DataSourceKind;
  /** Connection string: mysql:// / postgres:// / sqlite:... / redis:// */
  url: string;
  username?: string;
  password?: string;
  /** Maximum connections in the pool */
  maxConnections: number;
  /** Minimum idle connections in the pool */
  minIdle: number;
  /** Connect timeout (ms) */
  connectTimeoutMs: number;
  /** Acquire-from-pool timeout (ms) */
  acquireTimeoutMs: number;
  /** Per-query timeout (ms) */
  queryTimeoutMs: number;
  /** Idle reclamation (seconds) */
  idleTtlSecs: number;
  /** Read-only guard: only SELECT / read-only Redis commands are allowed */
  readonly: boolean;
  enabled: boolean;
}

export interface KeyValue {
  id: string;
  key: string;
  value: string;
  enabled: boolean;
  description?: string;
  /** Marks a header as system-appended (a default header); the UI can hide these as a group while still allowing edits */
  auto?: boolean;
  /** A form-data row used as a file upload: name = key, type is the MIME type.
   * - Tauri desktop: `path` holds the real absolute path (no pre-read) and `data` is empty;
   * - Browser: `data` holds base64 (without the `data:` prefix) and `path` is empty. */
  file?: { name: string; type: string; path?: string; data?: string } | null;
  /** form-data row mode: text = text value, file = file upload; only used by form-data, defaults to text */
  type?: "text" | "file";
}

export interface CookieItem {
  id: string;
  name: string;
  value: string;
  domain: string;
  path: string;
  enabled: boolean;
}

export type AuthType = "none" | "bearer" | "basic" | "apikey" | "oauth2";

export interface AuthConfig {
  type: AuthType;
  token?: string; // supports {{var}}
  username?: string;
  password?: string;
  key?: string;
  value?: string;
  addTo?: "header" | "query";
}

export type BodyMode =
  | "none"
  | "json"
  | "xml"
  | "form-data"
  | "x-www-form-urlencoded"
  | "raw"
  | "binary";

export interface HttpRequest {
  id: string;
  name: string;
  /** Request protocol (defaults to http for legacy data) */
  protocol?: "http";
  method: string;
  url: string;
  headers: KeyValue[];
  queryParams: KeyValue[];
  /** Path params (the `{name}` placeholders in the url): the name is read-only, the value is editable;
   *  kept in sync by url parsing and substituted back into the url when sending. */
  pathParams?: KeyValue[];
  body: string;
  bodyMode: BodyMode;
  /** Body content stored independently per text mode (json/xml/raw) so switching modes never interferes;
   *  falls back to the `body` field when missing (legacy data compat). */
  bodyByMode?: Partial<Record<BodyMode, string>>;
  contentType: string;
  /** Request body format hint (json/msgpack/protobuf/xml/form...), passed through to the backend codec router */
  requestFormat?: string;
  /** Response format hint (defaults to inferring from the response Content-Type) */
  responseFormat?: string;
  /** Structured key/values for form-data / x-www-form-urlencoded (including file rows) */
  formParams: KeyValue[];
  /** File chosen in binary mode:
   * - Tauri desktop: `path` holds the real absolute path (no pre-read) and `data` is empty;
   * - Browser: `data` holds base64 (without the `data:` prefix) and `path` is empty. */
  binaryFile: {
    name: string;
    type: string;
    path?: string;
    data?: string;
  } | null;
  auth: AuthConfig;
  cookies: CookieItem[];
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  /** Post-response action list (executed in order after the response); falls back to postreqScript when empty */
  postActions?: RequestAction[];
  responses?: ResponseDef[];
  modelId?: string | null;
  /** Post-response assertions (built-in + DB/Redis); sent with the request and evaluated after the response */
  assertions?: Assertion[];
}

// ─── Network protocols (multi-protocol requests)────────────────────────────────

export type ProtocolKind =
  "http" | "websocket" | "grpc" | "tcp" | "udp" | "sse" | "graphql";

export type PayloadType = "text" | "base64" | "hex";
export type WsFrameType = "text" | "binary";
export type GrpcStreamMode =
  "server_streaming" | "client_streaming" | "bidirectional";
export type TcpFramingMode =
  "delimiter" | "fixed" | "read_until_close" | "length_prefix";

/** A single message in a persistent-connection message sequence */
export interface WsMessageSpec {
  id: string;
  payload: string;
  payloadType?: PayloadType;
  /** WebSocket only: text frame / binary frame */
  messageType?: WsFrameType;
  preScript?: string;
  postScript?: string;
  waitMs?: number;
}

export interface TcpFraming {
  mode: TcpFramingMode;
  delimiter?: string;
  fixedLen?: number;
  bigEndian?: boolean;
}

/** TLS options (aligned with the backend `orbit-protocol::types::TlsOptions`) */
export interface TlsOptions {
  insecureSkipVerify?: boolean;
  caCert?: string;
  sni?: string;
  clientCert?: string;
}

/** Connection config of a connection-based (non-HTTP) collection: the single source of truth at connection level; message nodes inside inherit it.
 * Built-in protocols use the named fields below; plugin protocols extend it via [key: string] (driven by connectionConfigSchema). */
export interface ConnectionConfig {
  /** Target address (tcp://host:port, ws://..., grpc://...) */
  url?: string;
  /** TCP framing protocol */
  framing?: TcpFraming;
  /** WebSocket frame type */
  messageType?: WsFrameType;
  /** Close the WebSocket after receiving N messages */
  closeAfter?: number;
  tls?: TlsOptions;
  /** Connection-level default codecs (request_format / response_format) */
  codec?: string;
  timeoutMs?: number;
  /** Plugin protocol extension fields (rendered from connectionConfigSchema) */
  [key: string]: unknown;
}

export interface WsRequest {
  id: string;
  name: string;
  protocol: "websocket";
  url: string;
  /** Handshake request headers (configured before connecting and passed when the session opens) */
  headers: KeyValue[];
  messages: WsMessageSpec[];
  closeAfter?: number;
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

export interface GrpcRequest {
  id: string;
  name: string;
  protocol: "grpc";
  /** Server address (mirrors the HTTP url field) */
  url: string;
  /** Legacy compat field: shaped like `pkg.Service` or `pkg.Service/Method` */
  service?: string;
  /** rpc method name */
  method?: string;
  /** Owning package name (used to build /pkg.Service/Method) */
  packageName?: string;
  /** Owning service name */
  serviceName?: string;
  /** Fully-qualified input message name (e.g. .pkg.Request) */
  inputType?: string;
  /** Fully-qualified output message name */
  outputType?: string;
  /** Metadata (headers) passed as gRPC call metadata (used by the protocol panel editor) */
  headers: KeyValue[];
  /** Request body message as JSON */
  message?: string;
  /** JSON template generated from the backend schema (for the Message editor's "generate template") */
  messageTemplate?: string;
  /** Message format (json/protobuf) */
  messageFormat?: string;
  responseFormat?: string;
  streaming?: GrpcStreamMode;
  /** gRPC metadata (mirrors HTTP headers) */
  metadata: KeyValue[];
  auth: AuthConfig;
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

export interface TcpRequest {
  id: string;
  name: string;
  protocol: "tcp";
  url: string;
  payload?: string;
  payloadType?: PayloadType;
  framing?: TcpFraming;
  messages?: WsMessageSpec[];
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

export interface UdpRequest {
  id: string;
  name: string;
  protocol: "udp";
  url: string;
  payload?: string;
  payloadType?: PayloadType;
  messages?: WsMessageSpec[];
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

export interface SseRequest {
  id: string;
  name: string;
  protocol: "sse";
  url: string;
  headers: KeyValue[];
  maxEvents?: number;
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

export interface GraphqlRequest {
  id: string;
  name: string;
  protocol: "graphql";
  url: string;
  query?: string;
  variables?: string;
  operationName?: string;
  headers: KeyValue[];
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

/** Plugin protocol request (native/wasm dynamic protocol, single-request model)
 * protocol = the protocol id registered by the plugin (e.g. "pg"); connection params live in the collection config, message params pass through `options`. */
export interface PluginRequest {
  id: string;
  name: string;
  /** Plugin protocol id (a dynamic registry id, not part of the built-in enum) */
  protocol: string;
  url: string;
  headers: KeyValue[];
  /** Plugin request params (driven by requestConfigSchema) */
  options?: Record<string, unknown>;
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework: **read-compat only**;
   * merged before the built-in interpolation node during normalization (see `getPreActions`); new write paths only write `preActions`.
   */
  preResolveActions?: RequestAction[];
  /** Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; the order is the execution order) */
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

/** Protocol-agnostic unified request (corresponds to the backend RequestSpec) */
export type ApiRequest =
  | HttpRequest
  | WsRequest
  | GrpcRequest
  | TcpRequest
  | UdpRequest
  | SseRequest
  | GraphqlRequest
  | PluginRequest;

export function requestProtocol(req: ApiRequest): string {
  return req.protocol ?? "http";
}

export function isHttpRequest(req: ApiRequest): req is HttpRequest {
  return requestProtocol(req) === "http";
}

/** Create a default request for the given protocol (an unknown protocol means a plugin protocol, using the generic PluginRequest shape) */
export function createRequest(
  protocol: string,
  name = t("request.new", "New request"),
): ApiRequest {
  const id = `req-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
  switch (protocol) {
    case "websocket":
      return {
        id,
        name,
        protocol: "websocket",
        url: "",
        headers: [],
        messages: [],
        closeAfter: 1,
      };
    case "grpc":
      return {
        id,
        name,
        protocol: "grpc",
        url: "",
        service: "",
        headers: [],
        metadata: [],
        auth: { type: "none" },
        messageFormat: "json",
      };
    case "tcp":
      return { id, name, protocol: "tcp", url: "" };
    case "udp":
      return { id, name, protocol: "udp", url: "" };
    case "sse":
      return { id, name, protocol: "sse", url: "", headers: [] };
    case "graphql":
      return { id, name, protocol: "graphql", url: "", headers: [] };
    case "http":
      return {
        id,
        name,
        protocol: "http",
        method: "GET",
        url: "",
        headers: [],
        queryParams: [],
        body: "",
        bodyMode: "none",
        contentType: "",
        formParams: [],
        binaryFile: null,
        auth: { type: "none" },
        cookies: [],
      };
    default:
      // Plugin protocol: single request (connection params live in the collection, message params pass through options)
      return { id, name, protocol, url: "", headers: [] };
  }
}

export interface ResponseDef {
  id: string;
  name: string;
  status: number;
  contentType: string;
  body: string;
  description?: string;
  /** Raw schema kept on import (including field descriptions, shown as comments in examples) */
  schema?: any;
}

/** Snapshot of the request actually sent, after variable resolution and query assembly */
export interface SentRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
  /** Metadata needed by "request code" generation to reconstruct structured bodies such as multipart / binary */
  bodyMode?: BodyMode;
  formParams?: KeyValue[];
  /** Keeps only the file name, MIME and (under Tauri) the real path; commands reference local files as @path/@filename */
  binaryFile?: { name: string; type: string; path?: string } | null;
}

export interface HttpResponse {
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: string;
  duration: number;
  size: number;
  timing?: {
    dns: number;
    connect: number;
    tls: number;
    ttfb: number;
    download: number;
  };
  /** The fully built request (url+query, variables already resolved) */
  request?: SentRequest;
  /** Pre-request script console logs */
  preLogs?: ScriptLog[];
  /** Post-response script console logs */
  postLogs?: ScriptLog[];
  /** Post-response script assertion results */
  postTests?: TestResult[];
  /** Variables written by scripts (pre/post); the caller decides whether to persist them to the environment */
  varsSet?: Record<string, string>;
  /** Temporary variables written by scripts via pm.variables.set (lifetime of this request), never persisted */
  tempVarsSet?: Record<string, string>;
  /** Pre-request action execution results (a single list including the built-in interpolation node entry, in execution order) */
  preActions?: ActionLog[];
  /** Post-response action execution results (scripts / database, in configured order) */
  postActions?: ActionLog[];
  /** Variables written by actions (database queries) */
  actionVars?: Record<string, string>;
}

/** Script console log (shared by pre/post scripts) */
export interface ScriptLog {
  level: string;
  message: string;
}

/** Execution log of a single pre/post action (script / database query) */
export interface ActionLog {
  /** Stage: pre_resolve (before the built-in interpolation node) / interpolate (the built-in node) / pre (after it) / post */
  phase: string;
  /** Kind: script / db / interpolate / ref (ref = a dangling, unresolved script library reference) */
  kind: string;
  name: string;
  ok: boolean;
  elapsedMs: number;
  varsWritten: Record<string, string>;
  /** Result summary (database: rows/columns; failure: the reason) */
  detail: string;
  /** Console logs of the script action */
  logs: ScriptLog[];
}

/** Post-response script assertion results */
export interface TestResult {
  name: string;
  passed: boolean;
  message: string;
}

export interface AssertionResult {
  name: string;
  passed: boolean;
  message: string;
  /** Whether it is a hard assertion (a failure counts as an error) */
  isHard?: boolean;
  /** Variable the DB/Redis assertion result is stored into */
  exportedVars?: Record<string, string>;
}

// ─── Post-response assertion config (the wire form matches orbit-config::Check JSON exactly)───

export type AssertionComparator =
  | "equal"
  | "not_equal"
  | "contains"
  | "not_contains"
  | "exists"
  | "matches"
  | "gt"
  | "lt";

export type AssertionKind =
  | "status"
  | "body_contains"
  | "duration_lt"
  | "size_lt"
  | "jsonpath"
  | "jmespath"
  | "regex"
  | "xpath"
  | "header"
  | "css_selector"
  | "jsonschema"
  | "db"
  | "redis";

export type DbTargetKind =
  "row_count" | "scalar" | "cell" | "row" | "json_path";

/** Polling retry (wire keys are snake_case) */
export interface AssertionRetry {
  interval_ms?: number;
  max_attempts?: number;
  timeout_ms?: number | null;
}

export interface AssertionMeta {
  name?: string;
  enabled?: boolean;
}

/** How a DB assertion reads its value */
export interface AssertionDbTarget {
  type: DbTargetKind;
  row?: number;
  column?: string;
  path?: string;
}

/** Assertion config (type + variant params + meta). Field names / value types align with the backend orbit-config::Check */
export interface Assertion {
  type: AssertionKind;
  meta?: AssertionMeta;
  /** status: expected status code / size_lt: max bytes / duration_lt: duration text / body_contains: substring */
  value?: number | string;
  /** jsonpath / xpath expression */
  path?: string;
  /** jmespath expression */
  expression?: string;
  /** regex pattern */
  pattern?: string;
  /** css_selector value */
  selector?: string;
  /** jsonschema definition */
  schema?: string;
  /** Response header name for a header assertion (backend field `name`) */
  name?: string;
  comparator?: AssertionComparator;
  expected?: string;
  // ── db ──
  datasource?: string;
  sql?: string;
  target?: AssertionDbTarget;
  retry?: AssertionRetry;
  extract_var?: string;
  hard?: boolean;
  // ── redis ──
  command?: string;
  args?: string[];
}

// ─── Pre/post request actions (JS scripts / read-only database queries)───────────────

/** Multi-column mapping entry: writes the `column` of the result row into the variable `var` */
export interface ActionColumnVar {
  column: string;
  var: string;
}

/** Script action: aligned with the backend `RequestAction::Script` */
export interface ScriptActionItem {
  /** Stable frontend id (for drag & drop / React keys; stripped when sent to the backend) */
  id: string;
  kind: "script";
  name?: string;
  enabled: boolean;
  /** Script language (currently js only) */
  language?: "js";
  code: string;
}

/** Database action: aligned with the backend `RequestAction::Db` (read-only queries only) */
export interface DbActionItem {
  id: string;
  kind: "db";
  name?: string;
  enabled: boolean;
  /** Data source id / name */
  datasource: string;
  /** Read-only SQL for relational databases (one of sql or command) */
  sql?: string;
  /** Read-only Redis command */
  command?: string;
  args?: string[];
  /** How a single value is read (defaults to scalar) */
  target?: AssertionDbTarget;
  /** Variable name the single value is written to */
  extractVar?: string;
  /** Multi-column mapping: writes the columns of row `row` into several variables */
  columns?: ActionColumnVar[];
  /** Which row the multi-column mapping reads (defaults to 0) */
  row?: number;
  /** Polling retry (waiting for data to become ready) */
  retry?: AssertionRetry;
}

/**
 * The built-in "interpolate" node: turns the request template into the final message (variable interpolation + body assembly/encoding).
 *
 * Aligned with the backend `RequestAction::Interpolate` (YAML `{ type: interpolate }`).
 * Maintained by the system: cannot be edited / deleted / disabled / dragged; actions **before** it are pre-interpolation (they may write variables
 * for this round to consume and may rewrite the template), actions **after** it are post-interpolation (rewrites are the final bytes — ideal for signing / encryption).
 */
export interface InterpolateActionItem {
  /** Fixed id (see `INTERPOLATE_ACTION_ID`), stable across renders and persistence */
  id: string;
  kind: "interpolate";
}

/**
 * Script library reference item: a request stores only a **reference**, expanded into the library item's current content before execution / export.
 *
 * Aligned with the backend `RequestAction::Ref` (YAML `{ type: ref, library_id }`).
 * A deleted library item means a dangling reference: the editor shows it as broken and offers "reselect / delete",
 * and execution logs one error without aborting the request (never silently dropped).
 */
export interface RefActionItem {
  id: string;
  kind: "ref";
  /** Library item id */
  libraryId: string;
  /** Optional display alias (defaults to the library item name) */
  name?: string;
  enabled: boolean;
}

/** A pre/post action (one entry of the ordered list: script / database query / built-in interpolation node / script library reference) */
export type RequestAction =
  ScriptActionItem | DbActionItem | InterpolateActionItem | RefActionItem;

// ─── Script library (reusable action templates)─────────────────────────────

/**
 * A script library item (a reusable action template).
 *
 * Aligned with the backend `orbit_config::ActionTemplate`; `workspaceId` is stamped on save
 * (isomorphic with `Environment`). Only script / database actions are allowed — no built-in interpolation node
 * and no reference to another library item.
 *
 * A library item **does not declare when it runs**: once referenced into an action list the consumer orders it,
 * and the action's **actual position** in the list is the final authority on timing (consistent with the built-in interpolation node model).
 */
export interface ActionTemplate {
  id: string;
  name: string;
  description?: string;
  /** The concrete action (script or database query) */
  action: ScriptActionItem | DbActionItem;
  /** Sort order (list order in the management UI) */
  sortIndex?: number;
  /** Owning workspace (v2 snapshot field; stamped on save) */
  workspaceId?: string;
}

// ─── Data models (structured as JSON Schema)───

export type SchemaFieldType =
  "string" | "integer" | "number" | "boolean" | "object" | "array" | "null";

export interface SchemaField {
  id: string;
  name: string;
  type: SchemaFieldType;
  format?: string;
  required?: boolean;
  example?: string;
  description?: string;
  enumValues?: string[];
  refModelId?: string;
  children?: SchemaField[];
}

export interface DataModel {
  id: string;
  name: string;
  /** Owning workspace (v2 snapshot field; in the store it is the active workspace, stamped on save) */
  workspaceId?: string;
  description?: string;
  fields: SchemaField[];
}

// ─── Collections / request tree ──────────────────────────────────

export interface Collection {
  id: string;
  name: string;
  /** Owning workspace (v2 snapshot field; in the store it is the active workspace, stamped on save) */
  workspaceId?: string;
  /** Collection protocol kind; unset / "http" = an HTTP collection (legacy compat). Connection-based collections (websocket/grpc/tcp/.../plugin protocols) carry `connection` or gRPC metadata. */
  kind?: string;
  /** gRPC collection metadata (used only when kind=grpc) */
  grpc?: GrpcCollectionMeta;
  /** Connection config of a connection-based collection (single source of truth at connection level; request-level fields only override it) */
  connection?: ConnectionConfig;
  items: CollectionItem[];
}

// ─── gRPC collection metadata and hierarchy nodes ──────────────────────────────

/** gRPC interface source: proto file import or server reflection import */
export type GrpcSource =
  | { type: "proto"; files: { name: string; content: string }[] }
  | { type: "reflection"; target: string };

export interface GrpcCollectionMeta {
  source: GrpcSource;
  packages: GrpcPackageNode[];
  /** Encoded FileDescriptorProto bytes (base64), used to ask the backend for message templates */
  descriptorFiles?: string[];
}

export interface GrpcPackageNode {
  name: string;
  /** Raw proto content for this package (the proto import source) */
  proto?: string;
  /** Package-level auth (inherited by rpcs in the package; editable in the editor area) */
  auth?: AuthConfig;
  prereqScript?: string;
  postreqScript?: string;
  services: GrpcServiceNode[];
}

export interface GrpcServiceNode {
  name: string;
  methods: GrpcRpcNode[];
}

export interface GrpcRpcNode {
  name: string;
  inputType: string;
  outputType: string;
  clientStreaming: boolean;
  serverStreaming: boolean;
}

/** The three gRPC node levels inside a collection: package → service → rpc */
export type GrpcTreeItem =
  | {
      type: "grpc-package";
      id: string;
      /** package name */
      name: string;
      proto?: string;
      services: GrpcServiceNode[];
    }
  | {
      type: "grpc-service";
      id: string;
      /** package name */
      packageName: string;
      /** service name */
      name: string;
      methods: GrpcRpcNode[];
    }
  | { type: "grpc-rpc"; id: string; requestId: string };

/** Whether the collection is connection-based (non-HTTP) */
export function isConnectionCollection(
  c: Pick<Collection, "kind"> | undefined | null,
): boolean {
  return !!c && c.kind !== undefined && c.kind !== "http";
}

/** Build the default connection config for a protocol (the initial value when creating a connection-based collection) */
export function createDefaultConnection(protocol: string): ConnectionConfig {
  switch (protocol) {
    case "websocket":
      return { url: "ws://localhost:8080", messageType: "text", closeAfter: 1 };
    case "grpc":
      return { url: "grpc://localhost:9090" };
    case "tcp":
      return {
        url: "tcp://localhost:9000",
        framing: { mode: "read_until_close" },
      };
    case "udp":
      return { url: "udp://localhost:9000" };
    case "sse":
      return { url: "http://localhost:8080/events" };
    case "graphql":
      return { url: "http://localhost:8080/graphql" };
    default:
      return { url: "" };
  }
}

export type CollectionItem =
  | { type: "folder"; id: string; name: string; items: CollectionItem[] }
  | { type: "request"; id: string; requestId: string }
  | GrpcTreeItem;

export interface Environment {
  id: string;
  name: string;
  /** Owning workspace (v2 snapshot field; in the store it is the active workspace, stamped on save) */
  workspaceId?: string;
  variables: Record<string, string>;
  secrets: Record<string, string>;
}

export interface HistoryEntry {
  id: string;
  requestId: string;
  /** Owning workspace (v2 snapshot field; stamped on save, passed directly by module-level commands) */
  workspaceId?: string;
  name: string;
  method: string;
  url: string;
  status: number | null;
  duration: number | null;
  size: number | null;
  timestamp: number;
  responseBody?: string;
  timing?: {
    dns: number;
    connect: number;
    tls: number;
    ttfb: number;
    download: number;
  };
}

export interface Tab {
  id: string;
  /** Bound request tab; empty string for grpc node tabs (package/service) */
  requestId: string;
  /** grpc node tab (package / service have no request object and are shown in a standalone tab) */
  grpcNode?: {
    type: "grpc-package" | "grpc-service";
    collectionId: string;
    nodeId: string;
  };
  dirty: boolean;
}

// ─── Automation scenarios ─────────────────────────────────────

export type StepType =
  "request" | "loop" | "condition" | "wait" | "group" | "setvar";

export interface ScenarioStep {
  id: string;
  type: StepType;
  disabled?: boolean;
  name: string;
  requestId?: string;
  // loop
  count?: number;
  // condition
  expr?: string;
  // wait
  ms?: number;
  // setvar
  varKey?: string;
  varValue?: string;
  // Extraction (writes variables from the response for later steps; request steps only)
  // extractPath means different things per extractType: jsonpath → JSONPath, jmespath → expression,
  // header/cookie → name, regex → expression (always group 1).
  extractPath?: string;
  extractVar?: string;
  /** Extraction source type (defaults to jsonpath) */
  extractType?: "jsonpath" | "jmespath" | "header" | "regex" | "cookie";
  children?: ScenarioStep[];
  // condition else branch
  elseChildren?: ScenarioStep[];
}

export interface Scenario {
  id: string;
  name: string;
  /** Owning workspace (v2 snapshot field; in the store it is the active workspace, stamped on save) */
  workspaceId?: string;
  description?: string;
  steps: ScenarioStep[];
  /** Owning folder (null / unset = root) */
  folderId?: string | null;
  /** Priority (defaults to p2); batch folder/suite runs are ordered P0→P3 */
  priority?: ScenarioPriority;
  /** Run environment id (unset = unspecified, so no environment variables are injected; independent of the global activeEnvId) */
  envId?: string | null;
  /** Bound CSV test data set */
  dataSetId?: string | null;
  /** Whether test data is enabled (when off, only one round runs and no row variables are injected even if a data set is bound) */
  useDataSet?: boolean;
  /** Loop count (how many times the whole scenario runs; defaults to 1; combined with CSV row iteration = rows × loops) */
  iterations?: number;
  /** Failure policy (defaults to stop) */
  onError?: FailurePolicy;
  /** Whether request details are recorded (a run config switch; when on, detail steps can be expanded to inspect request/response; defaults to false) */
  recordRequestDetails?: boolean;
}

export interface StepRunResult {
  stepId: string;
  status: "pass" | "fail" | "skip";
  message: string;
  durationMs?: number;
}

// ─── Automation testing: folders / data sets / suites / run reports ────────────────────────

/** Scenario priority: P0 is highest, P3 lowest */
export type ScenarioPriority = "p0" | "p1" | "p2" | "p3";

/** Failure policy:
 *  - stop: end the run (engine on_error: stop, and no further rounds)
 *  - continue: ignore the error and keep running the remaining steps (engine on_error: continue)
 *  - next-loop: jump to the next loop (after this round ends, continue with the next; implemented by frontend orchestration) */
export type FailurePolicy = "stop" | "continue" | "next-loop";

/** Suite run mode */
export type RunMode = "serial" | "parallel";

/** Scenario folder (may be nested) */
export interface ScenarioFolder {
  id: string;
  name: string;
  /** Parent folder (null = root level) */
  parentId: string | null;
  workspaceId?: string;
  collapsed?: boolean;
}

/** Test data read mode */
export type DataSetMode = "sequential" | "random" | "shuffle";

/** CSV test data set: columns = variables, rows = one iteration each */
export interface ScenarioDataSet {
  id: string;
  name: string;
  workspaceId?: string;
  /** Raw CSV text; the first row is the header */
  csv: string;
  columns: string[];
  rowCount: number;
  mode: DataSetMode;
  updatedAt: number;
}

/** Test suite: a static group of scenarios (may span folders; a scenario can belong to several suites) */
export interface TestSuite {
  id: string;
  name: string;
  workspaceId?: string;
  description?: string;
  /** Suite run environment (overrides the environment of the member scenarios) */
  envId?: string | null;
  runMode: RunMode;
  /** Parallel concurrency (effective when runMode=parallel, 1-10, defaults to 3) */
  concurrency?: number;
  /** Member scenario ids (executed in array order; in serial mode they may be re-sorted by priority) */
  memberIds: string[];
  updatedAt: number;
}

/** Detailed run step (adds the ownership info needed for display on top of StepRunResult) */
export interface ScenarioRunStep {
  /** Stable key (stepId may repeat when one scenario runs several rounds) */
  seq: number;
  caseId: string;
  stepId: string;
  name: string;
  status: "pass" | "fail" | "skip";
  message: string;
  durationMs: number;
  /** Data set row number (undefined when test data is not enabled) */
  rowIndex?: number;
  /** Loop round (0-based) */
  iteration?: number;
  /** Request/response detail snapshot (present only when the scenario enables "record request details"; used by the expanded detail view) */
  request?: RecordedRequest;
}

/** Recorded detail of a single request (captured by the engine; camelCase fields aligned with the backend) */
export interface RecordedRequest {
  method: string;
  target: string;
  operation: string;
  requestHeaders: [string, string][];
  requestBody: string;
  requestTruncated: boolean;
  status: number;
  responseHeaders: [string, string][];
  responseBody: string;
  responseSize: number;
  responseTruncated: boolean;
  error: string | null;
}

/** Result of a single scenario in one run */
export interface ScenarioRunCaseResult {
  scenarioId: string;
  scenarioName: string;
  priority: ScenarioPriority;
  /** Name of the environment actually used (null when unspecified) */
  envName: string | null;
  dataSetName: string | null;
  iterations: number;
  /** Number of data set rows (0 = not data-driven) */
  rows: number;
  pass: number;
  fail: number;
  skip: number;
  durationMs: number;
  /** Row numbers that failed */
  failedRows: number[];
  /** Total request duration (ms; the sum over detail steps) */
  requestMs?: number;
  /** Request count */
  requestCount?: number;
  /** Total assertion count */
  assertCount?: number;
  error?: string;
}

/** One run = one persisted report */
export interface ScenarioRunRecord {
  id: string;
  workspaceId?: string;
  targetType: "case" | "folder" | "suite";
  targetId: string;
  targetName: string;
  runMode: RunMode;
  startedAt: number;
  durationMs: number;
  envName: string | null;
  status: "pass" | "fail" | "error";
  totalPass: number;
  totalFail: number;
  totalSkip: number;
  cases: ScenarioRunCaseResult[];
  /** Detail steps (only for the serial / single-scenario live stream; parallel mode stores summaries only) */
  steps: ScenarioRunStep[];
  aborted?: boolean;
  /** Loop count (the sum over all scenarios) */
  iterations?: number;
  /** Total request duration (ms) */
  totalRequestMs?: number;
  /** Request count */
  totalRequestCount?: number;
  /** Total assertion count */
  totalAssertions?: number;
}

/** Report list entry (a summary, so the full detail is not loaded) */
export interface ScenarioReportSummary {
  id: string;
  workspaceId?: string;
  targetType: "case" | "folder" | "suite";
  targetId: string;
  targetName: string;
  runMode: RunMode;
  startedAt: number;
  durationMs: number;
  envName: string | null;
  status: "pass" | "fail" | "error";
  totalPass: number;
  totalFail: number;
  totalSkip: number;
  caseCount: number;
}

// ─── Load testing ───────────────────────────────────────────

export type LoadExecutor =
  "constant-vus" | "ramping-vus" | "constant-arrival-rate";

/** A single ramp-up stage mode */
export type LoadRampMode = "gradual" | "instant" | "jmeter";

/** Ramp-up stage (ramping-vus) */
export interface LoadStage {
  id: string;
  /** Target VU count of the stage */
  target: number;
  /** Stage duration (e.g. "30s" / "1m") */
  duration: string;
  /** gradual = ramp up over the duration; instant = jump to the target immediately and hold */
  ramp: LoadRampMode;
  /** Ramp duration for ramp=jmeter (<= duration; defaults to duration) */
  rampUp?: string;
}

export interface LoadTestConfig {
  executor: LoadExecutor;
  vus: number;
  duration: string;
  rampUp: string;
  rate: number;
  /** ramping-vus: initial VU count */
  startVus: number;
  /** ramping-vus: maximum VU cap (0 = unlimited) */
  maxVus: number;
  /** ramping-vus: ramp stages */
  stages: LoadStage[];
  /** constant-arrival-rate: pre-allocated VU count */
  preAllocatedVus: number;
  requestId?: string | null;
  ignoreBody: boolean;
  /** Threshold rule list (configured in the UI, converted into YAML thresholds for the engine) */
  thresholds: ThresholdRule[];
}

/** A single threshold rule configured in the UI */
export interface ThresholdRule {
  id: string;
  metric: "p50" | "p90" | "p95" | "p99" | "avg" | "errorRate";
  condition: "<" | ">" | "<=" | ">=";
  value: number;
  abortOnFail?: boolean;
}

export interface LoadTestMetrics {
  time: number;
  vus: number;
  rps: number;
  p95: number;
  p99: number;
  errorRate: number;
  /** Per-stage average duration */
  timing?: TimingBreakdown;
  /** Errors grouped by kind (shown on hover in the load-test report) */
  error_breakdown?: ErrorGroup[];
}

export interface TimingBreakdown {
  dns_ms: number;
  tcp_ms: number;
  tls_ms: number;
  send_ms: number;
  ttfb_ms: number;
  download_ms: number;
  total_ms: number;
}

/** Error kind group: `type` is the backend classification key, `sample` is a representative error message */
export interface ErrorGroup {
  type: string;
  count: number;
  sample: string;
}

export interface LoadTestSummary {
  total_requests: number;
  total_failures: number;
  total_duration_ms: number;
  rps: number;
  p50_ms: number;
  p90_ms: number;
  p95_ms: number;
  p99_ms: number;
  p999_ms: number;
  min_ms: number;
  max_ms: number;
  mean_ms: number;
  error_rate: number;
  total_bytes: number;
  timing?: TimingBreakdown;
  error_breakdown?: ErrorGroup[];
}

/** Result of a single threshold (from the engine thresholds) */
export interface ThresholdGate {
  label: string;
  actual: number;
  target: number;
  passed: boolean;
  abort_on_fail: boolean;
}

/** The complete result after a load test finishes (thresholds included) */
export interface LoadTestCompleteResult {
  status: string;
  summary: LoadTestSummary;
  thresholds: ThresholdGate[];
  all_thresholds_passed: boolean;
}

// ─── Performance reports (persisted)────────────────────────────

export interface SavedReport {
  id: string;
  name: string;
  /** Owning workspace (reports are isolated per workspace; legacy reports default to the default one) */
  workspaceId?: string;
  endpoint: string;
  method: string;
  created_at: string;
  vus: number;
  duration: string;
  /** Human-readable ramp-up description (e.g. ramping-vus stages); falls back to vus/duration when empty */
  config?: string | null;
  summary: LoadTestSummary;
  thresholds: ThresholdGate[];
  all_thresholds_passed: boolean;
  is_baseline: boolean;
  baseline_name: string | null;
}

export interface PercentilePoint {
  p: number; // 50/75/90/95/99
  value: number;
  baseline?: number;
  sla?: number;
}

export interface SlaGate {
  label: string;
  target: number;
  actual: number;
  passed: boolean;
}

// ─── Plugins (WASM, aligned with orbit-plugin::PluginDescriptor)──────

/** Loaded plugin descriptor (corresponds to the Rust PluginDescriptor, snake_case fields) */
export interface PluginDescriptor {
  id: string;
  name: string;
  version: string;
  description: string;
  /** "protocol" | "codec" */
  kind: string;
  /** "enabled" | "error" */
  status: string;
  error?: string | null;
  /** Registered protocol ids (kind=protocol) */
  protocols: string[];
  /** Registered codec names (kind=codec) */
  codecs: string[];
  /** Connection params JSON Schema (protocol plugins; corresponds to manifest.connectionConfigSchema) */
  connectionConfigSchema?: Record<string, unknown> | null;
  /** Message params JSON Schema (protocol plugins; corresponds to manifest.requestConfigSchema) */
  requestConfigSchema?: Record<string, unknown> | null;
  /** Plugin install directory (`<plugins_root>/<id>` after a zip install; None = temporarily loaded) */
  installedDir?: string | null;
}

/** Protocol catalog entry (`GET /api/protocols`): built-in + plugin protocols, with dynamic form schema */
export interface ProtocolCatalogEntry {
  id: string;
  builtin: boolean;
  connectionConfigSchema?: Record<string, unknown> | null;
  requestConfigSchema?: Record<string, unknown> | null;
}

/** Codec catalog entry (`GET /api/codecs`): built-in + plugin codecs */
export interface CodecCatalogEntry {
  name: string;
  builtin: boolean;
}

/** Plugin directory scan report */
export interface PluginScanReport {
  scanned: string[];
  loaded: string[];
  /** [id, error][] */
  failed: [string, string][];
}

/** Plugin load result */
export interface PluginLoadResult {
  kind: string;
  capabilities: string[];
}

// ─── Analytics / feedback (measure everything)─────────────────────────

export type AnalyticsEvent =
  | "module_view"
  | "request_send"
  | "load_start"
  | "load_stop"
  | "scenario_run"
  | "plugin_install"
  | "import"
  | "feedback";

export interface FeedbackItem {
  id: string;
  rating: number;
  text: string;
  module: ModuleKey;
  createdAt: number;
}

// ─── Mock service (interfaces + expectation model)────────────────────

/** Param location: where in the request the value is taken from for comparison */
export type MockParamLocation = "query" | "path" | "header" | "body" | "cookie";

/** Comparison operator */
export type MockCompareOp =
  | "equals"
  | "not_equals"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "contains"
  | "not_contains"
  | "exists"
  | "not_exists"
  | "regex";

/** A single param condition */
export interface MockCondition {
  location: MockParamLocation;
  name: string;
  op: MockCompareOp;
  value: string;
}

/** IP condition (restricts when an expectation applies based on the source IP); off by default */
export interface MockIpCondition {
  enabled: boolean;
  ip: string;
}

/** A single expectation of an interface */
export interface MockExpectation {
  id: string;
  name: string;
  enabled: boolean;
  conditions: MockCondition[];
  ipCondition: MockIpCondition;
  status: number;
  headers: Record<string, string>;
  body: string;
  delayMs: number;
}

/** An interface (match key = method + path, data ownership key = requestId) and its expectation list */
export interface MockInterface {
  /** Owning request id: mock data stays attached to this request even after the url changes */
  requestId?: string;
  /** Owning workspace (v2 snapshot field; mocks are isolated per workspace) */
  workspaceId?: string;
  method: string;
  path: string;
  enabled: boolean;
  expectations: MockExpectation[];
}
