// Scenario tree utility: flatten scenarios + scenarioFolders into a renderable tree-node sequence.
// Search filtering (ancestor folders of matched cases are kept automatically), collapse/expand, per-folder case count.
import type { Scenario, ScenarioFolder } from "@/data/types";
import { getUiLocale } from "@/lib/localeDict";

export type TreeNodeKind = "folder" | "case";

export interface ScenarioTreeNode {
  kind: TreeNodeKind;
  /** folder.id / scenario.id (the two id prefixes differ, so they never collide) */
  id: string;
  name: string;
  depth: number;
  parentId: string | null;
  /** folder: whether collapsed */
  collapsed?: boolean;
  /** folder: number of cases in the subtree */
  caseCount?: number;
  /** case: the original scenario */
  scenario?: Scenario;
}

export type DropPos = "before" | "after" | "inside";

export interface DropTarget {
  id: string;
  kind: TreeNodeKind;
  pos: DropPos;
}

function folderSubtreeIds(
  folders: ScenarioFolder[],
  rootId: string,
): Set<string> {
  const ids = new Set<string>([rootId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const f of folders) {
      if (f.parentId && ids.has(f.parentId) && !ids.has(f.id)) {
        ids.add(f.id);
        changed = true;
      }
    }
  }
  return ids;
}

/** Whether folderId lies within ancestorId and its subtree (cycle guard: a folder may not be dragged into itself / its own subtree) */
export function isDescendantFolder(
  folders: ScenarioFolder[],
  folderId: string,
  ancestorId: string,
): boolean {
  return folderSubtreeIds(folders, ancestorId).has(folderId);
}

/** Number of cases within a folder's subtree */
export function folderCaseCount(
  scenarios: Scenario[],
  folders: ScenarioFolder[],
  folderId: string,
): number {
  const ids = folderSubtreeIds(folders, folderId);
  return scenarios.filter((s) => s.folderId && ids.has(s.folderId)).length;
}

const byName = (a: { name: string }, b: { name: string }): number =>
  a.name.localeCompare(b.name, getUiLocale());

/**
 * Build the tree-node sequence (already expanded by depth; a collapsed folder's subtree is not emitted).
 * When query is non-empty: keep matched cases and the "ancestor folders of matched cases + folders whose name matches".
 */
export function buildScenarioTree(
  scenarios: Scenario[],
  folders: ScenarioFolder[],
  query: string,
): ScenarioTreeNode[] {
  const q = query.trim().toLowerCase();

  // Search filter: matched case ids and the folder ids to keep
  const visibleCaseIds = new Set<string>();
  const keepFolderIds = new Set<string>();
  if (q) {
    for (const f of folders)
      if (f.name.toLowerCase().includes(q)) keepFolderIds.add(f.id);
    for (const sc of scenarios) {
      if (sc.name.toLowerCase().includes(q)) {
        visibleCaseIds.add(sc.id);
        // Keep ancestor folders
        let cur = sc.folderId ?? null;
        while (cur) {
          keepFolderIds.add(cur);
          cur = folders.find((f) => f.id === cur)?.parentId ?? null;
        }
      }
    }
  }

  const out: ScenarioTreeNode[] = [];

  const walkFolders = (
    parentId: string | null,
    depth: number,
    ignoreCollapsed: boolean,
  ): void => {
    const children = folders
      .filter((f) => (f.parentId ?? null) === parentId)
      .sort(byName);
    for (const f of children) {
      if (q && !keepFolderIds.has(f.id)) continue;
      const count = folderCaseCount(scenarios, folders, f.id);
      out.push({
        kind: "folder",
        id: f.id,
        name: f.name,
        depth,
        parentId: f.parentId ?? null,
        collapsed: f.collapsed ?? false,
        caseCount: count,
      });
      // Expanded folder: recurse into subfolders and render the cases inside
      if (ignoreCollapsed || !(f.collapsed ?? false)) {
        walkFolders(f.id, depth + 1, ignoreCollapsed);
        walkCases(f.id, depth + 1);
      }
    }
  };

  const walkCases = (folderId: string | null, depth: number): void => {
    const cases = scenarios
      .filter((s) => (s.folderId ?? null) === folderId)
      .sort(byName);
    for (const sc of cases) {
      if (q && !visibleCaseIds.has(sc.id)) continue;
      out.push({
        kind: "case",
        id: sc.id,
        name: sc.name,
        depth,
        parentId: sc.folderId ?? null,
        scenario: sc,
      });
    }
  };

  if (q) {
    // When searching, force-expand matched folders (ignore collapsed so results stay visible)
    walkFolders(null, 0, true);
    walkCases(null, 0);
    return out;
  }

  walkFolders(null, 0, false);
  walkCases(null, 0);
  return out;
}
