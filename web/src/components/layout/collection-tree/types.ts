// Shared type definitions for the collection tree.
import type { CollectionItem } from "@/data/types";

export const INDENT = 14;

/** Context payload of the context menu */
export type CtxPayload = {
  x: number;
  y: number;
  kind: "request" | "folder" | "collection" | "grpc-package";
  item?: CollectionItem;
  collectionId: string;
  /** When kind is "grpc-package" this is the package name */
  packageName?: string;
};

/** Context menu open callback (injected by the main component, writing the event coordinates into ctx state) */
export type CtxOpen = (
  e: React.MouseEvent,
  p: { kind: "request" | "folder"; item: CollectionItem; collectionId: string },
) => void;

/** Rename dialog state */
export interface RenameState {
  open: boolean;
  title: string;
  initialValue: string;
  onConfirm: (name: string) => void;
}

/** Drop position: before/after/inside (for inside, id is the target folder's id) */
export type DropTarget = {
  id: string | null;
  pos: "before" | "after" | "inside";
} | null;

/** Export target payload (shared by the context menu / inline menu) */
export interface ExportPayload {
  kind: "collection" | "folder" | "request";
  collectionId: string;
  item?: CollectionItem;
}
