// Collection domain: create/update/delete and drag-ordering of collections/folders/request nodes.
import type { StateCreator } from "zustand";
import type {
  ApiRequest,
  Collection,
  CollectionItem,
  ConnectionConfig,
  GrpcCollectionMeta,
  GrpcPackageNode,
  GrpcRequest,
} from "@/data/types";
import { createRequest } from "@/data/types";
import { defaultCollection, seedCollections, uid } from "@/data/seed";
import { saveNow } from "@/lib/persistence";
import {
  addToFolder,
  extractFromTree,
  findAndRemove,
  findPath,
  insertAt,
  insertRelativeToTarget,
  renameFolder,
} from "../utils/tree";
import type { AppState } from "../types";

export interface CollectionSlice {
  collections: Collection[];
  /** Currently selected collection (default target for new requests / folders; falls back to the first collection when unset) */
  activeCollectionId: string | null;
  setActiveCollection: (id: string) => void;

  addCollection: (
    name: string,
    kind?: string,
    connection?: ConnectionConfig,
  ) => string;
  /** Create a grpc collection: expands the parsed package/service/rpc hierarchy into collection tree nodes and creates a request per rpc */
  createGrpcCollection: (name: string, meta: GrpcCollectionMeta) => void;
  /** Rebuild the node tree of an existing grpc collection from the parsed hierarchy (idempotent: deduped by rpc name) */
  rebuildGrpcCollection: (
    collectionId: string,
    meta: GrpcCollectionMeta,
  ) => void;
  /** Update the proto content of a package in a grpc collection (written back to meta for persistence) */
  updateGrpcPackageProto: (
    collectionId: string,
    packageName: string,
    proto: string,
  ) => void;
  /** Update the config of a package in a grpc collection (auth / scripts) */
  updateGrpcPackageConfig: (
    collectionId: string,
    packageName: string,
    patch: Partial<{
      auth: import("@/data/types").AuthConfig;
      prereqScript: string;
      postreqScript: string;
    }>,
  ) => void;
  /** Delete a package from a grpc collection: removes its rpc requests, tree nodes and the package entry in meta */
  removeGrpcPackage: (collectionId: string, packageName: string) => void;
  /** Update the connection config of a connection-based collection (single source of truth at connection level) */
  updateConnection: (collectionId: string, config: ConnectionConfig) => void;
  renameCollection: (id: string, name: string) => void;
  removeCollection: (id: string) => void;
  addRequestToCollection: (
    collectionId: string,
    folderId?: string | null,
    protocol?: string,
  ) => string;
  removeItemFromCollection: (collectionId: string, itemId: string) => void;
  addFolderToCollection: (collectionId: string, name: string) => void;
  renameFolderInCollection: (
    collectionId: string,
    folderId: string,
    name: string,
  ) => void;
  moveItem: (
    collectionId: string,
    itemId: string,
    toFolderId: string | null,
    index: number,
  ) => void;
  moveCollectionItem: (
    dragId: string,
    targetId: string | null,
    placement: "before" | "after" | "inside",
  ) => void;
}

/** Stable key: the package/service/method combination (maps rpc nodes to requests) */
function rpcKey(
  packageName: string,
  serviceName: string,
  method: string,
): string {
  return `${packageName}/${serviceName}/${method}`;
}

/** Build grpc requests from the three-level package/service/rpc description (without collection tree nodes).
 *  Returns the requests plus a stable-key → requestId mapping for tree nodes to reference.
 *  defaultUrl is the collection connection address: newly imported requests default to it (existing requests keep the user's edits). */
function buildGrpcRequests(
  packages: GrpcPackageNode[],
  existing: Record<string, ApiRequest>,
  defaultUrl = "",
): {
  requests: Record<string, GrpcRequest>;
  requestIdByKey: Map<string, string>;
} {
  const requests: Record<string, GrpcRequest> = {};
  const requestIdByKey = new Map<string, string>();

  for (const pkg of packages) {
    for (const svc of pkg.services) {
      for (const rpc of svc.methods) {
        // Idempotent: reuse the existing request for the same (package, service, method), keeping the user's edits
        const key = rpcKey(pkg.name, svc.name, rpc.name);
        const existingReq = Object.values(existing).find(
          (r): r is GrpcRequest =>
            r.protocol === "grpc" &&
            "packageName" in r &&
            (r.packageName ?? "") === pkg.name &&
            (r.serviceName ?? "") === svc.name &&
            (r.method ?? "") === rpc.name,
        );
        const id = existingReq ? existingReq.id : uid("req");
        const base = createRequest("grpc", rpc.name) as GrpcRequest;
        requests[id] = {
          ...base,
          id,
          name: rpc.name,
          url: existingReq?.url ?? defaultUrl,
          packageName: pkg.name,
          serviceName: svc.name,
          method: rpc.name,
          inputType: rpc.inputType,
          outputType: rpc.outputType,
          streaming: rpc.clientStreaming
            ? rpc.clientStreaming && rpc.serverStreaming
              ? "bidirectional"
              : "client_streaming"
            : rpc.serverStreaming
              ? "server_streaming"
              : undefined,
          message: existingReq?.message ?? "",
          messageTemplate: existingReq?.messageTemplate,
          metadata: existingReq?.metadata ?? [],
          auth: existingReq?.auth ?? { type: "none" },
          prereqScript: existingReq?.prereqScript ?? "",
          postreqScript: existingReq?.postreqScript ?? "",
        };
        requestIdByKey.set(key, id);
      }
    }
  }

  return { requests, requestIdByKey };
}

/** Build collection tree nodes from the three-level package/service/rpc description (package→service→rpc flattened) */
function buildGrpcTree(
  packages: GrpcPackageNode[],
  requestIdByKey: Map<string, string>,
): CollectionItem[] {
  const items: CollectionItem[] = [];
  for (const pkg of packages) {
    const pkgNode: CollectionItem = {
      type: "grpc-package",
      id: uid("gpkg"),
      name: pkg.name || "(default)",
      proto: pkg.proto,
      services: pkg.services,
    };
    items.push(pkgNode);
    for (const svc of pkg.services) {
      items.push({
        type: "grpc-service",
        id: uid("gsvc"),
        packageName: pkg.name,
        name: svc.name,
        methods: svc.methods,
      });
      for (const rpc of svc.methods) {
        const key = rpcKey(pkg.name, svc.name, rpc.name);
        const requestId = requestIdByKey.get(key) ?? uid("req");
        items.push({ type: "grpc-rpc", id: uid("grpc"), requestId });
      }
    }
  }
  return items;
}

export const createCollectionSlice: StateCreator<
  AppState,
  [],
  [],
  CollectionSlice
> = (set, get) => ({
  collections: seedCollections(),
  activeCollectionId: null,

  setActiveCollection: (id) => set({ activeCollectionId: id }),

  addCollection: (name, kind, connection) => {
    const id = uid("col");
    set((s) => ({
      collections: [
        ...s.collections,
        // Unset kind = an "http" collection; connection-based collections carry `connection` (single source of truth at connection level)
        kind && kind !== "http"
          ? { id, name, kind, connection, items: [] }
          : { id, name, items: [] },
      ],
    }));
    void saveNow();
    return id;
  },
  updateConnection: (collectionId, config) => {
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? { ...c, connection: { ...c.connection, ...config } }
          : c,
      ),
    }));
    void saveNow();
  },
  createGrpcCollection: (name, meta) => {
    const collectionId = uid("col");
    // Create the requests first (writing into `requests`), then build tree nodes referencing their requestId
    const { requests, requestIdByKey } = buildGrpcRequests(
      meta.packages,
      get().requests,
    );
    set((s) => ({
      requests: { ...s.requests, ...requests },
      collections: [
        ...s.collections,
        {
          id: collectionId,
          name,
          kind: "grpc",
          grpc: meta,
          items: buildGrpcTree(meta.packages, requestIdByKey),
        },
      ],
      activeCollectionId: collectionId,
    }));
    void saveNow();
  },
  rebuildGrpcCollection: (collectionId, meta) => {
    const col = get().collections.find((c) => c.id === collectionId);
    if (!col) return;
    const { requests, requestIdByKey } = buildGrpcRequests(
      meta.packages,
      get().requests,
      col.connection?.url ?? "",
    );
    set((s) => ({
      requests: { ...s.requests, ...requests },
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? {
              ...c,
              grpc: meta,
              items: buildGrpcTree(meta.packages, requestIdByKey),
            }
          : c,
      ),
    }));
    void saveNow();
  },
  updateGrpcPackageProto: (collectionId, packageName, proto) => {
    set((s) => ({
      collections: s.collections.map((c) => {
        if (c.id !== collectionId || !c.grpc) return c;
        const packages = c.grpc.packages.map((p) =>
          p.name === packageName ? { ...p, proto } : p,
        );
        return { ...c, grpc: { ...c.grpc, packages } };
      }),
    }));
    void saveNow();
  },
  updateGrpcPackageConfig: (collectionId, packageName, patch) => {
    set((s) => ({
      collections: s.collections.map((c) => {
        if (c.id !== collectionId || !c.grpc) return c;
        const packages = c.grpc.packages.map((p) =>
          p.name === packageName ? { ...p, ...patch } : p,
        );
        return { ...c, grpc: { ...c.grpc, packages } };
      }),
    }));
    void saveNow();
  },
  removeGrpcPackage: (collectionId, packageName) => {
    const col = get().collections.find((c) => c.id === collectionId);
    if (!col?.grpc) return;
    const pkg = col.grpc.packages.find((p) => p.name === packageName);
    if (!pkg) return;
    // Stable keys of every rpc in this package (package/service/method)
    const keys = new Set<string>();
    for (const svc of pkg.services) {
      for (const rpc of svc.methods) {
        keys.add(rpcKey(pkg.name, svc.name, rpc.name));
      }
    }
    // Collect the request ids belonging to this package
    const requestIds = new Set<string>();
    for (const [rid, r] of Object.entries(get().requests)) {
      if (
        r?.protocol === "grpc" &&
        "packageName" in r &&
        keys.has(
          rpcKey(r.packageName ?? "", r.serviceName ?? "", r.method ?? ""),
        )
      ) {
        requestIds.add(rid);
      }
    }
    // Delete the requests one by one (also clearing drafts / dirty flags / open tabs)
    for (const rid of requestIds) get().removeRequest(rid);
    // Remove the package from meta and the package/service/rpc nodes from the collection tree
    set((s) => ({
      collections: s.collections.map((c) => {
        if (c.id !== collectionId || !c.grpc) return c;
        return {
          ...c,
          grpc: {
            ...c.grpc,
            packages: c.grpc.packages.filter((p) => p.name !== packageName),
          },
          items: c.items.filter((it) => {
            if (it.type === "grpc-package")
              return it.name !== (pkg.name || "(default)");
            if (it.type === "grpc-service") return it.packageName !== pkg.name;
            if (it.type === "grpc-rpc") return !requestIds.has(it.requestId);
            return true;
          }),
        };
      }),
    }));
    void saveNow();
  },
  renameCollection: (id, name) => {
    set((s) => ({
      collections: s.collections.map((c) => (c.id === id ? { ...c, name } : c)),
    }));
    void saveNow();
  },
  removeCollection: (id) => {
    set((s) => {
      const rest = s.collections.filter((c) => c.id !== id);
      // Always keep at least one collection (falling back to the default space when all are deleted) so API management stays usable
      return {
        collections:
          rest.length > 0
            ? rest
            : [
                {
                  ...defaultCollection(get().locale),
                  id: uid("col"),
                  items: [],
                },
              ],
        // Reset the selection when the currently selected collection is deleted
        activeCollectionId:
          s.activeCollectionId === id ? null : s.activeCollectionId,
      };
    });
    void saveNow();
  },
  addRequestToCollection: (collectionId, folderId, protocol) => {
    const requestId = get().addRequest(undefined, protocol);
    const node: CollectionItem = { type: "request", id: uid("ci"), requestId };
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? {
              ...c,
              items: folderId
                ? addToFolder(c.items, folderId, node)
                : [...c.items, node],
            }
          : c,
      ),
    }));
    get().openTab(requestId);
    void saveNow();
    return requestId;
  },
  removeItemFromCollection: (collectionId, itemId) => {
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? { ...c, items: findAndRemove(c.items, itemId) }
          : c,
      ),
    }));
    void saveNow();
  },
  addFolderToCollection: (collectionId, name) => {
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? {
              ...c,
              items: [
                ...c.items,
                { type: "folder", id: uid("f"), name, items: [] },
              ],
            }
          : c,
      ),
    }));
    void saveNow();
  },
  renameFolderInCollection: (collectionId, folderId, name) => {
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? { ...c, items: renameFolder(c.items, folderId, name) }
          : c,
      ),
    }));
    void saveNow();
  },
  moveItem: (collectionId, itemId, toFolderId, index) => {
    const col = get().collections.find((c) => c.id === collectionId);
    if (!col) return;
    const { node, rest } = extractFromTree(col.items, itemId);
    if (!node) return;
    set((s) => ({
      collections: s.collections.map((c) =>
        c.id === collectionId
          ? { ...c, items: insertAt(rest, toFolderId, index, node) }
          : c,
      ),
    }));
    void saveNow();
  },
  moveCollectionItem: (dragId, targetId, placement) => {
    if (dragId === targetId) return;
    set((s) => ({
      collections: s.collections.map((c) => {
        const dragPath = findPath(c.items, dragId);
        if (!dragPath) return c; // the dragged item is not in this collection
        // A folder must not be dragged into one of its own descendants
        if (placement === "inside") {
          const tgtPath = findPath(c.items, targetId ?? "");
          if (tgtPath && tgtPath.some((p) => p.id === dragId)) return c;
        }
        const { node, rest } = extractFromTree(c.items, dragId);
        if (!node) return c;
        if (targetId === null) {
          return { ...c, items: insertAt(rest, null, rest.length, node) };
        }
        if (placement === "inside") {
          return { ...c, items: addToFolder(rest, targetId, node) };
        }
        return {
          ...c,
          items: insertRelativeToTarget(rest, targetId, node, placement),
        };
      }),
    }));
    void saveNow();
  },
});
