// Store entry point: composes the domain slices and exports the unified hook plus convenience selectors.
//
// Layout (zustand slice pattern):
// - types.ts           → the composed AppState type (union of the slice interfaces)
// - slices/*.ts        → one slice per domain (state + actions)
// - utils/*.ts         → pure helpers (tree operations / YAML building)
import { create } from "zustand";
import { useShallow } from "zustand/react/shallow";
import type { ApiRequest } from "@/data/types";
import type { AppState } from "./types";
import { createAppSlice } from "./slices/appSlice";
import { createCollectionSlice } from "./slices/collectionSlice";
import { createRequestSlice } from "./slices/requestSlice";
import { createRequestRunnerSlice } from "./slices/requestRunner";
import { createModelSlice } from "./slices/modelSlice";
import { createEnvironmentSlice } from "./slices/environmentSlice";
import { createActionLibrarySlice } from "./slices/actionLibrarySlice";
import { createDataSourceSlice } from "./slices/dataSourceSlice";
import { createHistorySlice } from "./slices/historySlice";
import { createScenarioSlice } from "./slices/scenarioSlice";
import { createPluginSlice } from "./slices/pluginSlice";
import { createLoadTestSlice } from "./slices/loadTestSlice";
import { createWorkspaceSlice } from "./slices/workspaceSlice";
import { createAiSlice } from "./slices/aiSlice";

export const useAppStore = create<AppState>()((...a) => ({
  ...createAppSlice(...a),
  ...createCollectionSlice(...a),
  ...createRequestSlice(...a),
  ...createRequestRunnerSlice(...a),
  ...createModelSlice(...a),
  ...createEnvironmentSlice(...a),
  ...createActionLibrarySlice(...a),
  ...createDataSourceSlice(...a),
  ...createHistorySlice(...a),
  ...createScenarioSlice(...a),
  ...createPluginSlice(...a),
  ...createLoadTestSlice(...a),
  ...createWorkspaceSlice(...a),
  ...createAiSlice(...a),
}));

// Convenience selectors
export function useActiveRequest(): ApiRequest | null {
  return useAppStore((s) => {
    const tab = s.tabs.find((t) => t.id === s.activeTabId);
    if (!tab) return null;
    return s.drafts[tab.requestId] ?? s.requests[tab.requestId] ?? null;
  });
}
export function useEffectiveVars(): Record<string, string> {
  return useAppStore(
    useShallow((s) => {
      const env = s.environments.find((e) => e.id === s.activeEnvId);
      return { ...s.globalVariables, ...(env?.variables ?? {}) };
    }),
  );
}

/** Variable names available in the active environment (global variables + the current environment's variables + secrets, deduped).
 * Feeds {{var}} autocompletion in the script editor / body editor / URL input. */
export function useEnvVarNames(): string[] {
  return useAppStore(
    useShallow((s) => {
      const env = s.environments.find((e) => e.id === s.activeEnvId);
      const names = [
        ...Object.keys(s.globalVariables ?? {}),
        ...Object.keys(env?.variables ?? {}),
        ...Object.keys(env?.secrets ?? {}),
      ];
      return Array.from(new Set(names));
    }),
  );
}
