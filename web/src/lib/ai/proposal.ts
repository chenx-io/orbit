// Pure helpers for the AI proposal card: tool name → i18n key, field value formatting, diff kind labels.
// Kept under lib (no React component exports) to satisfy oxlint's react(only-export-components) rule.
import type { AiDiffKind, AiProposalAction, AiToolKind } from "@/data/aiTypes";

/** Tool name → i18n copy key (`ai.tool.<camelCase name>`). */
export function toolLabelKey(name: string): string {
  return `ai.tool.${name}`;
}

/** Keys whose value is used as the name directly (order is priority: name > title > URL). */
const SUBJECT_NAME_KEYS = ["name", "title", "url"];
/** Write-tool payloads wrap an entity: create_request's name lives at `request.name`, a scenario's at `scenario.name`, and so on */
const SUBJECT_ENTITY_KEYS = [
  "request",
  "scenario",
  "suite",
  "dataSet",
  "collection",
  "model",
];
/** Fallback: `update_*` / `run_*` only carry an id; not descriptive, but at least it distinguishes "not the same call". */
const SUBJECT_ID_KEYS = [
  "requestId",
  "scenarioId",
  "suiteId",
  "dataSetId",
  "collectionId",
  "id",
];

function pickString(
  source: Record<string, unknown>,
  keys: string[],
): string | null {
  for (const key of keys) {
    const value = source[key];
    if (typeof value !== "string") continue;
    const text = value.trim();
    // The collapsed row is a single line: only short, single-line text is accepted (long or multi-line text becomes noise again)
    if (text && !text.includes("\n") && text.length <= 60) return text;
  }
  return null;
}

/**
 * Pick a "title" from the tool arguments for the collapsed row.
 *
 * When the collapsed row shows only the tool name ("Create request"), a run of consecutive create_request calls is indistinguishable;
 * yet the arguments do carry an entity name (`request.name` / `scenario.name` / `name`),
 * so using it as the title makes "what this call did" obvious at a glance — the explicitly requested "show the title when collapsed by default".
 * Returns null when there is nothing (better to show only the tool name than to invent content).
 */
export function toolSubject(
  args?: Record<string, unknown> | null,
): string | null {
  if (!args) return null;
  const direct = pickString(args, SUBJECT_NAME_KEYS);
  if (direct) return direct;
  for (const key of SUBJECT_ENTITY_KEYS) {
    const nested = args[key];
    if (nested && typeof nested === "object") {
      const found = pickString(
        nested as Record<string, unknown>,
        SUBJECT_NAME_KEYS,
      );
      if (found) return found;
    }
  }
  return pickString(args, SUBJECT_ID_KEYS);
}

/** Tool permission tier → copy key. */
export function toolKindLabelKey(kind: AiToolKind): string {
  return `ai.kind.${kind}`;
}

/** Diff kind → copy key. */
export function diffKindLabelKey(kind: AiDiffKind): string {
  return `ai.diff.${kind}`;
}

/** Whether a value "carries no information" (empty string / null / empty array / empty object). */
export function isEmptyValue(value: unknown): boolean {
  if (value === null || value === undefined) return true;
  if (typeof value === "string") return value.trim() === "";
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object")
    return Object.keys(value as object).length === 0;
  return false;
}

/**
 * Whether it counts as "default-value noise" that does not deserve a row in the change card.
 *
 * Only **provably uninformative** defaults are listed (currently `auth.type = "none"`): it always appears when creating a request,
 * yet tells the user nothing about "what this call did". Every other value is kept — better an extra row than hiding a real change.
 */
export function isNeutralDefault(path: string, value: unknown): boolean {
  return path.endsWith("auth.type") && value === "none";
}

/** Whether this row should be skipped in the change card. `createOnly` = a create-type change (the before side is always empty, so only the new value matters). */
export function isNoiseDiff(
  diff: { path: string; before?: unknown; after?: unknown },
  createOnly: boolean,
): boolean {
  if (createOnly) {
    return isEmptyValue(diff.after) || isNeutralDefault(diff.path, diff.after);
  }
  // Update-type: show it whenever either side has content (a deleted field has an empty after side, but the user must still see it)
  return isEmptyValue(diff.before) && isEmptyValue(diff.after);
}

/** Proposal action → i18n key (for the card subtitle). */
export function actionLabelKey(action: AiProposalAction): string {
  return `ai.action.${action.type}`;
}

/** Format a field value in a proposal: strings are emitted directly, everything else uses compact JSON. */
export function formatProposalValue(value: unknown): string {
  if (value === undefined || value === null) return "—";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean")
    return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

/** Whether a value can be shown inline directly (short string / number / boolean). */
export function isInlineValue(value: unknown): boolean {
  if (typeof value === "number" || typeof value === "boolean") return true;
  return (
    typeof value === "string" && value.length <= 60 && !value.includes("\n")
  );
}

/**
 * Pick the most valuable diffs to show (at most `limit` of them).
 *
 * Ordering intent: show the fields "the user cares about most" first — url/method/name/prereqScript/postreqScript,
 * then the rest in path order, so long arrays (headers/assertions) do not crowd out the visual focus.
 */
const PRIORITY_PATTERNS = [
  "name",
  "method",
  "url",
  "body",
  "prereqScript",
  "postreqScript",
  "steps",
  "assertions",
];

/**
 * Action-list fields (by their first path segment): changes like `preActions[1].sql` always come first.
 *
 * Actions are one of the AI's main battlegrounds, and these paths end in `sql` / `code` / `library_id`,
 * so ordering by "tail segment priority" would push them past the 12-row cutoff behind long arrays like headers / columns —
 * and the user would never see "which field of which action actually changed".
 */
const ACTION_PATH_HEADS = ["preActions", "postActions", "preResolveActions"];

export function pickImportantDiffs<T extends { path: string }>(
  diffs: T[],
  limit = 12,
): T[] {
  const rank = (path: string) => {
    const segments = path.split(".");
    const head = segments[0] ?? path;
    if (ACTION_PATH_HEADS.some((field) => head.startsWith(field))) return 0;
    const tail = segments[segments.length - 1] ?? path;
    const hit = PRIORITY_PATTERNS.indexOf(tail);
    return hit === -1 ? PRIORITY_PATTERNS.length + 1 : hit + 1;
  };
  return [...diffs]
    .sort((a, b) => {
      const ra = rank(a.path);
      const rb = rank(b.path);
      if (ra !== rb) return ra - rb;
      return a.path.localeCompare(b.path);
    })
    .slice(0, limit);
}
