// Environment domain: environment CRUD, global variables, global secrets, active environment.
import type { StateCreator } from "zustand";
import type { Environment } from "@/data/types";
import { seedEnvironments, uid } from "@/data/seed";
import { saveNow } from "@/lib/persistence";
import type { AppState } from "../types";

export interface EnvironmentSlice {
  environments: Environment[];
  activeEnvId: string | null;
  globalVariables: Record<string, string>;
  globalSecrets: Record<string, string>;

  addEnvironment: (name: string) => void;
  renameEnvironment: (id: string, name: string) => void;
  updateEnvironment: (id: string, updates: Partial<Environment>) => void;
  removeEnvironment: (id: string) => void;
  /** Update global variables (shared by all environments; when sending, globalVariables are merged first, then environment variables) */
  updateGlobalVariables: (vars: Record<string, string>) => void;
  /** Update global secrets (a read-only secret store that scripts/templates may read but never write) */
  updateGlobalSecrets: (secrets: Record<string, string>) => void;
  setActiveEnv: (id: string | null) => void;
}

export const createEnvironmentSlice: StateCreator<
  AppState,
  [],
  [],
  EnvironmentSlice
> = (set) => ({
  environments: seedEnvironments(),
  activeEnvId: "env-dev",
  globalVariables: { password: "secret123", coupon: "SAVE20" },
  globalSecrets: {},

  addEnvironment: (name) => {
    set((s) => ({
      environments: [
        ...s.environments,
        { id: uid("env"), name, variables: {}, secrets: {} },
      ],
    }));
    void saveNow();
  },
  renameEnvironment: (id, name) => {
    set((s) => ({
      environments: s.environments.map((e) =>
        e.id === id ? { ...e, name } : e,
      ),
    }));
    void saveNow();
  },
  updateEnvironment: (id, updates) => {
    set((s) => ({
      environments: s.environments.map((e) =>
        e.id === id ? { ...e, ...updates } : e,
      ),
    }));
    void saveNow();
  },
  removeEnvironment: (id) => {
    set((s) => ({
      environments: s.environments.filter((e) => e.id !== id),
      activeEnvId: s.activeEnvId === id ? null : s.activeEnvId,
    }));
    void saveNow();
  },
  updateGlobalVariables: (vars) => {
    set({ globalVariables: vars });
    void saveNow();
  },
  updateGlobalSecrets: (secrets) => {
    set({ globalSecrets: secrets });
    void saveNow();
  },
  setActiveEnv: (id) => set({ activeEnvId: id }),
});
