// Pure helpers for the script library (reusable action templates): reference counting, construction, lookup.
//
// No React dependency (oxlint `react(only-export-components)`: files outside components export only functions/constants).
// Reference resolution (editing the library applies everywhere) happens in the engine before execution; this file only orchestrates and presents "selective reuse".
import type {
  ActionTemplate,
  DbActionItem,
  RequestAction,
  ScriptActionItem,
} from "@/data/types";
import { uid } from "@/data/seed";
import { t } from "@/lib/localeDict";
import { getPostActions, getPreActions, isRef } from "./requestActions";

/** Minimal request shape for referencing library items (the input of `getPreActions` / `getPostActions`) */
export interface ActionHostLike {
  id: string;
  name: string;
  prereqScript?: string;
  postreqScript?: string;
  preResolveActions?: RequestAction[];
  preActions?: RequestAction[];
  postActions?: RequestAction[];
}

/** Number of times a library item is referenced within one request */
export interface TemplateUsage {
  requestId: string;
  requestName: string;
  count: number;
}

/** Create a library item (empty description; defaults to an empty script action the editor can start from) */
export function newActionTemplate(
  name: string,
  action?: ScriptActionItem | DbActionItem,
): ActionTemplate {
  return {
    id: uid("tpl"),
    name: name.trim() || t("actionTemplate.untitled"),
    action: action ?? {
      id: uid("act"),
      kind: "script",
      name: "",
      enabled: true,
      language: "js",
      code: "",
    },
  };
}

/**
 * Whether the draft differs from the saved version (decides whether "save / cancel" is clickable).
 *
 * Only **editable fields** are compared (name / description / inner action), not `workspaceId` or `sortIndex`:
 * the former is stamped by the persistence layer on save, so counting it would keep showing "unsaved" even after saving.
 *
 * Inner actions are compared by serialization: differing field order only reports a change **extra** times,
 * at the cost of allowing one no-op save; it never reports "no change" for real edits and lose the ability to cancel.
 */
export function isTemplateDirty(
  draft: ActionTemplate,
  saved: ActionTemplate,
): boolean {
  return (
    draft.name !== saved.name ||
    (draft.description ?? "") !== (saved.description ?? "") ||
    JSON.stringify(draft.action) !== JSON.stringify(saved.action)
  );
}

/** Library item name (with an empty-value fallback, for cards / the picker panel) */
export function templateName(
  templates: ActionTemplate[],
  id: string,
): string | undefined {
  return templates.find((t) => t.id === id)?.name;
}

/** Whether the library item still exists (dangling-reference check) */
export function templateExists(
  templates: ActionTemplate[],
  id: string,
): boolean {
  return templates.some((t) => t.id === id);
}

/** Filter library items by name (picker search; a match on the name or description counts) */
export function filterTemplates(
  templates: ActionTemplate[],
  query: string,
): ActionTemplate[] {
  const keyword = query.trim().toLowerCase();
  if (!keyword) return templates;
  return templates.filter(
    (t) =>
      t.name.toLowerCase().includes(keyword) ||
      (t.description ?? "").toLowerCase().includes(keyword),
  );
}

/** Which requests reference a library item (with counts) — used by library management and the "warn before delete" flow */
export function templateUsages(
  templateId: string,
  requests: Record<string, ActionHostLike>,
): TemplateUsage[] {
  const out: TemplateUsage[] = [];
  for (const req of Object.values(requests)) {
    const list = [...getPreActions(req), ...getPostActions(req)];
    const count = list.filter(
      (a) => isRef(a) && a.libraryId === templateId,
    ).length;
    if (count > 0)
      out.push({ requestId: req.id, requestName: req.name, count });
  }
  return out.sort((a, b) => a.requestName.localeCompare(b.requestName));
}

/** Set of library item ids referenced by a request's action list (for list badges / dangling markers) */
export function referencedTemplateIds(
  requests: Record<string, ActionHostLike>,
): Set<string> {
  const ids = new Set<string>();
  for (const req of Object.values(requests)) {
    for (const a of [...getPreActions(req), ...getPostActions(req)]) {
      if (isRef(a)) ids.add(a.libraryId);
    }
  }
  return ids;
}
