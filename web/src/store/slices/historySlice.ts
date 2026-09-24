// History domain.
import type { StateCreator } from "zustand";
import type { HistoryEntry } from "@/data/types";
import { seedHistory, uid } from "@/data/seed";
import { saveNow } from "@/lib/persistence";
import { dataAddHistory, dataClearHistory, isTauri } from "@/lib/bridge";
import type { AppState } from "../types";

export interface HistorySlice {
  history: HistoryEntry[];

  addHistoryEntry: (e: Omit<HistoryEntry, "id" | "timestamp">) => void;
  clearHistory: () => void;
}

export const createHistorySlice: StateCreator<
  AppState,
  [],
  [],
  HistorySlice
> = (set, get) => ({
  history: seedHistory,

  addHistoryEntry: (e) => {
    const entry = { ...e, id: uid("h"), timestamp: Date.now() };
    set((s) => ({
      history: [entry, ...s.history].slice(0, 100),
    }));
    // Tauri: module-level commands persist immediately (without waiting for the debounced snapshot); snapshot sync still guarantees consistency
    if (isTauri()) {
      void dataAddHistory({
        ...entry,
        workspaceId: get().activeWorkspaceId ?? undefined,
      }).catch(() => {});
    }
  },
  clearHistory: () => {
    set({ history: [] });
    if (isTauri()) {
      void dataClearHistory().catch(() => {});
    } else {
      void saveNow();
    }
  },
});
