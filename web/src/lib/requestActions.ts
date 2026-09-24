// Pre/post-request "action" helpers: normalization (single list + built-in interpolation node), construction and wire-format serialization.
//
// Single-entry convention: always read pre-request actions via getPreActions (internally collapsed to "one list + one built-in node"),
// and post-response actions via getPostActions. Never duplicate the compat checks inside components.
//
// The pre-request list order is the execution order: actions **before** the built-in node run before variable interpolation (they may write variables
// for this round and may rewrite the request template), and actions **after** it run on the final message (rewrites are the final bytes — ideal for signing / encryption).
import type {
  ActionColumnVar,
  ActionTemplate,
  AssertionDbTarget,
  DbActionItem,
  InterpolateActionItem,
  RefActionItem,
  RequestAction,
  ScriptActionItem,
} from "@/data/types";
import { uid } from "@/data/seed";
import { t, tFormat } from "@/lib/localeDict";

/** Minimal shape of the legacy single-script fields (present on every request type) */
export interface ScriptFields {
  prereqScript?: string;
  postreqScript?: string;
  /**
   * @deprecated The "pre-interpolation actions" field from the previous two-stage rework.
   * **Read-compat only**: merged before the built-in interpolation node during normalization; new write paths only write the single `preActions` list.
   */
  preResolveActions?: RequestAction[];
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

/** Fixed id of the built-in interpolation node (stable across renders and persistence; nothing beyond deduplication) */
export const INTERPOLATE_ACTION_ID = "interpolate";

/** Whether this is the built-in interpolation node */
export function isInterpolate(a: RequestAction): a is InterpolateActionItem {
  return a.kind === "interpolate";
}

/** The built-in interpolation node (fixed shape; no "create" semantics) */
export function interpolateAction(): InterpolateActionItem {
  return { id: INTERPOLATE_ACTION_ID, kind: "interpolate" };
}

/** Whether this is a script library reference item */
export function isRef(a: RequestAction): a is RefActionItem {
  return a.kind === "ref";
}

/** Build a script library reference item (`name` is an optional display alias; defaults to the library entry name) */
export function newRefAction(libraryId: string, name?: string): RefActionItem {
  const alias = name?.trim();
  return {
    id: newActionId(),
    kind: "ref",
    libraryId,
    ...(alias ? { name: alias } : {}),
    enabled: true,
  };
}

/**
 * Ensure the list holds exactly **one** built-in interpolation node: duplicates keep only the first, and a missing one is prepended to the **front**.
 *
 * It is prepended rather than appended because existing actions (legacy `prereqScript` / legacy `preActions`) mean
 * "after interpolation" — only by prepending do they still run after interpolation (zero behavior migration).
 */
export function ensureInterpolate(actions: RequestAction[]): RequestAction[] {
  const out: RequestAction[] = [];
  let seen = false;
  for (const a of actions) {
    if (isInterpolate(a)) {
      if (seen) continue;
      seen = true;
    }
    out.push(a);
  }
  if (!seen) out.unshift(interpolateAction());
  return out;
}

/** Whether an action participates in execution (the built-in node is always enabled and cannot be disabled) */
export function isActionEnabled(a: RequestAction): boolean {
  return isInterpolate(a) || a.enabled !== false;
}

/** Filter by action enabled state (shared by execution and counting) */
export function enabledActions(actions: RequestAction[]): RequestAction[] {
  return actions.filter(isActionEnabled);
}

/**
 * Normalize pre-request actions (**the single entry point**): collapse to "one ordered list + one built-in interpolation node".
 *
 * - when explicit `preActions` is empty, fall back to the legacy single-script field `prereqScript` (no historical config lost);
 * - the previous `preResolveActions` is merged **before** the built-in node; if the list already contains the anchor it is treated as
 *   "already in single-list form" and the compat field is ignored (avoiding double insertion);
 * - returns a new array (the input is not mutated) containing at least one built-in interpolation node.
 */
export function getPreActions(
  req: ScriptFields | null | undefined,
): RequestAction[] {
  const legacy = (req?.prereqScript ?? "").trim();
  const base: RequestAction[] = req?.preActions?.length
    ? req.preActions
    : legacy
      ? [newScriptAction(req?.prereqScript as string, "legacy-prereq")]
      : [];
  const compat = req?.preResolveActions ?? [];
  const merged =
    base.some(isInterpolate) || compat.length === 0
      ? base
      : [...compat, interpolateAction(), ...base];
  return ensureInterpolate(merged);
}

/** Normalize post-response actions: falls back to the legacy `postreqScript` when the explicit list is empty. */
export function getPostActions(
  req: ScriptFields | null | undefined,
): RequestAction[] {
  if (!req) return [];
  const list = req.postActions ?? [];
  if (list.length > 0) return list;
  const legacy = (req.postreqScript ?? "").trim();
  return legacy ? [newScriptAction(req.postreqScript as string)] : [];
}

/** Generate an action id (used by frontend drag & drop / React keys) */
export function newActionId(): string {
  return uid("act");
}

/** Create a script action (`id` may be given explicitly so legacy-field normalization keeps a stable key) */
export function newScriptAction(
  code = "",
  id = newActionId(),
): ScriptActionItem {
  return { id, kind: "script", name: "", enabled: true, language: "js", code };
}

/** Create a database action (by default it writes the first row/column into a variable) */
export function newDbAction(datasource = ""): DbActionItem {
  return {
    id: newActionId(),
    kind: "db",
    name: "",
    enabled: true,
    datasource,
    sql: "SELECT 1",
    target: { type: "scalar" },
    extractVar: "",
    columns: [],
    row: 0,
  };
}

/** Action summary (the list card subtitle; never shows a password/connection string) */
export function summarizeAction(
  a: RequestAction,
  dsName?: (id: string) => string,
): string {
  // The built-in node and reference items are rendered by dedicated components, bypassing the summary
  if (isInterpolate(a)) return "";
  if (isRef(a)) {
    return tFormat("action.summary.library", a.name?.trim() || a.libraryId);
  }
  if (a.kind === "script") {
    const firstLine = (a.code ?? "").split("\n").find((l) => l.trim()) ?? "";
    return firstLine.trim() || t("action.summary.emptyScript");
  }
  const ds = a.datasource
    ? dsName
      ? dsName(a.datasource)
      : a.datasource
    : t("action.summary.noDataSource");
  const query = a.command
    ? `${a.command} ${(a.args ?? []).join(" ")}`.trim()
    : ((a.sql ?? "").split("\n").find((l) => l.trim()) ?? "");
  const vars = actionVarNames(a);
  const varPart = vars.length ? ` → ${vars.join(", ")}` : "";
  return `${ds} · ${query}${varPart}`;
}

/** Variable names the action writes (single value + multi-column mapping) */
export function actionVarNames(a: RequestAction): string[] {
  if (a.kind !== "db") return [];
  const names: string[] = [];
  if (a.extractVar?.trim()) names.push(a.extractVar.trim());
  for (const c of a.columns ?? []) {
    if (c.var?.trim()) names.push(c.var.trim());
  }
  return names;
}

// ─── Wire format (for Rust RequestAction deserialization; field names in snake_case) ───

export interface WireActionScript {
  type: "script";
  name?: string;
  enabled: boolean;
  language?: string;
  code: string;
}

export interface WireActionDb {
  type: "db";
  name?: string;
  enabled: boolean;
  datasource: string;
  sql?: string;
  command?: string;
  args?: string[];
  target?: { type: string; row?: number; column?: string; path?: string };
  extract_var?: string;
  columns?: ActionColumnVar[];
  row?: number;
  retry?: {
    interval_ms?: number;
    max_attempts?: number;
    timeout_ms?: number | null;
  };
}

/** The built-in interpolation node: the YAML / wire format is always `{ type: interpolate }` */
export interface WireActionInterpolate {
  type: "interpolate";
}

/**
 * Script library reference: wire format `{ type: "ref", library_id, name?, enabled }`.
 *
 * The library table is sent with the request by the caller (`action_templates`) and the engine expands it while mapping actions;
 * anything it cannot expand degrades to one error log entry and never fails the whole request.
 */
export interface WireActionRef {
  type: "ref";
  library_id: string;
  name?: string;
  enabled: boolean;
}

export type WireAction =
  WireActionScript | WireActionDb | WireActionInterpolate | WireActionRef;

/** Frontend action → backend wire format (drops the frontend-only id; kind → type) */
export function toWireAction(a: RequestAction): WireAction {
  if (isInterpolate(a)) return { type: "interpolate" };
  if (isRef(a)) {
    const out: WireActionRef = {
      type: "ref",
      library_id: a.libraryId,
      enabled: a.enabled !== false,
    };
    if (a.name?.trim()) out.name = a.name;
    return out;
  }
  if (a.kind === "script") {
    const out: WireActionScript = {
      type: "script",
      enabled: a.enabled !== false,
      code: a.code ?? "",
    };
    if (a.name?.trim()) out.name = a.name;
    if (a.language) out.language = a.language;
    return out;
  }
  const out: WireActionDb = {
    type: "db",
    enabled: a.enabled !== false,
    datasource: a.datasource ?? "",
  };
  if (a.name?.trim()) out.name = a.name;
  if (a.sql?.trim()) out.sql = a.sql;
  if (a.command?.trim()) out.command = a.command;
  if (a.args?.length) out.args = a.args;
  if (a.target) out.target = a.target;
  if (a.extractVar?.trim()) out.extract_var = a.extractVar.trim();
  const cols = (a.columns ?? []).filter(
    (c) => c.column?.trim() && c.var?.trim(),
  );
  if (cols.length) out.columns = cols;
  if (a.row) out.row = a.row;
  if (a.retry) out.retry = a.retry;
  return out;
}

/** Action list → wire-format list (enabled items only; an empty list returns null so the field can be omitted) */
export function toWireActions(actions: RequestAction[]): WireAction[] | null {
  const list = enabledActions(actions).map(toWireAction);
  return list.length ? list : null;
}

// ─── Snapshot (persistence) shape round-trip ───
//
// Snapshots round-trip through the Rust `orbit_data` **strongly-typed model**, so actions must be persisted in wire format
// (`type` tag + snake_case fields), and **disabled items must be kept** (`enabled: false` is user config too).
// If the "frontend shape (kind) → snapshot shape (type)" conversion is missing, actions are silently dropped in save/load round-trips.

/** Wire format → frontend action (re-adds a stable frontend id for drag & drop / React keys) */
export function fromWireAction(w: WireAction): RequestAction {
  if (w.type === "interpolate") return interpolateAction();
  if (w.type === "ref") {
    const alias = w.name?.trim();
    return {
      id: newActionId(),
      kind: "ref",
      libraryId: w.library_id,
      ...(alias ? { name: alias } : {}),
      enabled: w.enabled !== false,
    };
  }
  if (w.type === "script") {
    return {
      id: newActionId(),
      kind: "script",
      name: w.name ?? "",
      enabled: w.enabled !== false,
      language: "js",
      code: w.code ?? "",
    };
  }
  const out: DbActionItem = {
    id: newActionId(),
    kind: "db",
    name: w.name ?? "",
    enabled: w.enabled !== false,
    datasource: w.datasource ?? "",
  };
  if (w.sql !== undefined) out.sql = w.sql;
  if (w.command !== undefined) out.command = w.command;
  if (w.args?.length) out.args = w.args;
  if (w.target) out.target = w.target as AssertionDbTarget;
  if (w.extract_var?.trim()) out.extractVar = w.extract_var.trim();
  if (w.columns?.length)
    out.columns = w.columns.filter((c) => c.column && c.var);
  if (w.row) out.row = w.row;
  if (w.retry) out.retry = w.retry;
  return out;
}

/** Action list → snapshot shape (**keeps disabled items**; an empty list returns undefined to omit the field) */
export function actionsToPersisted(
  actions: RequestAction[],
): WireAction[] | undefined {
  return actions.length ? actions.map(toWireAction) : undefined;
}

/** Snapshot shape → action list (**keeps disabled items**; an empty array returns undefined) */
export function actionsFromPersisted(
  wire: WireAction[] | undefined,
): RequestAction[] | undefined {
  return wire?.length ? wire.map(fromWireAction) : undefined;
}

// ─── Compat: action list → legacy single-script string (fallback for import/export / load-test YAML) ───

/**
 * Degrade an action list to a single script string (script actions only; DB actions and the built-in node are ignored when unrepresentable).
 * Used by exits that still need the legacy format (Postman export, session protocols, etc.).
 */
export function actionsToLegacyScript(
  actions: RequestAction[],
): string | undefined {
  const scripts = enabledActions(actions)
    .filter((a): a is ScriptActionItem => a.kind === "script")
    .map((a) => a.code)
    .filter((c) => c && c.trim());
  return scripts.length ? scripts.join("\n\n") : undefined;
}

/** Whether any enabled database action exists (for the session-protocol downgrade warning) */
export function hasDbAction(actions: RequestAction[]): boolean {
  return actions.some((a) => a.kind === "db" && a.enabled);
}

// ─── Script library reference expansion (self-contained export / scenario YAML) ───

/**
 * Expand script library references in the action list — same rules as the backend `orbit_config::resolve_action_refs`.
 *
 * - matched entry → replace in place with the entry's current content (**position unchanged**): the display name is "ref alias → library name",
 *   and the enabled state is "ref enabled AND library enabled" (a disabled library entry is globally disabled);
 * - dangling / missing library entry → **keep the reference item as-is** (the exported file stays parseable; the engine logs an error);
 * - non-reference actions are returned unchanged.
 *
 * Purpose: called before collection export and scenario / load-test YAML generation so exported files do not depend on the library existing;
 * in-app snapshots still keep references (editing the library applies everywhere).
 */
export function resolveActionRefs(
  actions: RequestAction[],
  library: ActionTemplate[],
): RequestAction[] {
  if (!actions.some(isRef)) return actions;
  const table = new Map(library.map((t) => [t.id, t]));
  return actions.map((a) => {
    if (!isRef(a)) return a;
    const tpl = table.get(a.libraryId);
    if (!tpl) return a;
    return {
      ...tpl.action,
      // The expansion reuses the reference item's id: only for render keys / export, never persisted
      id: a.id,
      name: a.name?.trim() || tpl.name,
      enabled: a.enabled !== false && tpl.action.enabled !== false,
    } as RequestAction;
  });
}
