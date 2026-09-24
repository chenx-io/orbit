// Collection tree traversal helpers (used by CollectionTree only).
import type { Collection, CollectionItem } from "@/data/types";

/** Find an item across all collections (recursing into folders). */
export function findItemInCollections(
  collections: Collection[],
  id: string,
): CollectionItem | null {
  const walk = (its: CollectionItem[]): CollectionItem | null => {
    for (const it of its) {
      if (it.id === id) return it;
      if (it.type === "folder") {
        const f = walk(it.items);
        if (f) return f;
      }
    }
    return null;
  };
  for (const c of collections) {
    const r = walk(c.items);
    if (r) return r;
  }
  return null;
}

/** Return every descendant item id under a folder node (excluding itself). Used to forbid dropping a folder into its own subtree. */
export function descendantIds(node: CollectionItem): string[] {
  const out: string[] = [];
  if (node.type !== "folder") return out;
  const walk = (its: CollectionItem[]) => {
    for (const it of its) {
      out.push(it.id);
      if (it.type === "folder") walk(it.items);
    }
  };
  walk(node.items);
  return out;
}

/** Find the connection-type collection containing a given request id (the message editor inherits url/framing config from the collection connection through it) */
export function findConnectionCollection(
  collections: Collection[],
  requestId: string,
): Collection | null {
  const walk = (its: CollectionItem[]): boolean => {
    for (const it of its) {
      if (it.type === "request" && it.requestId === requestId) return true;
      if (it.type === "folder" && walk(it.items)) return true;
    }
    return false;
  };
  for (const c of collections) {
    if (c.kind !== undefined && c.kind !== "http" && walk(c.items)) return c;
  }
  return null;
}
