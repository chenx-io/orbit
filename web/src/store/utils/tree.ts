// Immutable operation helpers for the collection tree (CollectionItem).
// Every function is pure: it takes a tree and returns a new tree without mutating the input.
import type { CollectionItem } from "@/data/types";

/** Remove the node with the given id from the tree (recursive). */
export function findAndRemove(
  items: CollectionItem[],
  itemId: string,
): CollectionItem[] {
  return items
    .filter((it) => it.id !== itemId)
    .map((it) =>
      it.type === "folder"
        ? { ...it, items: findAndRemove(it.items, itemId) }
        : it,
    );
}

/** Append `node` to the end of the items of the given folderId (recursive lookup). */
export function addToFolder(
  items: CollectionItem[],
  folderId: string,
  node: CollectionItem,
): CollectionItem[] {
  return items.map((it) => {
    if (it.type === "folder") {
      if (it.id === folderId) return { ...it, items: [...it.items, node] };
      return { ...it, items: addToFolder(it.items, folderId, node) };
    }
    return it;
  });
}

/** Rename the folder with the given folderId. */
export function renameFolder(
  items: CollectionItem[],
  folderId: string,
  name: string,
): CollectionItem[] {
  return items.map((it) => {
    if (it.type === "folder") {
      if (it.id === folderId) return { ...it, name };
      return { ...it, items: renameFolder(it.items, folderId, name) };
    }
    return it;
  });
}

/** Find the folder node with the given folderId; returns null when absent. */
export function findFolderNode(
  items: CollectionItem[],
  folderId: string,
): CollectionItem | null {
  for (const it of items) {
    if (it.type === "folder") {
      if (it.id === folderId) return it;
      const found = findFolderNode(it.items, folderId);
      if (found) return found;
    }
  }
  return null;
}

/** Replace the items of the given folderId wholesale with `newItems`. */
export function replaceFolderItems(
  items: CollectionItem[],
  folderId: string,
  newItems: CollectionItem[],
): CollectionItem[] {
  return items.map((it) => {
    if (it.type === "folder") {
      if (it.id === folderId) return { ...it, items: newItems };
      return { ...it, items: replaceFolderItems(it.items, folderId, newItems) };
    }
    return it;
  });
}

/** Detach the node with the given id from the tree, returning { node, rest }; `node` is null when not found. */
export function extractFromTree(
  items: CollectionItem[],
  itemId: string,
): { node: CollectionItem | null; rest: CollectionItem[] } {
  let node: CollectionItem | null = null;
  const rest: CollectionItem[] = [];
  for (const it of items) {
    if (it.id === itemId) {
      node = it;
      continue;
    }
    if (it.type === "folder") {
      const sub = extractFromTree(it.items, itemId);
      if (sub.node) {
        node = sub.node;
        rest.push({ ...it, items: sub.rest });
      } else {
        rest.push(it);
      }
    } else {
      rest.push(it);
    }
  }
  return { node, rest };
}

/** Insert `node` at `index` under toFolderId (null = root level). */
export function insertAt(
  items: CollectionItem[],
  toFolderId: string | null,
  index: number,
  node: CollectionItem,
): CollectionItem[] {
  if (toFolderId === null) {
    const copy = [...items];
    copy.splice(Math.min(index, copy.length), 0, node);
    return copy;
  }
  return items.map((it) => {
    if (it.type === "folder" && it.id === toFolderId) {
      const copy = [...it.items];
      copy.splice(Math.min(index, copy.length), 0, node);
      return { ...it, items: copy };
    }
    if (it.type === "folder")
      return { ...it, items: insertAt(it.items, toFolderId, index, node) };
    return it;
  });
}

/** Return the path from the root to targetId (including itself); null when not found. */
export function findPath(
  items: CollectionItem[],
  id: string,
  trail: CollectionItem[] = [],
): CollectionItem[] | null {
  for (const it of items) {
    if (it.id === id) return [...trail, it];
    if (it.type === "folder") {
      const r = findPath(it.items, id, [...trail, it]);
      if (r) return r;
    }
  }
  return null;
}

/** Return the immediate parent folder id of a request item; null at root level, undefined when not found. */
export function findParentFolderId(
  items: CollectionItem[],
  requestId: string,
  parentId: string | null = null,
): string | null | undefined {
  for (const it of items) {
    if (it.type === "request" && it.requestId === requestId) return parentId;
    if (it.type === "folder") {
      const r = findParentFolderId(it.items, requestId, it.id);
      if (r !== undefined) return r;
    }
  }
  return undefined;
}

/** Insert `node` among the siblings of targetId according to before/after; returns the tree unchanged when targetId is missing. */
export function insertRelativeToTarget(
  items: CollectionItem[],
  targetId: string,
  node: CollectionItem,
  placement: "before" | "after",
): CollectionItem[] {
  const idx = items.findIndex((it) => it.id === targetId);
  if (idx >= 0) {
    const copy = [...items];
    copy.splice(placement === "before" ? idx : idx + 1, 0, node);
    return copy;
  }
  return items.map((it) =>
    it.type === "folder"
      ? {
          ...it,
          items: insertRelativeToTarget(it.items, targetId, node, placement),
        }
      : it,
  );
}
