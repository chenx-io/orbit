// Global tree styling conventions (the single source of truth).
// Consistent with the design language of the request-manager collection tree (collection-tree/TreeItem.tsx):
// compact rows (py-1), 14px indent per level, a light accent on hover, bg-accent/15 when selected,
// and a drop position expressed as before/after lines plus an inset outline for inside.
// Every tree UI (collection tree / scenario tree / future ones) must reuse the classes and constants exported here
// instead of hand-rolling its own indent / drop / selection styles.
import { cn } from "@/lib/utils";

/** Indent per level (px), matching the request-manager collection tree */
export const TREE_INDENT = 14;

/** Base row styling: compact, clickable, lightly highlighted on hover */
export const treeRow =
  "group relative flex cursor-pointer select-none items-center gap-1.5 rounded py-1 pr-1 text-sm transition-colors hover:bg-accent/10";

/** Selected / active state (the object being edited): a clearly visible left border plus highlight, so "no selection feedback" cannot happen */
export const treeRowActive =
  "bg-accent/25 font-medium ring-1 ring-inset ring-primary/40";

/** Semi-transparent drag source */
export const treeRowDragging = "opacity-40";

/** Drop position inside: an inset outline around the whole row */
export const treeDropInside = "ring-2 ring-inset ring-primary/70 bg-primary/10";

/** Drop lines for before/after (used with treeDropLineTop / Bottom) */
export const treeDropLine =
  "pointer-events-none absolute inset-x-0 z-10 h-0.5 rounded bg-primary";
export const treeDropLineTop = "-top-0.75";
export const treeDropLineBottom = "-bottom-0.75";

/** Collapse arrow */
export const treeChevron = "h-3.5 w-3.5 shrink-0 text-muted-foreground";

/** Folder icon (amber, matching the collection tree) */
export const treeFolderIcon = "h-4 w-4 shrink-0 text-amber-400";

/** Leaf node icon */
export const treeLeafIcon = "h-3.5 w-3.5 shrink-0 text-muted-foreground";

/** Inline hover button (⋮ / ▶ etc.) */
export const treeRowAction =
  "h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100";

/** Small count / metadata text */
export const treeMeta = "shrink-0 text-xs text-muted-foreground";

/** Compute the row indent for a depth (including the base padding, matching the collection tree) */
export function treeIndent(depth: number): React.CSSProperties {
  return { paddingLeft: depth * TREE_INDENT + 4 };
}

/** Convenience helper that composes the tree row class names */
export function treeRowCls(opts: {
  active?: boolean;
  dragging?: boolean;
  dropInside?: boolean;
  extra?: string;
}): string {
  return cn(
    treeRow,
    opts.active && treeRowActive,
    opts.dragging && treeRowDragging,
    opts.dropInside && treeDropInside,
    opts.extra,
  );
}
