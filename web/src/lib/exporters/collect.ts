// Pre-export processing: collect the endpoints to export from the collection tree and gather the referenced data models (including transitive references).
import type {
  ApiRequest,
  Collection,
  CollectionItem,
  DataModel,
} from "@/data/types";
import { isHttpRequest } from "@/data/types";
import type { ExportRequest } from "./types";

/** Return the node chain from the root to targetId (including itself); null when not found. */
function findChain(
  items: CollectionItem[],
  targetId: string,
): CollectionItem[] | null {
  for (const it of items) {
    if (it.id === targetId) return [it];
    if (it.type === "folder") {
      const sub = findChain(it.items, targetId);
      if (sub) return [it, ...sub];
    }
  }
  return null;
}

/**
 * Collect every endpoint under the export target.
 * - item is undefined → the whole collection (all folders included).
 * - item is a folder → every endpoint under it (subfolders included).
 * - item is a request → only that request.
 * The breadcrumb is the ancestor folder names (the collection name is excluded and the request itself does not count as a level).
 * Collections are an internal workspace concept and do not enter external exchange structures such as OpenAPI tags / Postman folders.
 */
export function collectExportRequests(
  collections: Collection[],
  requests: Record<string, ApiRequest>,
  collectionId: string,
  item?: CollectionItem,
): ExportRequest[] {
  const col = collections.find((c) => c.id === collectionId);
  if (!col) return [];

  const base: string[] = [];
  const out: ExportRequest[] = [];

  if (item && item.type === "request") {
    const req = requests[item.requestId];
    const chain = findChain(col.items, item.id);
    const ancestorFolders = (chain ?? [])
      .slice(0, -1)
      .filter(
        (i): i is Extract<CollectionItem, { type: "folder" }> =>
          i.type === "folder",
      )
      .map((i) => i.name);
    if (req && isHttpRequest(req))
      out.push({ request: req, breadcrumb: [...base, ...ancestorFolders] });
    return out;
  }

  const walk = (items: CollectionItem[], crumb: string[]) => {
    for (const it of items) {
      if (it.type === "request") {
        const req = requests[it.requestId];
        if (req && isHttpRequest(req))
          out.push({ request: req, breadcrumb: crumb });
      } else if (it.type === "folder") {
        // Recurse into plain folders only; the three grpc node levels (package/service/rpc) hold no exportable HTTP requests
        walk(it.items, [...crumb, it.name]);
      }
    }
  };

  if (item && item.type === "folder") {
    const chain = findChain(col.items, item.id);
    const ancestorFolders = (chain ?? [])
      .slice(0, -1)
      .filter(
        (i): i is Extract<CollectionItem, { type: "folder" }> =>
          i.type === "folder",
      )
      .map((i) => i.name);
    walk(item.items, [...base, ...ancestorFolders, item.name]);
  } else {
    walk(col.items, base);
  }
  return out;
}

/**
 * Collect the data models "referenced by the exported endpoints" (including transitive references).
 * Starting points: each request's request.modelId; then recurse through model fields' refModelId,
 * so referenced sub-models are exported too. Returns a deduped DataModel list.
 */
export function collectReferencedModels(
  items: ExportRequest[],
  models: DataModel[],
): DataModel[] {
  const byId = new Map(models.map((m) => [m.id, m]));
  const resultIds = new Set<string>();
  const queue: string[] = [];

  for (const { request } of items) {
    if (
      request.modelId &&
      byId.has(request.modelId) &&
      !resultIds.has(request.modelId)
    ) {
      resultIds.add(request.modelId);
      queue.push(request.modelId);
    }
  }
  while (queue.length) {
    const id = queue.pop()!;
    const m = byId.get(id);
    if (!m) continue;
    for (const f of m.fields) {
      if (
        f.refModelId &&
        byId.has(f.refModelId) &&
        !resultIds.has(f.refModelId)
      ) {
        resultIds.add(f.refModelId);
        queue.push(f.refModelId);
      }
    }
  }
  return [...resultIds].map((id) => byId.get(id)!).filter(Boolean);
}
