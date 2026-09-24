// Unified tree row component: every tree UI (collection tree / scenario tree / suite list / selectable scenario tree ...) shares
// the same implementation, so indentation, selection, hover actions and drop indicators stay identical and are maintained in one place.
// Usage: the caller flattens its data into an array of visible rows carrying a depth (collapse logic lives with the caller),
// renders each row with <TreeRow> and composes the content through the leading / children / actions slots.
import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  treeIndent,
  treeRowCls,
  treeDropLine,
  treeDropLineTop,
  treeDropLineBottom,
} from "./treeStyle";

export type TreeDropPos = "before" | "after" | "inside";

export interface TreeRowProps {
  /** Indentation level */
  depth?: number;
  /** Whether it is selected / active */
  selected?: boolean;
  /** Whether it is being dragged (semi-transparent) */
  dragging?: boolean;
  /** Drop position (before/after lines, an inset outline for inside) */
  dropPos?: TreeDropPos | null;
  /** Whether it is expandable (shows the collapse arrow) */
  hasChildren?: boolean;
  /** Whether it is collapsed */
  collapsed?: boolean;
  /** Collapse-arrow click callback (mutually exclusive with onClick; stops propagation automatically) */
  onToggle?: () => void;
  /** Click on the row body */
  onClick?: () => void;
  /** Row pointerdown (the parent's drag logic) */
  onPointerDown?: (e: React.PointerEvent) => void;
  /** Leading icon (folder / file / checkbox ...) */
  leading?: React.ReactNode;
  /** Middle content (name + metadata + badges, flex-1) */
  children?: React.ReactNode;
  /** Trailing actions (revealed on hover by default: run / delete ...) */
  actions?: React.ReactNode;
  /** Extra classes */
  className?: string;
  /** data-* attributes forwarded to the root div (e.g. data-node-id / data-node-kind) */
  dataAttrs?: Record<string, string>;
  /** tooltip */
  title?: string;
}

export function TreeRow({
  depth = 0,
  selected,
  dragging,
  dropPos,
  hasChildren,
  collapsed,
  onToggle,
  onClick,
  onPointerDown,
  leading,
  children,
  actions,
  className,
  dataAttrs,
  title,
}: TreeRowProps) {
  return (
    <div
      {...dataAttrs}
      title={title}
      onPointerDown={onPointerDown}
      onClick={onClick}
      className={cn(
        treeRowCls({
          active: selected,
          dragging,
          dropInside: dropPos === "inside",
        }),
        className,
      )}
      style={treeIndent(depth)}
    >
      {/* Drop lines (before/after) */}
      {dropPos === "before" && (
        <span className={cn(treeDropLine, treeDropLineTop)} />
      )}
      {dropPos === "after" && (
        <span className={cn(treeDropLine, treeDropLineBottom)} />
      )}

      {/* Collapse arrow / placeholder */}
      {hasChildren ? (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onToggle?.();
          }}
          className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent"
          aria-label={collapsed ? "expand" : "collapse"}
        >
          {collapsed ? (
            <ChevronRight className="h-3.5 w-3.5" />
          ) : (
            <ChevronDown className="h-3.5 w-3.5" />
          )}
        </button>
      ) : (
        <span className="h-3.5 w-3.5 shrink-0" />
      )}

      {leading}

      {/* Middle content: flexible width, truncatable */}
      <div className="flex min-w-0 flex-1 items-center gap-1.5">{children}</div>

      {/* Trailing actions: revealed on hover by default */}
      {actions && (
        <div className="ml-auto flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
          {actions}
        </div>
      )}
    </div>
  );
}
