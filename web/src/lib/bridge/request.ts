// Dynamic value generation + request execution (proxy).
import type {
  ActionLog,
  ActionTemplate,
  Assertion,
  AssertionResult,
  KeyValue,
  RequestAction,
  ScriptLog,
  TestResult,
} from "@/data/types";
import {
  toWireAction,
  toWireActions,
  type WireAction,
} from "@/lib/requestActions";
import type { WireRequestTemplate } from "@/lib/requestTemplate";
import { apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

/** Dynamic value (e.g. faker data), used for template variable input. */
export async function generateDynamicValue(
  category: string,
  method: string,
  args?: string,
): Promise<string> {
  if (isTauri()) {
    return tauriInvoke<string>("generate_dynamic_value", {
      category,
      method,
      args,
    });
  }
  return apiPost<string>("/api/dynamic", { category, method, args });
}

/** Parse a whole dynamic-value expression (including `|` pipes); executed entirely on the Rust side. */
export async function resolveDynamicValues(input: string): Promise<string> {
  if (isTauri()) {
    return tauriInvoke<string>("resolve_dynamic_values", { input });
  }
  return apiPost<string>("/api/dynamic/resolve", { input });
}

export type { ScriptLog, TestResult };

export interface ProxyResponse {
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
  /** Assertion results (built-in + DB/Redis), evaluated after the response and rendered uniformly in the frontend Tests tab */
  assertions?: AssertionResult[];
  /** Pre-request action execution results (a single list including the built-in interpolation node entry, in execution order) */
  preActions?: ActionLog[];
  /** Post-response action execution results (scripts / database, in configured order) */
  postActions?: ActionLog[];
  /** Variables written by actions (DB queries) */
  actionVars?: Record<string, string>;
  /** Request snapshot after pre-request script rewriting (method/url/headers/body); the frontend merges it into sentRequest for display and code generation */
  request?: {
    method: string;
    url: string;
    headers: Record<string, string>;
    body: string;
  } | null;
}

function headersToRecord(headers: KeyValue[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const h of headers) if (h.enabled && h.key) out[h.key] = h.value;
  return out;
}

export async function executeRequest(input: {
  method: string;
  /** Target address on the passthrough path; on the template path it is decided by `requestTemplate.url` */
  url?: string;
  /** Passthrough path: already-built request headers */
  headers?: KeyValue[];
  /** Passthrough path: already-built request body text */
  body?: string;
  /** base64 of a binary / multipart body (pre-read in the browser; takes precedence over body when sending) */
  bodyBinary?: string | null;
  /** Body mode (in Tauri path mode it lets Rust decide how to assemble the body) */
  bodyMode?: string | null;
  /** Tauri path mode (binary): the file's real absolute path; Rust reads it from disk and sends it */
  binaryFilePath?: string | null;
  /** Tauri path mode (form-data): structured fields (files carry a real path); Rust builds the multipart body.
   *  Note: like the rest of ExecuteRequest it uses the snake_case wire format (file_path/file_type/filename). */
  formParams?: Array<{
    key: string;
    value?: string;
    file_path?: string;
    file_type?: string;
    filename?: string;
  }> | null;
  /** Un-interpolated request template: when present the engine takes over interpolation/encoding/assembly (all one-off HTTP sends use this) */
  requestTemplate?: WireRequestTemplate | null;
  /** Pre-request action list (a single ordered list including the built-in node: before the anchor = pre-interpolation, after it = post-interpolation) */
  preActions?: RequestAction[] | null;
  /** Post-response action list */
  postActions?: RequestAction[] | null;
  /**
   * Script library table (reusable action templates of the current workspace).
   *
   * Sent from the **same source** as the action list: the engine uses it to expand `{ type: ref }` into concrete actions;
   * a missing entry only logs an error and never aborts the request.
   */
  actionTemplates?: ActionTemplate[] | null;
  /** Pre-request script (passthrough path; runs after variable resolution and before sending) */
  prereqScript?: string | null;
  /** Post-response script (runs after the response is received) */
  postreqScript?: string | null;
  /** Snapshot of the current environment variables, read by scripts via pm.environment.get */
  envVars?: Record<string, string> | null;
  /** Post-response assertion config (built-in + DB/Redis) */
  assertions?: Assertion[] | null;
  /** Build only, do not send: return the final request (a distributed agent resolves it on the control side first) */
  dryRun?: boolean;
}): Promise<ProxyResponse> {
  const headers = input.headers ?? [];
  if (isTauri()) {
    return tauriInvoke<ProxyResponse>("execute_request", {
      request: {
        method: input.method,
        url: input.url ?? "",
        headers: headersToRecord(headers),
        body: input.body ?? "",
        body_binary: input.bodyBinary ?? null,
        body_mode: input.bodyMode ?? null,
        binary_file_path: input.binaryFilePath ?? null,
        form_params: input.formParams ?? null,
        request_template: input.requestTemplate ?? null,
        pre_actions: actionWire(input.preActions),
        post_actions: actionWire(input.postActions),
        action_templates: templateWire(input.actionTemplates),
        prereq_script: input.prereqScript ?? null,
        postreq_script: input.postreqScript ?? null,
        env_vars: input.envVars ?? null,
        checks: input.assertions?.length ? input.assertions : null,
        dry_run: input.dryRun ?? null,
      },
    });
  }
  // Browser preview mode: the backend falls back to base64, so structured path fields are not sent; scripts are sent as usual
  return apiPost<ProxyResponse>("/api/proxy", {
    method: input.method,
    url: input.url ?? "",
    headers: headersToRecord(headers),
    body: input.body ?? "",
    body_binary: input.bodyBinary ?? null,
    request_template: input.requestTemplate ?? null,
    pre_actions: actionWire(input.preActions),
    post_actions: actionWire(input.postActions),
    action_templates: templateWire(input.actionTemplates),
    prereq_script: input.prereqScript ?? null,
    postreq_script: input.postreqScript ?? null,
    env_vars: input.envVars ?? null,
    checks: input.assertions?.length ? input.assertions : null,
    dry_run: input.dryRun ?? null,
  });
}

/** Action list → wire format (enabled items only; an empty list sends null to omit the field) */
function actionWire(actions?: RequestAction[] | null): WireAction[] | null {
  return actions?.length ? toWireActions(actions) : null;
}

/** Wire-format library item: the inner action becomes the `{ type }`-tagged form (the store holds the frontend `kind` form) */
type WireTemplate = Omit<ActionTemplate, "action"> & { action: WireAction };

/**
 * Library table → wire format.
 *
 * **The store's library items cannot be sent as-is**: on the frontend the inner action is `kind`-tagged (`{ kind: "script", code }`),
 * while the Rust `RequestAction` is `type`-tagged (`#[serde(tag = "type")]`) — skipping this step makes the whole
 * `execute_request` fail to deserialize (`missing field 'type'`), which shows up as "as long as the library has one entry,
 * no request can be sent".
 *
 * No enabled filtering happens here: disabled entries must be sent as-is so that expansion can mean "a disabled library entry is globally disabled".
 */
function templateWire(
  templates?: ActionTemplate[] | null,
): WireTemplate[] | null {
  if (!templates?.length) return null;
  return templates.map((tpl) => ({ ...tpl, action: toWireAction(tpl.action) }));
}
