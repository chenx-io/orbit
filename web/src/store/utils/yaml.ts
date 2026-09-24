// orbit-config YAML builder.
// Renders the in-app data models (LoadTestConfig / Scenario / ThresholdRule) into
// a YAML config string the Rust engine can execute. Pure functions, no store dependency.
import type {
  ActionTemplate,
  ApiRequest,
  Collection,
  ConnectionConfig,
  FailurePolicy,
  GrpcRequest,
  GraphqlRequest,
  HttpRequest,
  LoadTestConfig,
  RequestAction,
  Scenario,
  ScenarioStep,
  SseRequest,
  TcpRequest,
  ThresholdRule,
  UdpRequest,
  WsMessageSpec,
  WsRequest,
} from "@/data/types";
import { isConnectionCollection, isHttpRequest } from "@/data/types";
import { getActiveBody, stripBodyComments } from "@/lib/requestBody";
import { buildRequestUrlRaw } from "@/lib/resolve";
import {
  getPostActions,
  getPreActions,
  isActionEnabled,
  resolveActionRefs,
} from "@/lib/requestActions";

/** YAML double-quoted string escaping (backslash / quote / newline) */
function yqStr(s: string): string {
  return `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
}

/**
 * Pre/post action lists → YAML fragments (`pre_actions` / `post_actions`, siblings of `request:`).
 *
 * Only enabled items are emitted (the built-in interpolation node is always enabled); returns an empty string when there is nothing.
 *
 * `pre_actions` is a **single ordered list**: before the built-in `type: interpolate` node = pre-interpolation,
 * after it = post-interpolation (= the former "pre-actions"); legacy single-script fields are normalized into actions by `getPreActions`.
 * When the list has no anchor the engine inserts one **at the front**, so emitting it as-is is fine.
 *
 * **Self-contained export**: when `library` is passed, script library references are expanded into the entry's current content first —
 * so the exported file does not depend on the library still existing (in-app snapshots keep the reference; editing the library applies everywhere).
 */
function actionsYamlBlock(
  actions: RequestAction[],
  indent: string,
  key: "pre_actions" | "post_actions",
  library?: ActionTemplate[],
): string {
  // Expansion must happen before the enabled filter: a disabled library entry shows up as the expanded action
  const resolved = library?.length
    ? resolveActionRefs(actions, library)
    : actions;
  const list = resolved.filter(isActionEnabled);
  // The key is omitted when there are no enabled actions or only the built-in node (the engine fills in the default anchor position, avoiding an extra line per step)
  if (list.length === 0 || list.every((a) => a.kind === "interpolate"))
    return "";
  const i = `${indent}  `;
  const lines: string[] = [];
  for (const a of list) {
    if (a.kind === "interpolate") {
      // Built-in interpolation node: turns the request template into the final message (variable interpolation + body assembly/encoding)
      lines.push(`${i}- type: interpolate`);
      continue;
    }
    if (a.kind === "ref") {
      // Dangling reference (library entry deleted): keep the reference form; the engine logs an error without aborting the request
      const alias = a.name?.trim() ? `\n${i}  name: ${yqStr(a.name)}` : "";
      lines.push(
        `${i}- type: ref\n${i}  library_id: ${yqStr(a.libraryId)}${alias}`,
      );
      continue;
    }
    const nameLine = a.name?.trim() ? `${i}  name: ${yqStr(a.name)}\n` : "";
    if (a.kind === "script") {
      lines.push(`${i}- type: script\n${nameLine}${i}  code: ${yqStr(a.code)}`);
      continue;
    }
    let item = `${i}- type: db\n${nameLine}`;
    item += `${i}  datasource: ${yqStr(a.datasource)}\n`;
    if (a.command?.trim()) item += `${i}  command: ${yqStr(a.command)}\n`;
    if (a.args?.length)
      item += `${i}  args: [${a.args.map(yqStr).join(", ")}]\n`;
    if (a.sql?.trim()) item += `${i}  sql: ${yqStr(a.sql)}\n`;
    if (a.target) {
      item += `${i}  target:\n${i}    type: ${a.target.type}\n`;
      if (
        a.target.row !== undefined &&
        a.target.type !== "row_count" &&
        a.target.type !== "scalar"
      ) {
        item += `${i}    row: ${a.target.row}\n`;
      }
      if (a.target.column)
        item += `${i}    column: ${yqStr(a.target.column)}\n`;
      if (a.target.path) item += `${i}    path: ${yqStr(a.target.path)}\n`;
    }
    if (a.extractVar?.trim())
      item += `${i}  extract_var: ${yqStr(a.extractVar.trim())}\n`;
    const cols = (a.columns ?? []).filter(
      (c) => c.column?.trim() && c.var?.trim(),
    );
    if (cols.length > 0) {
      item += `${i}  columns:\n`;
      for (const c of cols) {
        item += `${i}    - column: ${yqStr(c.column)}\n${i}      var: ${yqStr(c.var)}\n`;
      }
    }
    if (a.row) item += `${i}  row: ${a.row}\n`;
    if (a.retry) {
      item += `${i}  retry:\n`;
      if (a.retry.interval_ms !== undefined)
        item += `${i}    interval_ms: ${a.retry.interval_ms}\n`;
      if (a.retry.max_attempts !== undefined)
        item += `${i}    max_attempts: ${a.retry.max_attempts}\n`;
      if (a.retry.timeout_ms)
        item += `${i}    timeout_ms: ${a.retry.timeout_ms}\n`;
    }
    lines.push(item.replace(/\n$/, ""));
  }
  return `\n${indent}${key}:\n${lines.join("\n")}`;
}

/** Find the connection config of the connection-based collection containing the given request (single source of truth at connection level; undefined when not found) */ export function findConnectionForRequest(
  requestId: string,
  collections?: Collection[],
): ConnectionConfig | undefined {
  if (!collections) return undefined;
  const walk = (items: Collection["items"]): boolean => {
    for (const it of items) {
      if (it.type === "request" && it.requestId === requestId) return true;
      if (it.type === "folder" && walk(it.items)) return true;
    }
    return false;
  };
  for (const c of collections) {
    if (!isConnectionCollection(c)) continue;
    if (walk(c.items)) return c.connection;
  }
  return undefined;
}

/** Parse duration strings like "5s"/"2m"/"1h" into milliseconds; falls back to 30s when unparsable. */
export function parseDuration(d: string): number {
  const m = d.match(/^(\d+)\s*(s|m|h)?$/i);
  if (!m) return 30000;
  const n = parseInt(m[1], 10);
  const unit = (m[2] || "s").toLowerCase();
  return n * (unit === "h" ? 3600000 : unit === "m" ? 60000 : 1000);
}

/** ThresholdRule → threshold expression string (a format the engine can parse) */
export function thresholdToExpr(t: ThresholdRule): string {
  const suffix = t.abortOnFail ? "; abort" : "";
  if (t.metric === "errorRate") {
    const val = String(t.value / 100);
    return `http_req_failed: rate ${t.condition} ${val}${suffix}`;
  }
  return `http_req_duration: ${t.metric === "avg" ? "avg" : `p(${t.metric.slice(1)})`} ${t.condition} ${t.value}${suffix}`;
}

/** Build an orbit-config YAML string from loadTestConfig */
export function buildLoadYaml(
  cfg: LoadTestConfig,
  req: HttpRequest | undefined,
  resolvedUrl: string,
  resolvedHeaders: Record<string, string>,
  resolvedBody: string,
  /** Script library table: used to expand references into concrete actions (self-contained export) */
  library?: ActionTemplate[],
): string {
  const headersYaml = Object.entries(resolvedHeaders)
    .map(([k, v]) => `            ${k}: "${v.replace(/"/g, '\\"')}"`)
    .join("\n");
  const formatYaml =
    (req?.requestFormat
      ? `\n          request_format: "${req.requestFormat}"`
      : "") +
    (req?.responseFormat
      ? `\n          response_format: "${req.responseFormat}"`
      : "");
  // Omit the headers: line when there are no request headers — a null-valued key followed by a sibling key (e.g. body) breaks YAML parsing
  const headersBlock = headersYaml
    ? `\n          headers:\n${headersYaml}`
    : "";
  // Bodies are always emitted as YAML double-quoted strings (backslashes/quotes/newlines escaped),
  // so raw text is not parsed as mappings/flow collections and byte-level passthrough is guaranteed.
  const bodyYaml = resolvedBody
    ? `\n          body: "${resolvedBody.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`
    : "";
  // Pre/post actions (scripts / database queries / built-in interpolation node; legacy single-script fields normalized by getPreActions)
  const scriptYaml =
    actionsYamlBlock(
      req ? getPreActions(req) : [],
      "        ",
      "pre_actions",
      library,
    ) +
    actionsYamlBlock(
      req ? getPostActions(req) : [],
      "        ",
      "post_actions",
      library,
    );

  // Threshold rules → YAML thresholds list (a top-level key, sibling of name/scenarios)
  const thresholdsYaml =
    (cfg.thresholds ?? []).length > 0
      ? `thresholds:\n${cfg.thresholds
          .map((t) => {
            const expr = thresholdToExpr(t);
            return `  - "${expr}"`;
          })
          .join("\n")}\n`
      : "";

  const executorYaml = (() => {
    switch (cfg.executor) {
      case "constant-vus":
        return `      type: constant-vus
      vus: ${cfg.vus}
      duration: "${cfg.duration}"
      ramp_up: "${cfg.rampUp}"`;
      case "ramping-vus": {
        const maxLine =
          (cfg.maxVus ?? 0) > 0 ? `\n      max_vus: ${cfg.maxVus}` : "";
        return `      type: ramping-vus
      start_vus: ${cfg.startVus || 0}${maxLine}
      stages:
${(cfg.stages ?? [])
  .map(
    (s) => `        - target: ${s.target}
          duration: "${s.duration}"
          ramp: ${s.ramp ?? "gradual"}${s.ramp === "jmeter" && s.rampUp ? `\n          ramp_up: "${s.rampUp}"` : ""}`,
  )
  .join("\n")}`;
      }
      case "constant-arrival-rate":
        return `      type: constant-arrival-rate
      rate: ${cfg.rate}
      duration: "${cfg.duration}"
      ramp_up: "${cfg.rampUp}"
      pre_allocated_vus: ${cfg.preAllocatedVus || cfg.vus}`;
    }
  })();

  return `name: "Load Test"
${thresholdsYaml}scenarios:
  - name: "${req?.name ?? cfg.requestId}"
    executor:
${executorYaml}
    steps:
      - type: request
        request:
          method: ${req?.method ?? "GET"}
          url: "${resolvedUrl.replace(/"/g, '\\"')}"${formatYaml}${headersBlock}${bodyYaml}${scriptYaml}`;
}

/** Recursively convert scenario steps into YAML fragments tagged with a type */
export function stepToYaml(
  st: ScenarioStep,
  requests: Record<string, ApiRequest>,
  envVars: Record<string, string>,
  indent: number,
  collections?: Collection[],
  /** Script library table: references in the scenario YAML are expanded into concrete actions (self-contained export) */
  library?: ActionTemplate[],
): string {
  const pad = "  ".repeat(indent);
  const inner = "  ".repeat(indent + 1);
  const inner2 = "  ".repeat(indent + 2);
  const yq = (s: string) =>
    `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
  // Variables are not substituted here: {{var}} / {{$...}} placeholders go into the YAML verbatim,
  // and the backend engine interpolates them per request (plan.variables injection + pipeline.interp_value normalization).
  /** Response variable extraction block (emitted only when both extractVar and extractPath are configured) */
  const extractYaml = (st: ScenarioStep, ind: string): string => {
    if (!st.extractVar || !st.extractPath) return "";
    const src = st.extractType ?? "jsonpath";
    const srcYaml = (() => {
      switch (src) {
        case "jmespath":
          return `expression: ${yq(st.extractPath)}`;
        case "header":
        case "cookie":
          return `name: ${yq(st.extractPath)}`;
        case "regex":
          return `pattern: ${yq(st.extractPath)}\n${ind}    group: 1`;
        default:
          return `path: ${yq(st.extractPath)}`;
      }
    })();
    return `\n${ind}extract:\n${ind}  - name: "${st.extractVar.replace(/"/g, '\\"')}"\n${ind}    from: ${src}\n${ind}    ${srcYaml}`;
  };

  if (st.type === "request") {
    const req = requests[st.requestId ?? ""];
    if (!req) {
      console.warn(
        `[stepToYaml] skipping step with no bound request: "${st.name}" (requestId: ${st.requestId})`,
      );
      return "";
    }
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";

    // Non-HTTP protocol: emit a RequestSpec block; connection-based collections inherit url/framing from connection
    if (!isHttpRequest(req)) {
      const conn = findConnectionForRequest(req.id, collections);
      return (
        `${pad}- type: request\n` +
        `${inner}name: ${yq(st.name)}\n` +
        `${inner}protocol: ${req.protocol}\n` +
        `${inner}request:\n` +
        protocolRequestYaml(req, envVars, inner2, conn) +
        actionsYamlBlock(getPreActions(req), inner, "pre_actions", library) +
        actionsYamlBlock(getPostActions(req), inner, "post_actions", library) +
        extractYaml(st, inner) +
        disabledLine +
        "\n"
      );
    }

    const headersYaml =
      req.headers.filter((h) => h.enabled && h.key).length > 0
        ? req.headers
            .filter((h) => h.enabled && h.key)
            .map(
              (h) => `${inner2}  ${h.key}: "${h.value.replace(/"/g, '\\"')}"`,
            )
            .join("\n")
        : "";
    // Strip comments (JSON // and /* */, XML <!-- -->) before exporting scenario YAML, keeping only valid data;
    // variable placeholders are kept verbatim and interpolated per request by the engine
    const bodyYaml = getActiveBody(req)
      ? `\n${inner}  body: "${stripBodyComments(getActiveBody(req), req.bodyMode).replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`
      : "";
    const formatYaml =
      (req.requestFormat
        ? `\n${inner}  request_format: "${req.requestFormat}"`
        : "") +
      (req.responseFormat
        ? `\n${inner}  response_format: "${req.responseFormat}"`
        : "");
    // Pre/post actions (scripts / database queries; legacy single-script fields are normalized and emitted as actions too)
    const scriptYaml =
      actionsYamlBlock(getPreActions(req), inner, "pre_actions", library) +
      actionsYamlBlock(getPostActions(req), inner, "post_actions", library);
    const method = req.method || "GET";
    // The URL is only assembled structurally (Path/Query params, lib/resolve.buildRequestUrlRaw),
    // without resolving variables: {{var}} / {{$...}} placeholders go into the YAML verbatim,
    // and the backend engine interpolates them on every request (consistent with dynamic values being "regenerated per request").
    const url = buildRequestUrlRaw(req);

    return (
      `${pad}- type: request\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}request:\n` +
      `${inner}  method: ${method}\n` +
      `${inner}  url: ${yq(url)}${formatYaml}` +
      (headersYaml ? `\n${inner}  headers:\n${headersYaml}` : "") +
      bodyYaml +
      scriptYaml +
      extractYaml(st, inner) +
      disabledLine +
      "\n"
    );
  }

  if (st.type === "loop") {
    const count = st.count ?? 1;
    const childrenYaml = (st.children ?? [])
      .map((c) =>
        stepToYaml(c, requests, envVars, indent + 2, collections, library),
      )
      .join("");
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";
    return (
      `${pad}- type: loop\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}count: ${count}\n` +
      `${inner}steps:\n${childrenYaml}` +
      disabledLine +
      "\n"
    );
  }

  if (st.type === "wait") {
    const ms = st.ms ?? 1000;
    const secs = (ms / 1000).toFixed(ms % 1000 === 0 ? 0 : 1);
    const dur = ms >= 1000 ? `${secs}s` : `${ms}ms`;
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";
    return (
      `${pad}- type: wait\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}duration: "${dur}"` +
      disabledLine +
      "\n"
    );
  }

  if (st.type === "setvar") {
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";
    return (
      `${pad}- type: setvar\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}key: "${st.varKey ?? "var"}"\n` +
      `${inner}value: "${(st.varValue ?? "").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"` +
      disabledLine +
      "\n"
    );
  }

  if (st.type === "condition") {
    const thenYaml = (st.children ?? [])
      .map((c) =>
        stepToYaml(c, requests, envVars, indent + 2, collections, library),
      )
      .join("");
    const elseYaml =
      (st.elseChildren ?? []).length > 0
        ? `\n${inner}else:\n${st.elseChildren!.map((c) => stepToYaml(c, requests, envVars, indent + 2, collections, library)).join("")}`
        : "";
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";
    return (
      `${pad}- type: condition\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}expression: "${(st.expr ?? "true").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"\n` +
      `${inner}then:\n${thenYaml}${elseYaml}` +
      disabledLine +
      "\n"
    );
  }

  if (st.type === "group") {
    const childrenYaml = (st.children ?? [])
      .map((c) =>
        stepToYaml(c, requests, envVars, indent + 2, collections, library),
      )
      .join("");
    const disabledLine = st.disabled ? `\n${inner}  disabled: true` : "";
    return (
      `${pad}- type: group\n` +
      `${inner}name: ${yq(st.name)}\n` +
      `${inner}steps:\n${childrenYaml}` +
      disabledLine +
      "\n"
    );
  }

  return "";
}

/** The request block for non-HTTP protocols (corresponds to the backend RequestSpec variants).
 * `ind` is the indent of the request fields (callers pass inner2, 12 spaces).
 * `conn` is the connection config of the connection-based collection (single source of truth at connection level); message nodes inherit
 * connection fields such as url/framing/close_after from it, with request-level fields only overriding them (legacy compat). */
function protocolRequestYaml(
  req: Exclude<ApiRequest, HttpRequest>,
  envVars: Record<string, string>,
  ind: string,
  conn?: ConnectionConfig,
): string {
  const resolveTmpl = (s: string) =>
    s.replace(/\{\{(\w+)\}\}/g, (_, k: string) => envVars[k] ?? `{{${k}}}`);
  const yq = (s: string) =>
    `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
  const i2 = ind + "  "; // message items / framing fields
  const i3 = i2 + "  "; // message fields
  const i4 = i3 + "  "; // script content
  const script = (s?: string) => {
    if (!s || !s.trim()) return "";
    return ` |\n${s
      .replace(/\t/g, "  ")
      .split("\n")
      .map((l) => `${i4}${l}`)
      .join("\n")}`;
  };
  const messageYaml = (m: WsMessageSpec) => {
    const lines: string[] = [`${i2}- payload: ${yq(resolveTmpl(m.payload))}`];
    if (m.payloadType && m.payloadType !== "text")
      lines.push(`${i3}  payload_type: ${m.payloadType}`);
    if (m.messageType && m.messageType !== "text")
      lines.push(`${i3}  message_type: ${m.messageType}`);
    if (m.preScript) lines.push(`${i3}  pre_script:${script(m.preScript)}`);
    if (m.postScript) lines.push(`${i3}  post_script:${script(m.postScript)}`);
    if (m.waitMs) lines.push(`${i3}  wait_ms: ${m.waitMs}`);
    return lines.join("\n");
  };
  const messagesYaml = (msgs: WsMessageSpec[]) =>
    msgs.length ? `\n${ind}messages:\n${msgs.map(messageYaml).join("\n")}` : "";
  /** TCP framing: the collection connection takes precedence, request level overrides */
  const framingYaml = (t: TcpRequest) => {
    const f = t.framing ?? conn?.framing;
    if (!f || f.mode === "read_until_close") return "";
    return (
      `\n${ind}framing:\n${i2}  mode: ${f.mode}` +
      (f.delimiter ? `\n${i2}  delimiter: ${yq(f.delimiter)}` : "") +
      (f.fixedLen ? `\n${i2}  fixed_len: ${f.fixedLen}` : "") +
      (f.bigEndian === false ? `\n${i2}  big_endian: false` : "")
    );
  };

  switch (req.protocol) {
    case "websocket": {
      const w = req as WsRequest;
      const url = conn?.url ?? w.url;
      const closeAfter = conn?.closeAfter ?? w.closeAfter;
      const close =
        closeAfter && closeAfter !== 1
          ? `\n${ind}close_after: ${closeAfter}`
          : "";
      return `${ind}url: ${yq(resolveTmpl(url))}${close}${messagesYaml(w.messages)}`;
    }
    case "tcp": {
      const t = req as TcpRequest;
      const url = conn?.url ?? t.url;
      const payload = t.payload
        ? `\n${ind}payload: ${yq(resolveTmpl(t.payload))}`
        : "";
      const ptype =
        t.payloadType && t.payloadType !== "text"
          ? `\n${ind}payload_type: ${t.payloadType}`
          : "";
      return `${ind}url: ${yq(resolveTmpl(url))}${framingYaml(t)}${payload}${ptype}${messagesYaml(t.messages ?? [])}`;
    }
    case "udp": {
      const u = req as UdpRequest;
      const url = conn?.url ?? u.url;
      const payload = u.payload
        ? `\n${ind}payload: ${yq(resolveTmpl(u.payload))}`
        : "";
      const ptype =
        u.payloadType && u.payloadType !== "text"
          ? `\n${ind}payload_type: ${u.payloadType}`
          : "";
      return `${ind}protocol: udp\n${ind}url: ${yq(resolveTmpl(url))}${payload}${ptype}${messagesYaml(u.messages ?? [])}`;
    }
    case "grpc": {
      const g = req as GrpcRequest;
      const url = conn?.url ?? g.url;
      const msg = g.message ? `\n${ind}message: ${yq(g.message)}` : "";
      const mf =
        g.messageFormat && g.messageFormat !== "json"
          ? `\n${ind}message_format: ${g.messageFormat}`
          : "";
      const rf = g.responseFormat
        ? `\n${ind}response_format: ${g.responseFormat}`
        : "";
      const st = g.streaming ? `\n${ind}streaming: ${g.streaming}` : "";
      return `${ind}url: ${yq(resolveTmpl(url))}\n${ind}service: ${yq(g.service ?? "")}${msg}${mf}${rf}${st}`;
    }
    case "sse": {
      const s = req as SseRequest;
      const url = conn?.url ?? s.url;
      const max =
        s.maxEvents && s.maxEvents !== 50
          ? `\n${ind}max_events: ${s.maxEvents}`
          : "";
      return `${ind}protocol: sse\n${ind}url: ${yq(resolveTmpl(url))}${max}`;
    }
    case "graphql": {
      const g = req as GraphqlRequest;
      const url = conn?.url ?? g.url;
      const q = g.query ? `\n${ind}query: ${yq(g.query)}` : "";
      const v = g.variables ? `\n${ind}variables: ${yq(g.variables)}` : "";
      const op = g.operationName
        ? `\n${ind}operation_name: ${yq(g.operationName)}`
        : "";
      return `${ind}url: ${yq(resolveTmpl(url))}${q}${v}${op}`;
    }
    default:
      return "";
  }
}

/** The connection block of a connection-based collection (connection-level config independent of the request block).
 * Not emitted in M0 (the backend RequestSpec does not support the connection key yet; message nodes stay compatible by inheriting into the request block);
 * once the backend supports it in M1, message nodes of connection-based collections carry their connection config through this block. */
export function connectionYaml(
  conn: ConnectionConfig,
  envVars: Record<string, string>,
  ind: string,
): string {
  const resolveTmpl = (s: string) =>
    s.replace(/\{\{(\w+)\}\}/g, (_, k: string) => envVars[k] ?? `{{${k}}}`);
  const yq = (s: string) =>
    `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
  const i2 = ind + "  ";
  let out = `${ind}url: ${yq(resolveTmpl(conn.url ?? ""))}`;
  if (conn.framing && conn.framing.mode !== "read_until_close") {
    out +=
      `\n${ind}framing:\n${i2}  mode: ${conn.framing.mode}` +
      (conn.framing.delimiter
        ? `\n${i2}  delimiter: ${yq(conn.framing.delimiter)}`
        : "") +
      (conn.framing.fixedLen
        ? `\n${i2}  fixed_len: ${conn.framing.fixedLen}`
        : "") +
      (conn.framing.bigEndian === false ? `\n${i2}  big_endian: false` : "");
  }
  if (conn.closeAfter) out += `\n${ind}close_after: ${conn.closeAfter}`;
  if (conn.codec) out += `\n${ind}codec: ${yq(conn.codec)}`;
  if (conn.timeoutMs) out += `\n${ind}timeout_ms: ${conn.timeoutMs}`;
  return out;
}

/** Scenario YAML run parameters */
export interface ScenarioYamlOptions {
  /** Loop count (how many times the whole scenario runs; >=1, defaults to 1) */
  iterations?: number;
  /** Failure policy. The engine's OnError only has stop / continue;
   *  next-loop has no engine primitive → emitted as stop, and the frontend orchestration layer decides at the loop boundary whether to continue. */
  onError?: FailurePolicy;
  /** Script library table: expands references into concrete actions so the scenario YAML is self-contained */
  library?: ActionTemplate[];
}

/** Build Sequential executor YAML from a scenario */
export function buildScenarioYaml(
  scenario: Scenario,
  requests: Record<string, ApiRequest>,
  envVars: Record<string, string>,
  collections?: Collection[],
  opts?: ScenarioYamlOptions,
): string {
  const stepsYaml = scenario.steps
    .map((s) => stepToYaml(s, requests, envVars, 4, collections, opts?.library))
    .filter(Boolean) // remove empty strings (disabled steps, steps with no bound request, etc.)
    .join("");
  const iterations = Math.max(1, Math.floor(opts?.iterations ?? 1));
  const onError = opts?.onError === "continue" ? "continue" : "stop";
  // Variables are injected into the plan's top-level variables (environment variables + data set row variables, already merged by the caller),
  // and the engine interpolates them on every request (the scheduler injects plan.variables into FlowRunner)
  const varsYaml = Object.entries(envVars)
    .map(
      ([k, v]) =>
        `  ${JSON.stringify(k)}: "${v.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`,
    )
    .join("\n");
  const variablesBlock = varsYaml ? `variables:\n${varsYaml}\n` : "";
  return `name: "${scenario.name.replace(/"/g, '\\"')}"
${variablesBlock}scenarios:
  - name: "${scenario.name.replace(/"/g, '\\"')}"
    executor:
      type: sequential
      iterations: ${iterations}
    on_error: ${onError}
    steps:
${stepsYaml}`;
}

/** Build one-shot sequential YAML from a single request (for one-off debugging of non-HTTP protocols) */
export function buildSingleRequestYaml(
  req: ApiRequest,
  envVars: Record<string, string>,
  /** Script library table: expands references into concrete actions (self-contained export) */
  library?: ActionTemplate[],
): string {
  const step: ScenarioStep = {
    id: "single",
    type: "request",
    name: req.name || "request",
    requestId: req.id,
  };
  const stepsYaml = stepToYaml(
    step,
    { [req.id]: req },
    envVars,
    4,
    undefined,
    library,
  );
  return `name: "Single Request"
scenarios:
  - name: "single"
    executor:
      type: sequential
      iterations: 1
    on_error: stop
    steps:
${stepsYaml}`;
}
