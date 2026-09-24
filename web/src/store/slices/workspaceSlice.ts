// Workspace domain: the project boundary, with its own collections/models/environments/automation/history/reports.
// The store holds domain data only for the active workspace; the full partition cache lives in workspaceData
// (sourced from v2 snapshot partitioning / captured on switch), and persistence is handled by snapshot sync (stamped by buildPersisted).
// duplicateWorkspace deep-copies (collection tree/requests/models/environments/scenarios) with all ids regenerated.
import type { StateCreator } from "zustand";
import type {
  ApiRequest,
  Collection,
  CollectionItem,
  DataModel,
  Environment,
  Scenario,
  ScenarioDataSet,
  ScenarioFolder,
  ScenarioStep,
  TestSuite,
  Workspace,
  WorkspaceData,
} from "@/data/types";
import {
  seedCollections,
  seedEnvironments,
  seedHistory,
  seedModels,
  seedScenarios,
  seedWorkspaces,
  uid,
} from "@/data/seed";
import { t } from "@/lib/localeDict";
import {
  captureWorkspaceData,
  emptyWorkspaceData,
  spreadWorkspaceData,
} from "@/lib/persistence";
import type { AppState } from "../types";

export interface WorkspaceSlice {
  workspaces: Workspace[];
  /** Currently active workspace; null = show the workspace list (selection page) */
  activeWorkspaceId: string | null;
  /** Full domain data partition cache (key = workspaceId): data of inactive workspaces is kept here */
  workspaceData: Record<string, WorkspaceData>;

  setActiveWorkspace: (id: string) => void;
  /** Return to the workspace list page (entry for the top switcher's "manage / back to list") */
  returnToWorkspaceList: () => void;
  addWorkspace: (name: string, description?: string) => string;
  /** Update the name / description (omitted fields stay unchanged) */
  updateWorkspace: (
    id: string,
    patch: { name?: string; description?: string },
  ) => void;
  /** Delete a workspace: also clears its partition cache */
  removeWorkspace: (id: string) => void;
  /** Duplicate a workspace: deep-copies collection tree + requests + models (with refModelId remapping) + environments + scenarios, regenerating all ids */
  duplicateWorkspace: (id: string) => void;
}

const WS_COLORS = [
  "#71717a",
  "#8b5cf6",
  "#10b981",
  "#f59e0b",
  "#ef4444",
  "#ec4899",
];

/** First-run default workspace partition seed (consistent with each slice's initial value; overridden by applyPersisted when a snapshot exists) */
function defaultWorkspaceSeed(): WorkspaceData {
  return {
    collections: seedCollections(),
    requests: {},
    models: seedModels,
    environments: seedEnvironments(),
    actionTemplates: [],
    scenarios: seedScenarios,
    scenarioFolders: [],
    scenarioDataSets: [],
    scenarioSuites: [],
    history: seedHistory,
    activeEnvId: "env-dev",
    globalVariables: { password: "secret123", coupon: "SAVE20" },
    globalSecrets: {},
  };
}

/** Deep-copy a collection tree: node ids are regenerated and requestId is rewritten by the mapping */
function cloneItems(
  items: CollectionItem[],
  remapReq: (rid?: string) => string | undefined,
): CollectionItem[] {
  return items.map((it) => {
    switch (it.type) {
      case "folder":
        return { ...it, id: uid("f"), items: cloneItems(it.items, remapReq) };
      case "request":
        return {
          ...it,
          id: uid("ci"),
          requestId: remapReq(it.requestId) ?? "",
        };
      case "grpc-package":
        return { ...it, id: uid("gpkg"), services: it.services };
      case "grpc-service":
        return { ...it, id: uid("gsvc"), methods: it.methods };
      case "grpc-rpc":
        return {
          ...it,
          id: uid("grpc"),
          requestId: remapReq(it.requestId) ?? "",
        };
    }
  });
}

/** Deep-copy scenario steps: requestId of request steps is rewritten by the mapping and container fields recurse */
function cloneSteps(
  steps: ScenarioStep[],
  remapReq: (rid?: string) => string | undefined,
): ScenarioStep[] {
  return steps.map((st) => {
    const base =
      st.type === "request"
        ? { ...st, requestId: remapReq(st.requestId) ?? "" }
        : { ...st };
    const out = base as ScenarioStep & {
      children?: ScenarioStep[];
      elseChildren?: ScenarioStep[];
    };
    if (out.children) out.children = cloneSteps(out.children, remapReq);
    if (out.elseChildren)
      out.elseChildren = cloneSteps(out.elseChildren, remapReq);
    return out;
  });
}

export const createWorkspaceSlice: StateCreator<
  AppState,
  [],
  [],
  WorkspaceSlice
> = (set, get) => ({
  workspaces: seedWorkspaces(),
  // Show the list first (when a snapshot exists, applyPersisted restores the remembered active workspace)
  activeWorkspaceId: null,
  workspaceData: { "ws-default": defaultWorkspaceSeed() },

  setActiveWorkspace: (id) => {
    const s = get();
    if (s.activeWorkspaceId === id) {
      get().setActiveModule("api");
      return;
    }
    // 1) Write the current workspace's data back to the partition cache (so switching back loses nothing)
    const flushed = s.activeWorkspaceId
      ? { ...s.workspaceData, [s.activeWorkspaceId]: captureWorkspaceData(s) }
      : s.workspaceData;
    const target = flushed[id] ?? emptyWorkspaceData();
    set({
      workspaceData: flushed,
      activeWorkspaceId: id,
      // Merge the target workspace's requests into the global map (the snapshot holds all requests globally anyway)
      requests: { ...s.requests, ...target.requests },
      ...spreadWorkspaceData(target, id),
      // After switching, clear transient cross-workspace selections (collection/scenario/tabs)
      activeCollectionId: null,
      activeScenarioId: null,
      activeTabId: null,
    });
    // Entering a workspace returns to an in-workspace module (API) instead of staying on an app-level module (distributed/plugins)
    get().setActiveModule("api");
  },

  returnToWorkspaceList: () => {
    const s = get();
    // Write the current workspace's data back to the partition cache (the list page holds no domain data, so saves no longer touch store domain fields)
    const flushed = s.activeWorkspaceId
      ? { ...s.workspaceData, [s.activeWorkspaceId]: captureWorkspaceData(s) }
      : s.workspaceData;
    // App-level modules (distributed/plugins) are full-screen shells, so we must fall back to an in-workspace module,
    // otherwise the App layer keeps rendering AppModuleShell and cannot switch to the workspace list page
    const activeModule =
      s.activeModule === "distributed" || s.activeModule === "plugins"
        ? "api"
        : s.activeModule;
    set({ workspaceData: flushed, activeWorkspaceId: null, activeModule });
  },

  addWorkspace: (name, description) => {
    const id = uid("ws");
    const ws: Workspace = {
      id,
      name: name.trim() || t("workspace.untitled"),
      description,
      color: WS_COLORS[get().workspaces.length % WS_COLORS.length],
      createdAt: Date.now(),
      sortIndex: get().workspaces.length,
    };
    set((s) => ({
      workspaces: [...s.workspaces, ws],
      workspaceData: { ...s.workspaceData, [id]: emptyWorkspaceData() },
    }));
    return id;
  },

  updateWorkspace: (id, patch) =>
    set((s) => ({
      workspaces: s.workspaces.map((w) =>
        w.id === id
          ? {
              ...w,
              name:
                patch.name !== undefined ? patch.name.trim() || w.name : w.name,
              description:
                patch.description !== undefined
                  ? patch.description
                  : w.description,
            }
          : w,
      ),
    })),

  removeWorkspace: (id) => {
    set((s) => {
      const active = s.activeWorkspaceId === id ? null : s.activeWorkspaceId;
      // Delete the partition cache (matching the backend's cascading cleanup of all data for that workspace)
      const workspaceData = { ...s.workspaceData };
      delete workspaceData[id];
      return {
        workspaces: s.workspaces.filter((w) => w.id !== id),
        workspaceData,
        activeWorkspaceId: active,
        // The current workspace is being deleted: return to the list page and clear the domain data
        ...(active === null ? spreadWorkspaceData(emptyWorkspaceData()) : {}),
      };
    });
  },

  duplicateWorkspace: (id) => {
    const s = get();
    const src = s.workspaces.find((w) => w.id === id);
    const data = s.workspaceData[id];
    if (!src || !data) return;
    const newId = uid("ws");

    // Id mapping for requests / models (collection tree nodes and scenario step references are rewritten too)
    const reqIdMap = new Map<string, string>();
    for (const rid of Object.keys(data.requests)) reqIdMap.set(rid, uid("req"));
    const modelIdMap = new Map<string, string>();
    for (const m of data.models) modelIdMap.set(m.id, uid("m"));
    const remapReq = (rid?: string) =>
      rid && reqIdMap.has(rid) ? reqIdMap.get(rid) : rid;
    const remapModel = (mid?: string) =>
      mid && modelIdMap.has(mid) ? modelIdMap.get(mid) : mid;

    const collections: Collection[] = data.collections.map((c) => ({
      ...c,
      id: uid("col"),
      workspaceId: newId,
      items: cloneItems(c.items, remapReq),
    }));

    const requests: Record<string, ApiRequest> = {};
    for (const [rid, r] of Object.entries(data.requests)) {
      const nid = reqIdMap.get(rid) ?? rid;
      requests[nid] = { ...r, id: nid } as ApiRequest;
      // The HTTP request's modelId references a model → remap it
      if (r.protocol === "http" && "modelId" in r && r.modelId) {
        (requests[nid] as ApiRequest & { modelId?: string }).modelId =
          remapModel((r as ApiRequest & { modelId?: string }).modelId);
      }
    }

    const models: DataModel[] = data.models.map((m) => ({
      ...m,
      id: modelIdMap.get(m.id) ?? m.id,
      fields: m.fields.map((f) =>
        f.refModelId ? { ...f, refModelId: remapModel(f.refModelId) } : f,
      ),
    }));

    // Environment id remapping (envId references on scenarios / suites must be rewritten as well)
    const envIdMap = new Map<string, string>();
    for (const e of data.environments ?? []) envIdMap.set(e.id, uid("env"));
    const remapEnv = (eid?: string | null) =>
      eid && envIdMap.has(eid) ? (envIdMap.get(eid) ?? eid) : (eid ?? null);

    const environments: Environment[] = (data.environments ?? []).map((e) => ({
      ...e,
      id: envIdMap.get(e.id) ?? e.id,
      workspaceId: newId,
    }));

    // Scenario folder id remapping (scenario folderId / folder parentId / suite members are rewritten too)
    const folderIdMap = new Map<string, string>();
    for (const f of data.scenarioFolders ?? [])
      folderIdMap.set(f.id, uid("sf"));
    const scIdMap = new Map<string, string>();
    for (const sc of data.scenarios ?? []) scIdMap.set(sc.id, uid("sc"));

    const scenarios: Scenario[] = (data.scenarios ?? []).map((sc) => ({
      ...sc,
      id: scIdMap.get(sc.id) ?? sc.id,
      workspaceId: newId,
      folderId: sc.folderId ? (folderIdMap.get(sc.folderId) ?? null) : null,
      envId: remapEnv(sc.envId),
      steps: cloneSteps(sc.steps, remapReq),
    }));

    const scenarioFolders: ScenarioFolder[] = (data.scenarioFolders ?? []).map(
      (f) => ({
        ...f,
        id: folderIdMap.get(f.id) ?? f.id,
        workspaceId: newId,
        parentId: f.parentId ? (folderIdMap.get(f.parentId) ?? null) : null,
      }),
    );

    const scenarioDataSets: ScenarioDataSet[] = (
      data.scenarioDataSets ?? []
    ).map((d) => ({ ...d, id: uid("ds"), workspaceId: newId }));

    const scenarioSuites: TestSuite[] = (data.scenarioSuites ?? []).map(
      (su) => ({
        ...su,
        id: uid("su"),
        workspaceId: newId,
        envId: remapEnv(su.envId),
        memberIds: su.memberIds.map((m) => scIdMap.get(m) ?? m),
      }),
    );

    const ws: Workspace = {
      ...src,
      id: newId,
      name: `${src.name} ${t("common.copySuffix", "(copy)")}`,
      description: src.description,
      createdAt: Date.now(),
      sortIndex: s.workspaces.length,
    };

    set((st) => ({
      workspaces: [...st.workspaces, ws],
      workspaceData: {
        ...st.workspaceData,
        [newId]: {
          collections,
          requests,
          models,
          environments,
          // Library items keep their original ids, so references (`libraryId`) inside the copy still resolve without remapping
          actionTemplates: data.actionTemplates ?? [],
          scenarios,
          scenarioFolders,
          scenarioDataSets,
          scenarioSuites,
          history: [],
          activeEnvId: null,
          globalVariables: {},
          globalSecrets: {},
        },
      },
    }));
  },
});
