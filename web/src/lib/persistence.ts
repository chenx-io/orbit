// Local snapshot persistence service (full-database JSON).
//
// Design notes:
// - The sync unit is a Snapshot (one JSON document carrying all user data): Tauri writes app_data_dir/orbit_data.json,
//   the browser writes localStorage["orbit.snapshot"]; both structures are identical, so future remote sync is just upload/download of this document.
// - Save points: a 500ms debounce on store changes plus a manual saveNow() (Ctrl+S / the Data Management panel).
// - Only "user data + a few UI preferences" are persisted; runtime state (response/loading/load tests etc.) is never written.
// - History stores request metadata only (no response bodies), keeping localStorage within its quota.
import type {
  ActionTemplate,
  ApiRequest,
  Collection,
  CollectionItem,
  DataModel,
  DataSource,
  Environment,
  HistoryEntry,
  Locale,
  MockInterface,
  PluginDescriptor,
  Scenario,
  ScenarioDataSet,
  ScenarioFolder,
  ScenarioStep,
  TestSuite,
  Workspace,
  WorkspaceData,
} from "@/data/types";
import type { AiPrefs } from "@/data/aiTypes";
import { DEFAULT_AI_PREFS } from "@/data/aiTypes";
import {
  defaultCollection,
  seedEnvironments,
  setSeedLocale,
  seedWorkspaces,
} from "@/data/seed";
import {
  actionsFromPersisted,
  actionsToPersisted,
  fromWireAction,
  type WireAction,
} from "@/lib/requestActions";
import {
  clearSnapshotJson,
  dataClear,
  dataLoadSnapshot,
  dataSaveSnapshot,
  getMockRules,
  isTauri,
  restoreMockRules,
  SNAPSHOT_LS_KEY,
} from "@/lib/bridge";
import type { useAppStore } from "@/store/useStore";

/** Snapshot structure version:
 *  v1: no workspace layer (activeEnvId / globalVariables / globalSecrets are flat and entities have no owner)
 *  v2: a workspace layer (workspaces / activeWorkspaceId / per-ws maps, with entities carrying a workspaceId).
 *  Older data is migrated automatically on load (normalizePersistedData, aligned with the backend ensure_migrated). */
export const SNAPSHOT_VERSION = 2;
const DEBOUNCE_MS = 300;
/** Timeout and cache TTL for fetching mock rules while saving (avoids repeated IPC on frequent saves and prevents hangs) */
const MOCK_FETCH_TIMEOUT_MS = 1500;
const MOCK_FETCH_TTL_MS = 10_000;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

type StoreState = ReturnType<typeof useAppStore.getState>;

/** Legacy snapshot compat: executionTarget used to be { mode, agentId } and is now { mode, agentIds } (null = all agents) */
function normalizeExecutionTarget(t: unknown): {
  mode: "local" | "agent";
  agentIds: string[] | null;
} {
  const v = t as
    | {
        mode?: string;
        agentId?: string | null;
        agentIds?: string[] | null;
      }
    | null
    | undefined;
  if (!v || v.mode !== "agent") return { mode: "local", agentIds: null };
  const ids = Array.isArray(v.agentIds)
    ? v.agentIds
    : v.agentId
      ? [v.agentId]
      : null;
  return { mode: "agent", agentIds: ids };
}

// ─── Snapshot structure ─────────────────────────────────────────

/** Persisted history shape: request metadata only, no response body */
export interface PersistedHistoryEntry {
  id: string;
  requestId: string;
  /** Owning workspace (v2 snapshot field; stamped on save) */
  workspaceId?: string;
  name: string;
  method: string;
  url: string;
  status: number | null;
  duration: number | null;
  size: number | null;
  timestamp: number;
}

export interface PersistedData {
  // ── v2: workspace layer (project boundary) ──
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  /** Active environment remembered per workspace */
  activeEnvByWorkspace: Record<string, string | null>;
  /** Global variables held independently per workspace */
  globalVariablesByWorkspace: Record<string, Record<string, string>>;
  /** Global secrets held independently per workspace */
  globalSecretsByWorkspace: Record<string, Record<string, string>>;

  collections: Collection[];
  requests: Record<string, ApiRequest>;
  models: DataModel[];
  environments: Environment[];
  /** Script library items (reusable action templates; workspace-level entities, defaulting to an empty array) */
  actionTemplates?: ActionTemplate[];
  scenarios: Scenario[];
  /** Scenario folder tree (in the same partition as scenarios and migrating with the workspace) */
  scenarioFolders: ScenarioFolder[];
  /** CSV test data sets */
  scenarioDataSets: ScenarioDataSet[];
  /** Test suites */
  scenarioSuites: TestSuite[];
  plugins: PluginDescriptor[];
  history: PersistedHistoryEntry[];
  mockRules: MockInterface[];
  /** Data source connection configs (global entities reused across workspaces; used by DB/Redis assertions) */
  dataSources: DataSource[];
  locale: Locale;
  theme: "light" | "dark" | "system";
  ui: { sidebarCollapsed: boolean };
  /** How requests are executed (local / agent), defaulting to local; agentIds=null means all available agents */
  executionTarget: { mode: "local" | "agent"; agentIds: string[] | null };
  /** AI assistant preferences (app-level, shared across workspaces; **no keys** — keys live in <app_data_dir>/ai/credentials.yaml) */
  ai: AiPrefs;
}

export interface Snapshot {
  schemaVersion: number;
  savedAt: number;
  source: "tauri" | "web";
  /** Remote sync config placeholder (future work); only the field is kept for now */
  sync: { remoteUrl: string | null; lastSyncedAt: number | null };
  data: PersistedData;
}

// ─── Workspace partition helpers (the store holds only the active workspace; snapshots are partitioned per workspace) ───

/** Empty workspace data (for a new workspace / a partition fallback with no data) */
export function emptyWorkspaceData(): WorkspaceData {
  return {
    collections: [],
    requests: {},
    models: [],
    environments: [],
    actionTemplates: [],
    scenarios: [],
    scenarioFolders: [],
    scenarioDataSets: [],
    scenarioSuites: [],
    history: [],
    activeEnvId: null,
    globalVariables: {},
    globalSecrets: {},
  };
}

/** Collect the requestIds referenced by collection trees / scenario steps (determines request ownership per workspace; used for request counts) */
export function collectRefRequestIds(
  collections: Collection[],
  scenarios: Scenario[],
): Set<string> {
  const refIds = new Set<string>();
  const walkItems = (items: CollectionItem[]): void => {
    for (const it of items) {
      if (it.type === "folder") walkItems(it.items);
      else if (it.type === "request" || it.type === "grpc-rpc") {
        if (it.requestId) refIds.add(it.requestId);
      }
    }
  };
  for (const c of collections) walkItems(c.items);
  const walkSteps = (steps: ScenarioStep[]): void => {
    for (const st of steps) {
      if (st.type === "request" && st.requestId) refIds.add(st.requestId);
      if ("children" in st && Array.isArray(st.children))
        walkSteps(st.children);
      if ("elseChildren" in st && Array.isArray(st.elseChildren)) {
        walkSteps(st.elseChildren);
      }
    }
  };
  for (const s of scenarios) walkSteps(s.steps);
  return refIds;
}

/** Capture the domain data of the store's active workspace (for switching workspaces / the snapshot before saving).
 *  Requests are filtered by collection tree / scenario references (this workspace's subset of the global map). */
export function captureWorkspaceData(state: StoreState): WorkspaceData {
  const refs = collectRefRequestIds(state.collections, state.scenarios);
  const requests: Record<string, ApiRequest> = {};
  for (const [id, r] of Object.entries(state.requests)) {
    if (refs.has(id)) requests[id] = r;
  }
  return {
    collections: state.collections,
    requests,
    models: state.models,
    environments: state.environments,
    actionTemplates: state.actionTemplates,
    scenarios: state.scenarios,
    scenarioFolders: state.scenarioFolders,
    scenarioDataSets: state.scenarioDataSets,
    scenarioSuites: state.scenarioSuites,
    history: state.history,
    activeEnvId: state.activeEnvId,
    globalVariables: state.globalVariables,
    globalSecrets: state.globalSecrets,
  };
}

/** Spread a workspace's partition data back into the store (called when switching workspaces); an empty collection list falls back to the default space (stable id avoids duplicates) */
export function spreadWorkspaceData(
  d: WorkspaceData,
  wsId = "ws-default",
): Partial<StoreState> {
  return {
    collections:
      d.collections.length > 0
        ? d.collections
        : [
            {
              ...defaultCollection(),
              id: `col-${wsId}`,
              items: [],
            },
          ],
    models: d.models,
    environments: d.environments,
    actionTemplates: d.actionTemplates ?? [],
    scenarios: d.scenarios,
    scenarioFolders: d.scenarioFolders ?? [],
    scenarioDataSets: d.scenarioDataSets ?? [],
    scenarioSuites: d.scenarioSuites ?? [],
    history: d.history,
    // activeEnvId fallback: fall back to the first environment when the referenced one is gone
    activeEnvId: d.environments.some((e) => e.id === d.activeEnvId)
      ? d.activeEnvId
      : (d.environments[0]?.id ?? null),
    globalVariables: d.globalVariables,
    globalSecrets: d.globalSecrets,
  };
}

/** Filter snapshot entities by workspace and read the per-ws state to build that workspace's partition data.
 *  Requests form a global map: collect the requests referenced by this workspace's collection trees / scenario steps. */
function partitionWorkspaceData(
  data: PersistedData,
  wsId: string,
): WorkspaceData {
  const ws = (id?: string) => id ?? "ws-default";
  const collections = data.collections.filter(
    (c) => ws(c.workspaceId) === wsId,
  );
  const scenarios = data.scenarios.filter((s) => ws(s.workspaceId) === wsId);
  const refIds = collectRefRequestIds(collections, scenarios);
  const requests: Record<string, ApiRequest> = {};
  for (const [id, r] of Object.entries(data.requests)) {
    if (refIds.has(id)) requests[id] = r;
  }
  return {
    collections,
    requests,
    models: data.models.filter((m) => ws(m.workspaceId) === wsId),
    environments: data.environments.filter((e) => ws(e.workspaceId) === wsId),
    actionTemplates: (data.actionTemplates ?? []).filter(
      (t) => ws(t.workspaceId) === wsId,
    ),
    scenarios,
    scenarioFolders: data.scenarioFolders.filter(
      (f) => ws(f.workspaceId) === wsId,
    ),
    scenarioDataSets: data.scenarioDataSets.filter(
      (d) => ws(d.workspaceId) === wsId,
    ),
    scenarioSuites: data.scenarioSuites.filter(
      (s) => ws(s.workspaceId) === wsId,
    ),
    history: data.history
      .filter((h) => ws(h.workspaceId) === wsId)
      .map((h) => ({ ...h }) as HistoryEntry),
    activeEnvId: data.activeEnvByWorkspace[wsId] ?? null,
    globalVariables: data.globalVariablesByWorkspace[wsId] ?? {},
    globalSecrets: data.globalSecretsByWorkspace[wsId] ?? {},
  };
}

/** Normalize snapshot data: v1 (no workspace layer) → v2 (workspace layer).
 *  Aligned with the backend PersistedData::ensure_migrated: legacy data belongs to the default workspace. */
function normalizePersistedData(data: PersistedData): PersistedData {
  // Be lenient with the v1 shape: the flat activeEnvId / globalVariables / globalSecrets fields may be missing
  const raw = data as PersistedData & {
    activeEnvId?: string | null;
    globalVariables?: Record<string, string>;
    globalSecrets?: Record<string, string>;
  };
  const workspaces =
    raw.workspaces?.length > 0 ? raw.workspaces : seedWorkspaces(raw.locale);
  const wsId = workspaces[0].id;
  return {
    workspaces,
    activeWorkspaceId: raw.activeWorkspaceId ?? wsId,
    activeEnvByWorkspace: raw.activeEnvByWorkspace ?? {
      [wsId]: raw.activeEnvId ?? null,
    },
    globalVariablesByWorkspace: raw.globalVariablesByWorkspace ?? {
      [wsId]: raw.globalVariables ?? {},
    },
    globalSecretsByWorkspace: raw.globalSecretsByWorkspace ?? {
      [wsId]: raw.globalSecrets ?? {},
    },
    collections: raw.collections ?? [],
    // Restore action fields from the snapshot wire format to the frontend shape (re-adding ids): downstream (partitions / store) uses the frontend shape consistently
    requests: mapPersistedRequests(raw.requests ?? {}),
    models: raw.models ?? [],
    environments: raw.environments ?? [],
    actionTemplates: fromPersistedTemplates(raw.actionTemplates),
    scenarios: raw.scenarios ?? [],
    scenarioFolders: raw.scenarioFolders ?? [],
    scenarioDataSets: raw.scenarioDataSets ?? [],
    scenarioSuites: raw.scenarioSuites ?? [],
    plugins: raw.plugins ?? [],
    history: raw.history ?? [],
    mockRules: raw.mockRules ?? [],
    dataSources: raw.dataSources ?? [],
    locale: raw.locale ?? "zh-CN",
    theme: raw.theme ?? "system",
    ui: raw.ui ?? { sidebarCollapsed: false },
    executionTarget: normalizeExecutionTarget(raw.executionTarget as never),
    // Legacy snapshots lack the ai field: merge it with the new fields so future preference additions also get defaults
    ai: { ...DEFAULT_AI_PREFS, ...((raw.ai ?? {}) as Partial<AiPrefs>) },
  };
}

// ─── Storage adapters (an interface reserved for remote sync)──────────────────

export interface PersistenceAdapter {
  readonly kind: "local" | "remote";
  load(): Promise<Snapshot | null>;
  /** baseSavedAt omitted = force overwrite (first launch / clearing data); a value = the optimistic-lock baseline (rejected when older than the local copy) */
  save(snapshot: Snapshot, baseSavedAt?: number | null): Promise<void>;
  clear(): Promise<void>;
}

/** Local adapter: Tauri → the backend DataService (authoritative file), browser → localStorage.
 *  The Tauri channel uses data_load/data_save (the Rust data layer);
 *  snapshots use structure v2 (workspace layer), and legacy v1 data is migrated automatically on load. */
function getLocalAdapter(): PersistenceAdapter {
  return {
    kind: "local",
    load: async () => {
      const json = await dataLoadSnapshot();
      if (!json) return null;
      try {
        return JSON.parse(json) as Snapshot;
      } catch (e) {
        console.warn("[persist] failed to parse the snapshot JSON; ignored", e);
        // Corrupted entries are cleared outright in the browser (the Rust layer archives them as .bak on Tauri)
        try {
          await clearSnapshotJson();
        } catch {
          /* ignore */
        }
        return null;
      }
    },
    save: async (snapshot, baseSavedAt) => {
      await dataSaveSnapshot(JSON.stringify(snapshot), baseSavedAt);
    },
    clear: () => dataClear(),
  };
}

// ─── Serialization / deserialization ─────────────────────────────────

/**
 * Request object → snapshot shape: action fields become the **wire format** (`type` tag + snake_case), everything else as-is.
 *
 * The backend `orbit_data` deserializes snapshots with a strongly-typed `RequestAction`, which cannot read the frontend (`kind`) shape,
 * so a missing conversion silently drops actions in save / load round-trips (script library references included).
 */
function toPersistedRequest(r: ApiRequest): ApiRequest {
  return {
    ...r,
    preResolveActions: actionsToPersisted(r.preResolveActions ?? []),
    preActions: actionsToPersisted(r.preActions ?? []),
    postActions: actionsToPersisted(r.postActions ?? []),
  } as unknown as ApiRequest;
}

/** Request object: restore action fields from the snapshot shape to the frontend shape (re-adding stable ids) */
function fromPersistedRequest(r: ApiRequest): ApiRequest {
  const wire = (v: unknown) => v as WireAction[] | undefined;
  return {
    ...r,
    preResolveActions: actionsFromPersisted(wire(r.preResolveActions)),
    preActions: actionsFromPersisted(wire(r.preActions)),
    postActions: actionsFromPersisted(wire(r.postActions)),
  } as ApiRequest;
}

/** Request map from the snapshot → frontend shape (action fields restored) */
function mapPersistedRequests(
  map: Record<string, ApiRequest>,
): Record<string, ApiRequest> {
  const out: Record<string, ApiRequest> = {};
  for (const [id, r] of Object.entries(map)) out[id] = fromPersistedRequest(r);
  return out;
}

/** Library item: its inner action must be restored from the snapshot shape as well */
function fromPersistedTemplates(
  list: ActionTemplate[] | undefined,
): ActionTemplate[] {
  return (list ?? []).map((tpl) => ({
    ...tpl,
    action: fromWireAction(
      tpl.action as unknown as WireAction,
    ) as ActionTemplate["action"],
  }));
}

/** Library item: convert its inner action to the snapshot shape */
function toPersistedTemplates(
  list: ActionTemplate[] | undefined,
): ActionTemplate[] {
  return (list ?? []).map((tpl) => ({
    ...tpl,
    action: actionsToPersisted([
      tpl.action,
    ])![0] as unknown as ActionTemplate["action"],
  }));
}

function toPersistedHistory(h: HistoryEntry): PersistedHistoryEntry {
  return {
    id: h.id,
    requestId: h.requestId,
    name: h.name,
    method: h.method,
    url: h.url,
    status: h.status,
    duration: h.duration,
    size: h.size,
    timestamp: h.timestamp,
  };
}

/** Persisted fields are picked via an allowlist (so future transient fields cannot leak into the snapshot).
 *  The store holds only the active workspace's data and caches all partitions in workspaceData:
 *  first write back to the active partition, then concatenate the entities of [all partitions] into flat snapshot arrays (stamping workspaceId uniformly).
 *  The list page (activeWorkspaceId = null) serializes from the partitions too, losing no workspace data. */
export function buildPersisted(state: StoreState): PersistedData {
  const wsId = state.activeWorkspaceId;
  // Align the active workspace partition with the store (store fields hold the latest edits; other partitions keep their cache)
  const partitioned = wsId
    ? { ...state.workspaceData, [wsId]: captureWorkspaceData(state) }
    : state.workspaceData;
  const activeEnvByWorkspace: Record<string, string | null> = {};
  const globalVariablesByWorkspace: Record<string, Record<string, string>> = {};
  const globalSecretsByWorkspace: Record<string, Record<string, string>> = {};
  const collections: Collection[] = [];
  const models: DataModel[] = [];
  const environments: Environment[] = [];
  const actionTemplates: ActionTemplate[] = [];
  const scenarios: Scenario[] = [];
  const scenarioFolders: ScenarioFolder[] = [];
  const scenarioDataSets: ScenarioDataSet[] = [];
  const scenarioSuites: TestSuite[] = [];
  const history: PersistedHistoryEntry[] = [];
  const requests: Record<string, ApiRequest> = {};
  for (const [id, d] of Object.entries(partitioned)) {
    activeEnvByWorkspace[id] = d.activeEnvId;
    globalVariablesByWorkspace[id] = d.globalVariables;
    globalSecretsByWorkspace[id] = d.globalSecrets;
    for (const c of d.collections) collections.push({ ...c, workspaceId: id });
    for (const m of d.models) models.push({ ...m, workspaceId: id });
    for (const e of d.environments)
      environments.push({ ...e, workspaceId: id });
    for (const t of toPersistedTemplates(d.actionTemplates))
      actionTemplates.push({ ...t, workspaceId: id });
    for (const s of d.scenarios) scenarios.push({ ...s, workspaceId: id });
    for (const f of d.scenarioFolders ?? [])
      scenarioFolders.push({ ...f, workspaceId: id });
    for (const ds of d.scenarioDataSets ?? [])
      scenarioDataSets.push({ ...ds, workspaceId: id });
    for (const su of d.scenarioSuites ?? [])
      scenarioSuites.push({ ...su, workspaceId: id });
    for (const h of d.history) {
      history.push({ ...toPersistedHistory(h), workspaceId: id });
    }
    // Convert actions to the snapshot wire format: the backend's strongly-typed model only reads the `type` tag plus snake_case fields
    for (const [rid, r] of Object.entries(d.requests))
      requests[rid] = toPersistedRequest(r);
  }
  // Global requests not referenced by any partition (orphans) are kept, so nothing is lost
  for (const [rid, r] of Object.entries(state.requests)) {
    if (!requests[rid]) requests[rid] = toPersistedRequest(r);
  }
  return {
    workspaces:
      state.workspaces.length > 0
        ? state.workspaces
        : seedWorkspaces(state.locale),
    activeWorkspaceId: state.activeWorkspaceId,
    activeEnvByWorkspace,
    globalVariablesByWorkspace,
    globalSecretsByWorkspace,
    collections,
    requests,
    models,
    environments,
    actionTemplates,
    scenarios,
    scenarioFolders,
    scenarioDataSets,
    scenarioSuites,
    plugins: state.plugins,
    history,
    mockRules: [],
    dataSources: state.dataSources,
    locale: state.locale,
    theme: state.theme,
    ui: { sidebarCollapsed: state.ui.sidebarCollapsed },
    executionTarget: state.executionTarget,
    ai: state.aiPrefs,
  };
}

/** Hydrate the store from snapshot data (skipping runtime fields); returns whether it succeeded.
 *  v2: partitioned by workspace — the store holds only the active workspace's data and caches all partitions in workspaceData. */
export function applyPersisted(
  set: (partial: Partial<StoreState>) => void,
  data: PersistedData,
  currentUi?: StoreState["ui"],
): boolean {
  try {
    const normalized = normalizePersistedData(data);
    const wsId =
      normalized.activeWorkspaceId ??
      normalized.workspaces[0]?.id ??
      "ws-default";
    const workspaceData: Record<string, WorkspaceData> = {};
    for (const ws of normalized.workspaces) {
      workspaceData[ws.id] = partitionWorkspaceData(normalized, ws.id);
    }
    // Record the restored locale first so any seed data created afterwards uses matching names.
    setSeedLocale(normalized.locale);
    set({
      workspaces: normalized.workspaces,
      activeWorkspaceId: wsId,
      workspaceData,
      ...spreadWorkspaceData(workspaceData[wsId] ?? emptyWorkspaceData(), wsId),
      requests: normalized.requests,
      plugins: normalized.plugins,
      dataSources: normalized.dataSources,
      locale: normalized.locale,
      theme: normalized.theme,
      executionTarget: normalizeExecutionTarget(normalized.executionTarget),
      aiPrefs: normalized.ai,
      // Only the persisted sidebarCollapsed is restored; other transient fields such as dialog switches keep their current values
      ui: { ...(currentUi ?? ({} as StoreState["ui"])), ...normalized.ui },
    });
    return true;
  } catch (e) {
    console.warn("[persist] failed to restore the snapshot into the store", e);
    return false;
  }
}

// ─── Persistence service ────────────────────────────────────────

export interface PersistenceService {
  /** Save immediately (manual save / Ctrl+S) */
  saveNow: () => Promise<void>;
  /** Timestamp of the latest save; null when never saved */
  lastSavedAt: () => number | null;
  /** Export the current state as a snapshot JSON string (not persisted) */
  exportSnapshot: () => string;
  /**
   * Re-fetch the snapshot from the backend and hydrate the store.
   *
   * Purpose: after the AI assistant rewrites authoritative data on the Rust side (a write tool persisting), the frontend must reload,
   * otherwise the UI keeps stale data and the next autosave pushes it back, overwriting the AI's changes.
   */
  reloadFromBackend: () => Promise<boolean>;
  /** Import snapshot JSON: validate the version → hydrate the store → save immediately */
  importSnapshot: (json: string) => Promise<boolean>;
  /** Clear the local snapshot and return to the initial seed state (completed by reloading the page) */
  clearLocal: () => Promise<void>;
}

let activeService: PersistenceService | null = null;

/** Immediate-save entry point for the global shortcut (Ctrl+S) */
export function saveNow(): void {
  void activeService?.saveNow();
}

/** Get the current persistence service (used by UI such as the Data Management panel); null before initialization */
export function getPersistenceService(): PersistenceService | null {
  return activeService;
}

/** Init singleton guard: React StrictMode runs useEffect twice,
 *  so repeated calls reuse the first promise, avoiding duplicate subscriptions / listeners. */
let initPromise: Promise<PersistenceService> | null = null;

/** Initialize persistence on app startup: load → restore → subscribe to autosave. */
export function initPersistence(
  store: typeof useAppStore,
): Promise<PersistenceService> {
  if (!initPromise) {
    initPromise = doInitPersistence(store);
  }
  return initPromise;
}

async function doInitPersistence(
  store: typeof useAppStore,
): Promise<PersistenceService> {
  const adapter = getLocalAdapter();

  let snapshot: Snapshot | null = null;
  try {
    snapshot = await adapter.load();
  } catch (e) {
    console.warn("[persist] failed to load the local snapshot", e);
    snapshot = null;
  }

  let lastSaved: number | null = null;
  /** Cache of the most recently fetched mock rules (reused when the exit snapshot cannot fetch asynchronously) */
  let lastMockRules: MockInterface[] = [];
  /** Timestamp of the last successful mock-rule fetch (the cache is reused within the TTL to cut IPC) */
  let lastMockFetchAt = 0;
  if (snapshot && snapshot.schemaVersion <= SNAPSHOT_VERSION) {
    applyPersisted(
      (p) => store.setState(p),
      snapshot.data,
      store.getState().ui,
    );
    lastSaved = snapshot.savedAt;
    // Restore mock rules (a failure does not block the main flow)
    if (snapshot.data.mockRules.length > 0) {
      void restoreMockRules(snapshot.data.mockRules).catch((e) =>
        console.warn("[persist] failed to restore mock rules", e),
      );
    }
  } else if (snapshot) {
    console.warn(
      "[persist] incompatible snapshot version (expected",
      SNAPSHOT_VERSION,
      "actual",
      snapshot.schemaVersion,
      "), ignoring",
    );
  }

  const service: PersistenceService = {
    async saveNow() {
      try {
        const state = store.getState();
        const data = buildPersisted(state);
        // Fetch the latest mock rules into the snapshot: reuse the cache within the TTL plus a timeout fallback (so a hanging IPC cannot block saving/window close).
        // The snapshot must carry rules for every workspace (so switching workspaces loses nothing), hence a full fetch without workspaceId
        if (Date.now() - lastMockFetchAt > MOCK_FETCH_TTL_MS) {
          try {
            data.mockRules = await Promise.race([
              getMockRules(),
              sleep(MOCK_FETCH_TIMEOUT_MS).then(() => {
                throw new Error("getMockRules timeout");
              }),
            ]);
            lastMockRules = data.mockRules;
            lastMockFetchAt = Date.now();
          } catch (e) {
            console.warn(
              "[persist] mock-rule fetch failed/timed out (reusing the cache)",
              e,
            );
            data.mockRules = lastMockRules;
          }
        } else {
          data.mockRules = lastMockRules;
        }
        const snap: Snapshot = {
          schemaVersion: SNAPSHOT_VERSION,
          savedAt: Date.now(),
          source: isTauri() ? "tauri" : "web",
          sync: { remoteUrl: null, lastSyncedAt: null },
          data,
        };
        // Optimistic lock: send the last successful save timestamp; the backend rejects it when newer (protecting against multi-window / future multi-client overwrites)
        await adapter.save(snap, lastSaved);
        lastSaved = snap.savedAt;
      } catch (e) {
        console.warn(
          "[persist] failed to save the snapshot (possibly a conflict)",
          e,
        );
      }
    },
    lastSavedAt: () => lastSaved,
    /**
     * Reload the authoritative snapshot from the backend (called after an AI write tool persists).
     *
     * On success `lastSaved` is refreshed, so the following autosave is not rejected for carrying a stale optimistic-lock timestamp.
     */
    async reloadFromBackend() {
      try {
        const json = await dataLoadSnapshot();
        if (!json) return false;
        const snap = JSON.parse(json) as Snapshot;
        if (snap.schemaVersion > SNAPSHOT_VERSION) {
          console.warn(
            "[persist] reload failed: snapshot version is too new",
            snap.schemaVersion,
          );
          return false;
        }
        const ok = applyPersisted(
          (p) => store.setState(p),
          snap.data,
          store.getState().ui,
        );
        if (ok) {
          lastSaved = snap.savedAt;
          if (snap.data.mockRules.length > 0) {
            void restoreMockRules(snap.data.mockRules).catch(() => undefined);
            lastMockRules = snap.data.mockRules;
            lastMockFetchAt = Date.now();
          }
        }
        return ok;
      } catch (e) {
        console.warn(
          "[persist] failed to reload the snapshot from the backend",
          e,
        );
        return false;
      }
    },
    exportSnapshot: () => {
      const state = store.getState();
      const data = buildPersisted(state);
      return JSON.stringify(
        {
          schemaVersion: SNAPSHOT_VERSION,
          savedAt: Date.now(),
          source: isTauri() ? "tauri" : "web",
          sync: { remoteUrl: null, lastSyncedAt: null },
          data,
        } satisfies Snapshot,
        null,
        2,
      );
    },
    async importSnapshot(json) {
      let snap: Snapshot;
      try {
        snap = JSON.parse(json) as Snapshot;
      } catch (e) {
        console.warn("[persist] failed to parse the imported snapshot", e);
        return false;
      }
      if (snap.schemaVersion > SNAPSHOT_VERSION) {
        console.warn(
          "[persist] incompatible imported snapshot version (expected",
          SNAPSHOT_VERSION,
          "actual",
          snap.schemaVersion,
          "）",
        );
        return false;
      }
      const ok = applyPersisted(
        (p) => store.setState(p),
        snap.data,
        store.getState().ui,
      );
      if (ok) {
        if (snap.data.mockRules.length > 0) {
          void restoreMockRules(snap.data.mockRules).catch(() => undefined);
        }
        await service.saveNow();
      }
      return ok;
    },
    /**
     * Clear data: return to the initial state (default space + seed environments).
     * Strategy: write a "clean initial snapshot" over the existing data (not depending on the delete succeeding),
     * reset the in-memory store directly as well (so the UI reflects it immediately without a reload), and finally force a reload as a fallback.
     */
    async clearLocal() {
      const state = store.getState();
      const fresh: Snapshot = {
        schemaVersion: SNAPSHOT_VERSION,
        savedAt: Date.now(),
        source: isTauri() ? "tauri" : "web",
        sync: { remoteUrl: null, lastSyncedAt: null },
        data: {
          workspaces: seedWorkspaces(state.locale),
          activeWorkspaceId: "ws-default",
          activeEnvByWorkspace: { "ws-default": "env-dev" },
          globalVariablesByWorkspace: {},
          globalSecretsByWorkspace: {},
          collections: [defaultCollection(state.locale)],
          requests: {},
          models: [],
          environments: seedEnvironments(state.locale),
          scenarios: [],
          scenarioFolders: [],
          scenarioDataSets: [],
          scenarioSuites: [],
          plugins: [],
          history: [],
          mockRules: [],
          dataSources: [],
          locale: state.locale,
          theme: state.theme,
          ui: { sidebarCollapsed: state.ui.sidebarCollapsed },
          executionTarget: state.executionTarget,
          ai: state.aiPrefs,
        },
      };
      // 1) Overwrite the local snapshot (localStorage + Tauri file); a failure does not block the rest
      try {
        await adapter.save(fresh);
      } catch (e) {
        console.warn(
          "[persist] clear data: failed to write the initial snapshot",
          e,
        );
      }
      // 2) Reset the in-memory store directly so the UI returns to the initial state at once (no stale data even if the reload does not take effect)
      try {
        store.setState({
          workspaces: seedWorkspaces(state.locale),
          activeWorkspaceId: "ws-default",
          workspaceData: { "ws-default": emptyWorkspaceData() },
          collections: [defaultCollection(state.locale)],
          requests: {},
          drafts: {},
          dirty: {},
          models: [],
          environments: seedEnvironments(state.locale),
          activeEnvId: "env-dev",
          globalVariables: {},
          globalSecrets: {},
          history: [],
          tabs: [],
          activeTabId: null,
          scenarios: [],
          activeScenarioId: null,
          scenarioFolders: [],
          scenarioDataSets: [],
          scenarioSuites: [],
          activeSuiteId: null,
          plugins: [],
          dataSources: [],
          response: null,
          responseError: null,
          assertionResults: [],
        });
      } catch (e) {
        console.warn(
          "[persist] clear data: failed to reset the in-memory state",
          e,
        );
      }
      // 3) Force a reload (replace navigation is more reliable than location.reload() in the Tauri webview)
      try {
        window.location.replace(window.location.href);
      } catch (e) {
        console.warn("[persist] clear data: failed to reload the page", e);
      }
    },
  };

  activeService = service;

  // Autosave: subscribe to store changes and persist after the debounce.
  // Note that subscribe is registered after hydrate (applyPersisted), so restoring never triggers a save.
  let timer: ReturnType<typeof setTimeout> | undefined;
  const schedule = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void service.saveNow(), DEBOUNCE_MS);
  };
  const unsubscribe = store.subscribe(schedule);

  // Exit fallback: ensures changes not yet persisted are not lost.
  // - Browser: beforeunload synchronously writes localStorage (setItem is synchronous), so reload/close loses nothing;
  // - Tauri: intercept onCloseRequested → save asynchronously first → then destroy the window.
  const buildSnapshotForExit = (): Snapshot | null => {
    try {
      const state = store.getState();
      const data = buildPersisted(state);
      data.mockRules = lastMockRules; // no async fetch is possible on exit, so reuse the cache
      return {
        schemaVersion: SNAPSHOT_VERSION,
        savedAt: Date.now(),
        source: isTauri() ? "tauri" : "web",
        sync: { remoteUrl: null, lastSyncedAt: null },
        data,
      };
    } catch (e) {
      console.warn("[persist] failed to build the exit snapshot", e);
      return null;
    }
  };
  window.addEventListener("beforeunload", () => {
    if (timer) clearTimeout(timer);
    unsubscribe();
    // Exit fallback: synchronously write localStorage (persisted in both the Tauri webview and the browser),
    // catching the last unpersisted changes inside the debounce window. Window close is never intercepted, so it always closes cleanly.
    const snap = buildSnapshotForExit();
    if (snap) {
      try {
        localStorage.setItem(SNAPSHOT_LS_KEY, JSON.stringify(snap));
      } catch (e) {
        console.warn("[persist] synchronous save before exit failed", e);
      }
    }
  });

  // First launch: persist a seed-based snapshot immediately when none is available
  if (!snapshot || snapshot.schemaVersion !== SNAPSHOT_VERSION) {
    void service.saveNow();
  }

  return service;
}
