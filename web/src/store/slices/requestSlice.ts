// Request (HttpRequest) domain: request CRUD, drafts, tabs and bulk import.
import type { StateCreator } from "zustand";
import type {
  ApiRequest,
  AuthConfig,
  CollectionItem,
  HttpRequest,
  ResponseDef,
  Tab,
} from "@/data/types";
import { createRequest, isHttpRequest, requestProtocol } from "@/data/types";
import { seedRequests, uid } from "@/data/seed";
import { t, tFormat } from "@/lib/localeDict";
import { saveNow } from "@/lib/persistence";
import {
  addToFolder,
  findFolderNode,
  findParentFolderId,
  findAndRemove,
  replaceFolderItems,
} from "../utils/tree";
import type { AppState } from "../types";

/** Minimal description of an imported endpoint (matches ImportDialog's parse result) */
export interface ImportEndpoint {
  name: string;
  method: string;
  url: string;
  headers: Record<string, string>;
  queryParams?: Record<string, string>;
  body: string;
  contentType: string;
  group?: string;
  summary?: string;
  modelRef?: string;
  /** Auth type: none / bearer / basic / apikey / oauth2 (from OpenAPI securitySchemes) */
  authType?: string;
  /** Request parameter name for apiKey */
  authKeyName?: string;
  /** Where apiKey is injected: header / query */
  authAddTo?: string;
  /** Response example */
  responses?: { status: number; name: string; body: string; schema?: any }[];
  /** Pre-request script (Postman event prerequest / OpenAPI x- extension) */
  prereqScript?: string;
  /** Post-response script (Postman event test / OpenAPI x- extension) */
  postreqScript?: string;
}

export interface RequestSlice {
  requests: Record<string, ApiRequest>;
  drafts: Record<string, ApiRequest>;
  dirty: Record<string, boolean>;
  tabs: Tab[];
  activeTabId: string | null;
  requestTab:
    | "params"
    | "headers"
    | "body"
    | "auth"
    | "prescript"
    | "postscript"
    | "assertions"
    | "examples"
    | "mock";

  addRequest: (partial?: Partial<HttpRequest>, protocol?: string) => string;
  updateRequest: (id: string, updates: Partial<ApiRequest>) => void;
  updateDraft: (
    id: string,
    updates: Partial<ApiRequest>,
    opts?: { silent?: boolean },
  ) => void;
  saveDraft: (id: string) => void;
  saveDraftAs: (id: string, name?: string) => void;
  restoreDraft: (id: string) => void;
  renameRequest: (id: string, name: string) => void;
  removeRequest: (id: string) => void;
  duplicateRequest: (id: string) => void;
  importRequests: (
    endpoints: ImportEndpoint[],
    targetCollectionId?: string,
    targetFolderId?: string | null,
    modelIds?: Record<string, string>,
  ) => void;
  openTab: (requestId: string) => void;
  /** Open a tab for a grpc package/service node (no request object; standalone tab) */
  openGrpcTab: (grpcNode: NonNullable<Tab["grpcNode"]>) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  closeAllTabs: () => void;
  closeOtherTabs: (tabId: string) => void;
  closeTabsLeft: (tabId: string) => void;
  closeTabsRight: (tabId: string) => void;
  closeCleanTabs: () => void;
  setRequestTab: (t: RequestSlice["requestTab"]) => void;
}

/** Infer bodyMode from content_type; no body (empty content_type) → none */
function inferBodyMode(ct: string): HttpRequest["bodyMode"] {
  if (!ct) return "none";
  if (ct.includes("x-www-form-urlencoded")) return "x-www-form-urlencoded";
  if (ct.includes("multipart")) return "form-data";
  if (ct.includes("text/plain")) return "raw";
  if (ct.includes("xml")) return "xml";
  if (ct.includes("json")) return "json";
  return "raw";
}

/** Build an AuthConfig from the imported auth scheme */
function buildAuth(ep: ImportEndpoint): AuthConfig {
  switch (ep.authType) {
    // Bearer: the token defaults to the {{token}} template reference (the user defines the token value in global/environment variables)
    case "bearer":
      return { type: "bearer", token: "{{token}}" };
    case "basic":
      return { type: "basic", username: "", password: "" };
    case "apikey":
      return {
        type: "apikey",
        key: ep.authKeyName || "",
        value: "",
        addTo: ep.authAddTo === "query" ? "query" : "header",
      };
    case "oauth2":
      return { type: "oauth2" };
    default:
      return { type: "none" };
  }
}

/** Imported response examples → ResponseDef[] */
function buildResponses(
  eps:
    { status: number; name: string; body: string; schema?: any }[] | undefined,
): ResponseDef[] {
  return (eps ?? []).map((r) => ({
    id: uid("resp"),
    name: r.name || String(r.status),
    status: r.status,
    contentType: "application/json",
    body: r.body,
    schema: r.schema,
  }));
}

export const createRequestSlice: StateCreator<
  AppState,
  [],
  [],
  RequestSlice
> = (set, get) => ({
  requests: Object.fromEntries(seedRequests.map((r) => [r.id, r])),
  drafts: {},
  dirty: {},
  // No initial tabs (nothing openable in the seed); avoids leftover "deleted" tabs pointing at nonexistent requests
  tabs: [],
  activeTabId: null,
  requestTab: "params",

  addRequest: (partial, protocol) => {
    const id = uid("req");
    const req: ApiRequest =
      protocol && protocol !== "http"
        ? { ...createRequest(protocol, partial?.name ?? t("request.new")), id }
        : {
            id,
            name: partial?.name ?? t("request.new"),
            protocol: "http",
            method: partial?.method ?? "GET",
            url: partial?.url ?? "",
            headers: partial?.headers ?? [],
            queryParams: partial?.queryParams ?? [],
            pathParams: partial?.pathParams ?? [],
            body: partial?.body ?? "",
            bodyMode: partial?.bodyMode ?? "json",
            bodyByMode: partial?.bodyByMode ?? {},
            contentType: partial?.contentType ?? "application/json",
            formParams: partial?.formParams ?? [],
            binaryFile: partial?.binaryFile ?? null,
            auth: partial?.auth ?? { type: "none" },
            cookies: partial?.cookies ?? [],
            responses: partial?.responses ?? [],
            modelId: partial?.modelId ?? null,
            // Pre/post scripts (written when importing Postman events / OpenAPI x- extensions)
            prereqScript: partial?.prereqScript ?? "",
            postreqScript: partial?.postreqScript ?? "",
            // Pre-actions form one ordered list (including the built-in interpolation node); post-actions are a separate list
            preActions: partial?.preActions ?? [],
            postActions: partial?.postActions ?? [],
          };
    set((s) => ({ requests: { ...s.requests, [id]: req } }));
    void saveNow();
    return id;
  },
  updateRequest: (id, updates) => {
    set((s) => {
      const existing = s.requests[id];
      if (!existing) return s;
      const next = { ...existing, ...updates } as ApiRequest;
      // Writing the single pre-action list clears the previous compat fields: their content is already in the list and would be merged twice
      if (updates.preActions !== undefined) next.preResolveActions = [];
      return {
        requests: { ...s.requests, [id]: next },
        tabs: s.tabs.map((t) =>
          t.requestId === id ? { ...t, dirty: true } : t,
        ),
      };
    });
    void saveNow();
  },
  updateDraft: (id, updates, opts) =>
    set((s) => {
      const base = s.drafts[id] ?? s.requests[id];
      if (!base) return s;
      const next = { ...base, ...updates } as ApiRequest;
      // Same as above: clear the compat fields when writing the single list (avoiding duplicate merging on the Rust side)
      if (updates.preActions !== undefined) next.preResolveActions = [];
      const changed = Object.keys(updates).some((k) => {
        const key = k as keyof typeof next;
        return JSON.stringify(updates[key]) !== JSON.stringify(base[key]);
      });
      if (!changed) return s; // nothing changed at all, do nothing
      const silent = opts?.silent === true;
      // Silent completion (system behavior such as automatic default headers / path params, not a user edit):
      // saved requests update the in-memory value directly, without entering a draft or marking dirty
      if (silent && !s.drafts[id]) {
        return { requests: { ...s.requests, [id]: next } };
      }
      return {
        drafts: { ...s.drafts, [id]: next },
        dirty: silent ? s.dirty : { ...s.dirty, [id]: true },
        tabs: s.tabs.map((t) =>
          t.requestId === id ? { ...t, dirty: silent ? t.dirty : true } : t,
        ),
      };
    }),
  saveDraft: (id) => {
    set((s) => {
      const draft = s.drafts[id];
      if (!draft) return s;
      const drafts = { ...s.drafts };
      delete drafts[id];
      const dirty = { ...s.dirty };
      delete dirty[id];
      return {
        requests: { ...s.requests, [id]: draft },
        drafts,
        dirty,
        tabs: s.tabs.map((t) =>
          t.requestId === id ? { ...t, dirty: false } : t,
        ),
      };
    });
    // "Save request" = persist immediately, without relying on the debounce
    void saveNow();
  },
  saveDraftAs: (id, name) => {
    const s = get();
    const base = s.drafts[id] ?? s.requests[id];
    if (!base) return;
    const newId = get().addRequest(
      {
        ...base,
        name: name ?? tFormat("request.copyName", base.name),
        id: undefined,
      } as Partial<HttpRequest>,
      requestProtocol(base),
    );
    set((st) => {
      let targetCol: string | null = null;
      let folderId: string | null = null;
      for (const c of st.collections) {
        const found = findParentFolderId(c.items, id);
        if (found !== undefined) {
          targetCol = c.id;
          folderId = found;
          break;
        }
      }
      if (!targetCol) return st;
      const node: CollectionItem = {
        type: "request",
        id: uid("ci"),
        requestId: newId,
      };
      const collections = st.collections.map((c) =>
        c.id === targetCol
          ? {
              ...c,
              items: folderId
                ? addToFolder(c.items, folderId, node)
                : [...c.items, node],
            }
          : c,
      );
      return { collections };
    });
    get().openTab(newId);
    void saveNow();
  },
  restoreDraft: (id) => {
    set((s) => {
      const drafts = { ...s.drafts };
      delete drafts[id];
      const dirty = { ...s.dirty };
      delete dirty[id];
      return {
        drafts,
        dirty,
        tabs: s.tabs.map((t) =>
          t.requestId === id ? { ...t, dirty: false } : t,
        ),
      };
    });
  },
  renameRequest: (id, name) => {
    set((s) => ({
      requests: s.requests[id]
        ? { ...s.requests, [id]: { ...s.requests[id], name } }
        : s.requests,
      drafts: s.drafts[id]
        ? { ...s.drafts, [id]: { ...s.drafts[id], name } }
        : s.drafts,
    }));
    void saveNow();
  },
  removeRequest: (id) => {
    set((s) => {
      const { [id]: _drop, ...rest } = s.requests;
      const drafts = { ...s.drafts };
      delete drafts[id];
      const dirty = { ...s.dirty };
      delete dirty[id];
      const collections = s.collections.map((c) => ({
        ...c,
        items: findAndRemove(
          c.items.filter((it) =>
            it.type === "request" ? it.requestId !== id : true,
          ),
          "",
        ),
      }));
      const tabs = s.tabs.filter((t) => t.requestId !== id);
      const activeTabId =
        s.activeTabId && tabs.find((t) => t.id === s.activeTabId)
          ? s.activeTabId
          : (tabs[0]?.id ?? null);
      return { requests: rest, drafts, dirty, collections, tabs, activeTabId };
    });
    void saveNow();
  },
  duplicateRequest: (id) => {
    const orig = get().requests[id];
    if (!orig) return;
    const newId = get().addRequest(
      {
        ...orig,
        name: tFormat("request.copyName", orig.name),
        id: undefined,
      } as Partial<HttpRequest>,
      requestProtocol(orig),
    );
    get().openTab(newId);
  },
  importRequests: (endpoints, targetCollectionId, targetFolderId, modelIds) => {
    const colId = targetCollectionId || get().collections[0]?.id;
    if (!colId || endpoints.length === 0) return;

    // Deduplication is scoped to the [target collection]: same-named endpoints in different collections are distinct and created independently.
    // This avoids: (1) importing the same file into the same collection producing duplicates; (2) importing into different collections
    // wrongly overwriting another collection's requests, which would make several collection nodes share one requestId (selected together on click).
    const col = get().collections.find((c) => c.id === colId);
    const colRequestIds = new Set<string>();
    const collectIds = (items: CollectionItem[]) => {
      for (const it of items) {
        if (it.type === "request") colRequestIds.add(it.requestId);
        else if (it.type === "folder") collectIds(it.items);
      }
    };
    if (col) collectIds(col.items);

    // method|name → existing request id (current collection only, HTTP requests only — protocol requests do not participate in dedup)
    const keyToId: Record<string, string> = {};
    const requests = get().requests;
    for (const id of colRequestIds) {
      const req = requests[id];
      if (req && isHttpRequest(req)) keyToId[`${req.method}|${req.name}`] = id;
    }

    // created: newly created requests (need tree nodes inserted); overwritten: already in the collection, content updated only (no new nodes)
    const created: { requestId: string; group: string }[] = [];
    const overwritten: string[] = [];
    for (const ep of endpoints) {
      const dedupKey = `${ep.method}|${ep.name}`;
      const ct = ep.contentType || "";
      const bodyMode = inferBodyMode(ct);
      // Match modelRef to imported model ID
      const modelId =
        ep.modelRef && modelIds ? (modelIds[ep.modelRef] ?? null) : null;
      const auth = buildAuth(ep);
      const responses = buildResponses(ep.responses);
      if (keyToId[dedupKey]) {
        // An endpoint with the same name and method exists in this collection → overwrite (dedup: no duplicate entry, no duplicate node insertion)
        const existingId = keyToId[dedupKey];
        set((s) => ({
          requests: {
            ...s.requests,
            [existingId]: {
              ...s.requests[existingId],
              name: ep.name || s.requests[existingId].name,
              method: ep.method,
              url: ep.url,
              headers: Object.entries(ep.headers).map(([key, value]) => ({
                id: uid("kv"),
                key,
                value,
                enabled: true,
              })),
              queryParams: Object.entries(ep.queryParams || {}).map(
                ([key, value]) => ({
                  id: uid("kv"),
                  key,
                  value,
                  enabled: true,
                }),
              ),
              body: ep.body,
              bodyByMode: { [bodyMode]: ep.body },
              contentType: ct,
              bodyMode,
              modelId,
              auth,
              responses,
              prereqScript: ep.prereqScript,
              postreqScript: ep.postreqScript,
            },
          },
          tabs: s.tabs.map((t) =>
            t.requestId === existingId ? { ...t, dirty: false } : t,
          ),
        }));
        overwritten.push(existingId);
      } else {
        const id = get().addRequest({
          name: ep.name || t("request.imported"),
          method: ep.method,
          url: ep.url,
          headers: Object.entries(ep.headers).map(([key, value]) => ({
            id: uid("kv"),
            key,
            value,
            enabled: true,
          })),
          queryParams: Object.entries(ep.queryParams || {}).map(
            ([key, value]) => ({ id: uid("kv"), key, value, enabled: true }),
          ),
          body: ep.body,
          bodyByMode: { [bodyMode]: ep.body },
          contentType: ct,
          bodyMode,
          modelId,
          auth,
          responses,
          prereqScript: ep.prereqScript,
          postreqScript: ep.postreqScript,
        });
        created.push({ requestId: id, group: ep.group || "" });
      }
    }

    set((s) => {
      const collections = s.collections.map((c) => {
        if (c.id !== colId) return c;
        let draft = [...c.items];

        if (targetFolderId) {
          // Import into specific target folder
          const targetNode = findFolderNode(draft, targetFolderId);
          if (targetNode && targetNode.type === "folder") {
            const nodes = created.map((cr) => ({
              type: "request" as const,
              id: uid("ci"),
              requestId: cr.requestId,
            }));
            return {
              ...c,
              items: replaceFolderItems(draft, targetFolderId, [
                ...targetNode.items,
                ...nodes,
              ]),
            };
          }
        }

        // Group by group field
        const grouped = new Map<string, typeof created>();
        const ungrouped: typeof created = [];
        for (const cr of created) {
          if (cr.group) {
            if (!grouped.has(cr.group)) grouped.set(cr.group, []);
            grouped.get(cr.group)!.push(cr);
          } else {
            ungrouped.push(cr);
          }
        }

        // Create folders for groups
        for (const [groupName, items] of grouped) {
          let folder = draft.find(
            (it) => it.type === "folder" && it.name === groupName,
          );
          if (!folder) {
            folder = {
              type: "folder" as const,
              id: uid("f"),
              name: groupName,
              items: [],
            };
            draft = [...draft, folder];
          }
          const nodes = items.map((cr) => ({
            type: "request" as const,
            id: uid("ci"),
            requestId: cr.requestId,
          }));
          draft = draft.map((it) =>
            it.type === "folder" && it.id === folder!.id
              ? { ...it, items: [...it.items, ...nodes] }
              : it,
          );
        }

        // Ungrouped entries go into the "import" folder
        if (ungrouped.length > 0) {
          let folder = draft.find(
            (it) =>
              it.type === "folder" && it.name === t("request.importFolder"),
          );
          if (!folder) {
            folder = {
              type: "folder" as const,
              id: uid("f"),
              name: t("request.importFolder"),
              items: [],
            };
            draft = [...draft, folder];
          }
          const nodes = ungrouped.map((cr) => ({
            type: "request" as const,
            id: uid("ci"),
            requestId: cr.requestId,
          }));
          draft = draft.map((it) =>
            it.type === "folder" && it.id === folder!.id
              ? { ...it, items: [...it.items, ...nodes] }
              : it,
          );
        }

        return { ...c, items: draft };
      });
      return { collections };
    });
    get().track("import");
    // Open the first imported/updated request (prefer newly created; when all are overwrites, open the first overwritten one)
    const firstToOpen = created[0]?.requestId ?? overwritten[0];
    if (firstToOpen) get().openTab(firstToOpen);
    void saveNow();
  },
  openTab: (requestId) => {
    const s = get();
    // Opening any request tab → clears the active package/service node of the gRPC collection,
    // otherwise switching back to an HTTP request would keep showing the gRPC page on the right.
    if (s.activeGrpcNode) set({ activeGrpcNode: null });
    const existing = s.tabs.find((t) => t.requestId === requestId);
    // Which request was active before the switch (used to decide whether the response area must be reset)
    const prevRequestId =
      s.tabs.find((t) => t.id === s.activeTabId)?.requestId ?? null;
    if (existing) {
      set({ activeTabId: existing.id, activeModelId: null });
    } else {
      const tabId = uid("tab");
      set({
        tabs: [...s.tabs, { id: tabId, requestId, dirty: false }],
        activeTabId: tabId,
        activeModelId: null,
      });
    }
    // When switching to a different request, clear the previous response so the response area (including the "Request" tab) resets with the current request,
    // avoiding the stale previous request's built data making "switching requests leave the content unchanged".
    if (prevRequestId !== requestId) {
      set({ response: null, responseError: null, assertionResults: [] });
    }
  },
  openGrpcTab: (grpcNode) => {
    const s = get();
    // Note: activeGrpcNode is not cleared — it drives collection tree highlighting (tabs and tree selection stay consistent)
    const existing = s.tabs.find(
      (t) =>
        t.grpcNode &&
        t.grpcNode.nodeId === grpcNode.nodeId &&
        t.grpcNode.type === grpcNode.type,
    );
    if (existing) {
      set({ activeTabId: existing.id, activeModelId: null });
    } else {
      const tabId = uid("tab");
      set({
        tabs: [...s.tabs, { id: tabId, requestId: "", grpcNode, dirty: false }],
        activeTabId: tabId,
        activeModelId: null,
      });
    }
    // Clear the previous response when switching nodes
    set({ response: null, responseError: null, assertionResults: [] });
  },
  closeTab: (tabId) =>
    set((s) => {
      const tabs = s.tabs.filter((t) => t.id !== tabId);
      const activeTabId =
        s.activeTabId === tabId
          ? (tabs[tabs.length - 1]?.id ?? null)
          : s.activeTabId;
      return { tabs, activeTabId };
    }),
  setActiveTab: (tabId) => {
    const s = get();
    // Switching between tabs → also clears the gRPC node active state, ensuring the right side shows the request editor rather than the gRPC page
    if (s.activeGrpcNode) set({ activeGrpcNode: null });
    const prevRequestId =
      s.tabs.find((t) => t.id === s.activeTabId)?.requestId ?? null;
    const targetRequestId =
      s.tabs.find((t) => t.id === tabId)?.requestId ?? null;
    set({ activeTabId: tabId });
    // Switching between tabs to a different request also resets the response area
    if (prevRequestId !== targetRequestId) {
      set({ response: null, responseError: null, assertionResults: [] });
    }
  },
  closeAllTabs: () => set({ tabs: [], activeTabId: null }),
  closeOtherTabs: (tabId) =>
    set((s) => ({
      tabs: s.tabs.filter((t) => t.id === tabId),
      activeTabId: tabId,
    })),
  closeTabsLeft: (tabId) =>
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return s;
      const tabs = s.tabs.slice(idx);
      const activeTabId =
        s.activeTabId && tabs.some((t) => t.id === s.activeTabId)
          ? s.activeTabId
          : (tabs[tabs.length - 1]?.id ?? null);
      return { tabs, activeTabId };
    }),
  closeTabsRight: (tabId) =>
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return s;
      const tabs = s.tabs.slice(0, idx + 1);
      const activeTabId =
        s.activeTabId && tabs.some((t) => t.id === s.activeTabId)
          ? s.activeTabId
          : (tabs[tabs.length - 1]?.id ?? null);
      return { tabs, activeTabId };
    }),
  closeCleanTabs: () =>
    set((s) => {
      // Close all unmodified tabs (no unsaved changes), keeping the ones marked dirty
      const tabs = s.tabs.filter((t) => s.dirty[t.requestId]);
      const activeTabId =
        s.activeTabId && tabs.some((t) => t.id === s.activeTabId)
          ? s.activeTabId
          : (tabs[tabs.length - 1]?.id ?? null);
      return { tabs, activeTabId };
    }),
  setRequestTab: (t) => set({ requestTab: t }),
});
