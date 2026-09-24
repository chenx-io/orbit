// Data source domain: global connection configs (reused across workspaces, used by DB/Redis assertions).
// Any change triggers saveNow() persistence (consistent with environmentSlice).
import type { StateCreator } from "zustand";
import type { DataSource } from "@/data/types";
import { uid } from "@/data/seed";
import { saveNow } from "@/lib/persistence";
import type { AppState } from "../types";

export interface DataSourceSlice {
  /** All data source connections (global entities) */
  dataSources: DataSource[];

  /** Replace all (import / snapshot restore) */
  setDataSources: (items: DataSource[]) => void;
  /** Add / update one and persist */
  upsertDataSource: (ds: DataSource) => void;
  /** Delete one and persist */
  removeDataSource: (id: string) => void;
  /** Build default values for a new data source */
  blankDataSource: () => DataSource;
}

const defaults = (): Omit<DataSource, "id" | "name" | "kind" | "url"> => ({
  maxConnections: 8,
  minIdle: 1,
  connectTimeoutMs: 5000,
  acquireTimeoutMs: 5000,
  queryTimeoutMs: 10000,
  idleTtlSecs: 300,
  readonly: true,
  enabled: true,
});

export const createDataSourceSlice: StateCreator<
  AppState,
  [],
  [],
  DataSourceSlice
> = (set) => ({
  dataSources: [],

  setDataSources: (items) => {
    set({ dataSources: items });
    void saveNow();
  },

  upsertDataSource: (ds) => {
    set((s) => {
      const exists = s.dataSources.some((d) => d.id === ds.id);
      return {
        dataSources: exists
          ? s.dataSources.map((d) => (d.id === ds.id ? ds : d))
          : [...s.dataSources, ds],
      };
    });
    void saveNow();
  },

  removeDataSource: (id) => {
    set((s) => ({
      dataSources: s.dataSources.filter((d) => d.id !== id),
    }));
    void saveNow();
  },

  blankDataSource: () => ({
    id: uid("ds"),
    name: "",
    kind: "mysql",
    url: "",
    ...defaults(),
  }),
});
