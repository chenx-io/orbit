// Script library (reusable action templates) domain: item CRUD.
//
// A library item is a **workspace-level entity**: it follows snapshot partitioning (see the exhaustive helper in lib/persistence.ts)
// and is stamped with a workspaceId on save, so switching workspaces switches the whole library.
// Unlike data sources, which are app-level global entities and are left unchanged here.
import type { StateCreator } from "zustand";
import type { ActionTemplate } from "@/data/types";
import { uid } from "@/data/seed";
import { t } from "@/lib/localeDict";
import { saveNow } from "@/lib/persistence";
import { newActionTemplate } from "@/lib/actionLibrary";
import type { AppState } from "../types";

export interface ActionLibrarySlice {
  /** Script library items of the current workspace */
  actionTemplates: ActionTemplate[];

  /** Create a library item and return its new id (initial values may be passed in, for "save this action as a library item") */
  addActionTemplate: (partial?: Partial<ActionTemplate>) => string;
  updateActionTemplate: (id: string, updates: Partial<ActionTemplate>) => void;
  /**
   * Delete a library item.
   *
   * **Requests referencing it are deliberately left untouched**: the resulting dangling references are shown as broken by the
   * editor, which offers "reselect / delete"; execution logs an error without aborting the request — silent cleanup would make the
   * user silently lose the intent they composed.
   */
  removeActionTemplate: (id: string) => void;
  /** Duplicate a library item and return its new id (null when the source is missing) */
  duplicateActionTemplate: (id: string) => string | null;
}

export const createActionLibrarySlice: StateCreator<
  AppState,
  [],
  [],
  ActionLibrarySlice
> = (set, get) => ({
  actionTemplates: [],

  addActionTemplate: (partial) => {
    const base = newActionTemplate(partial?.name ?? "");
    // The id and the inner action id are always generated here, so callers cannot pass in duplicates
    const tpl: ActionTemplate = {
      ...base,
      ...partial,
      id: base.id,
      action: { ...base.action, ...(partial?.action ?? {}) },
    };
    set((s) => ({ actionTemplates: [...s.actionTemplates, tpl] }));
    void saveNow();
    return tpl.id;
  },

  updateActionTemplate: (id, updates) => {
    set((s) => ({
      actionTemplates: s.actionTemplates.map((t) =>
        t.id === id ? { ...t, ...updates, id: t.id } : t,
      ),
    }));
    void saveNow();
  },

  removeActionTemplate: (id) => {
    set((s) => ({
      actionTemplates: s.actionTemplates.filter((t) => t.id !== id),
    }));
    void saveNow();
  },

  duplicateActionTemplate: (id) => {
    const src = get().actionTemplates.find((t) => t.id === id);
    if (!src) return null;
    const copy: ActionTemplate = {
      ...src,
      id: uid("tpl"),
      name: `${src.name} ${t("common.copySuffix", "(copy)")}`,
      action: { ...src.action, id: uid("act") },
    };
    set((s) => ({ actionTemplates: [...s.actionTemplates, copy] }));
    void saveNow();
    return copy.id;
  },
});
