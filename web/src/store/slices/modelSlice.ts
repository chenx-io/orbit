// Data model (DataModel) domain plus sidebar switching.
import type { StateCreator } from "zustand";
import type { DataModel } from "@/data/types";
import { seedModels, uid } from "@/data/seed";
import { t } from "@/lib/localeDict";
import { saveNow } from "@/lib/persistence";
import type { AppState } from "../types";

export interface ModelSlice {
  models: DataModel[];
  sidebarTab: "collections" | "models";
  activeModelId: string | null;

  addModel: (name?: string) => string;
  updateModel: (id: string, updates: Partial<DataModel>) => void;
  removeModel: (id: string) => void;
  setSidebarTab: (t: "collections" | "models") => void;
  setActiveModel: (id: string | null) => void;
}

export const createModelSlice: StateCreator<AppState, [], [], ModelSlice> = (
  set,
) => ({
  models: seedModels,
  sidebarTab: "collections",
  activeModelId: null,

  addModel: (name) => {
    const id = uid("model");
    set((s) => ({
      models: [
        ...s.models,
        { id, name: name ?? t("model.new", "New model"), fields: [] },
      ],
    }));
    void saveNow();
    return id;
  },
  updateModel: (id, updates) => {
    set((s) => ({
      models: s.models.map((m) => (m.id === id ? { ...m, ...updates } : m)),
    }));
    void saveNow();
  },
  removeModel: (id) => {
    set((s) => ({ models: s.models.filter((m) => m.id !== id) }));
    void saveNow();
  },
  setSidebarTab: (t) =>
    set((s) => ({
      sidebarTab: t,
      activeModelId:
        t === "collections"
          ? null
          : (s.activeModelId ?? s.models[0]?.id ?? null),
    })),
  setActiveModel: (id) => set({ activeModelId: id }),
});
