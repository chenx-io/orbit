// Tree row component: folders (expandable/collapsible) and request nodes (method badge + name),
// supporting drag drop indicators (before/after lines, inside highlight) and an inline menu.
import { Folder, FolderOpen, MoreVertical } from "lucide-react";
import {
  cn,
  methodColor,
  methodBg,
  protocolColor,
  PROTOCOL_LABEL,
} from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useStore";
import {
  isHttpRequest,
  requestProtocol,
  type CollectionItem,
} from "@/data/types";
import { INDENT, type CtxOpen, type DropTarget } from "./types";
import { NameTooltip } from "./NameTooltip";

/** Drag ancestor guides (vertical dashed lines marking the level this row belongs to) */
function GuideLines({ offsets }: { offsets: number[] }) {
  return (
    <>
      {offsets.map((off, i) => (
        <div
          key={i}
          className="absolute top-0 bottom-0 w-px bg-border/50"
          style={{ left: off }}
        />
      ))}
    </>
  );
}

export function TreeItem({
  collectionId,
  item,
  depth,
  guides,
  onItemContextMenu,
  dragId,
  dropTarget,
  onRowPointerDown,
  expanded,
  onToggle,
}: {
  collectionId: string;
  item: CollectionItem;
  depth: number;
  guides: number[];
  onItemContextMenu: CtxOpen;
  dragId: string | null;
  dropTarget: DropTarget;
  onRowPointerDown: (
    id: string,
    e: React.PointerEvent,
    activate: () => void,
  ) => void;
  /** Top-level unified folder expansion state (unrecorded = expanded) */
  expanded: Record<string, boolean>;
  onToggle: (id: string) => void;
}) {
  const requests = useAppStore((s) => s.requests);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const tabs = useAppStore((s) => s.tabs);
  const openTab = useAppStore((s) => s.openTab);

  const isFld = item.type === "folder";
  // Folders expand only when explicitly expanded (unrecorded = collapsed by default); see CollectionTree colOpen for collections defaulting to expanded
  const open = expanded[item.id] === true;
  const pl = depth * INDENT;
  const iconCenter = pl + 4 + 8; // folder icon center X
  const childGuides = [...guides, iconCenter]; // guides inherited by children

  // Drop indicator (matching AutomationModule: explicit before/after lines and an inside highlight)
  const isBefore = dropTarget?.id === item.id && dropTarget.pos === "before";
  const isAfter = dropTarget?.id === item.id && dropTarget.pos === "after";
  const isInside = dropTarget?.id === item.id && dropTarget.pos === "inside";
  const isDragSource = dragId === item.id;
  const dropClass = isInside
    ? "ring-2 ring-inset ring-primary/70 bg-primary/10"
    : "";

  // The three grpc node levels are rendered by GrpcTree and not handled here (a defensive fallback)
  if (
    item.type === "grpc-package" ||
    item.type === "grpc-service" ||
    item.type === "grpc-rpc"
  ) {
    return null;
  }

  if (item.type === "folder") {
    return (
      <div className="relative">
        {/* Ancestor guides */}
        <GuideLines offsets={guides} />
        <div
          data-tree-id={item.id}
          data-can-nest={isFld ? "1" : "0"}
          onPointerDown={(e) =>
            onRowPointerDown(item.id, e, () => onToggle(item.id))
          }
          onContextMenu={(e) => {
            e.preventDefault();
            onItemContextMenu(e, { kind: "folder", item, collectionId });
          }}
          className={cn(
            "group relative flex items-center gap-1.5 rounded py-1 pr-1 text-sm hover:bg-accent/10 cursor-pointer select-none",
            dropClass,
            isDragSource && "opacity-40",
          )}
          style={{ paddingLeft: pl + 4, touchAction: "none" }}
        >
          {isBefore && (
            <div className="pointer-events-none absolute inset-x-0 -top-0.75 z-10 h-0.5 rounded bg-primary" />
          )}
          {isAfter && (
            <div className="pointer-events-none absolute inset-x-0 -bottom-0.75 z-10 h-0.5 rounded bg-primary" />
          )}
          {open ? (
            <FolderOpen className="h-4 w-4 shrink-0 text-amber-400" />
          ) : (
            <Folder className="h-4 w-4 shrink-0 text-amber-400" />
          )}
          <NameTooltip name={item.name} className="flex-1 text-left" />
          {/* Inline ⋮ button: reuses the context menu (TreeContextMenu) — one implementation including multi-protocol new requests */}
          <Button
            variant="ghost"
            size="icon-sm"
            className="opacity-0 group-hover:opacity-100 shrink-0 h-6 w-6"
            onClick={(e) => {
              e.stopPropagation();
              onItemContextMenu(e, { kind: "folder", item, collectionId });
            }}
          >
            <MoreVertical className="h-3.5 w-3.5" />
          </Button>
        </div>
        {open &&
          item.items.map((child) => (
            <TreeItem
              key={child.id}
              collectionId={collectionId}
              item={child}
              depth={depth + 1}
              guides={childGuides}
              onItemContextMenu={onItemContextMenu}
              dragId={dragId}
              dropTarget={dropTarget}
              onRowPointerDown={onRowPointerDown}
              expanded={expanded}
              onToggle={onToggle}
            />
          ))}
      </div>
    );
  }

  const req = requests[item.requestId];
  const isActive =
    tabs.find((t) => t.id === activeTabId)?.requestId === item.requestId;
  if (!req) return null;
  const protocol = requestProtocol(req);
  const isHttp = isHttpRequest(req);
  const bg = isHttp ? methodBg(req.method) : "bg-muted/60";
  const label = isHttp ? req.method : (PROTOCOL_LABEL[protocol] ?? protocol);
  const color = isHttp ? methodColor(req.method) : protocolColor(protocol);
  const lastGuide = guides[guides.length - 1] ?? -1;
  return (
    <>
      <div className="relative">
        {/* Ancestor guides */}
        <GuideLines offsets={guides} />
        <div
          data-tree-id={item.id}
          data-can-nest={isFld ? "1" : "0"}
          onPointerDown={(e) =>
            onRowPointerDown(item.id, e, () => openTab(item.requestId))
          }
          onContextMenu={(e) => {
            e.preventDefault();
            onItemContextMenu(e, { kind: "request", item, collectionId });
          }}
          className={cn(
            "group relative flex items-center gap-1.5 rounded py-1 pr-1 text-sm cursor-pointer select-none",
            // The selected state needs its own hover background: a hover variant wins otherwise and would cover the selected background
            isActive
              ? "bg-primary/15 font-medium text-foreground hover:bg-primary/25"
              : "hover:bg-accent/10",
            dropClass,
            isDragSource && "opacity-40",
          )}
          style={{ paddingLeft: pl + 4, touchAction: "none" }}
        >
          {isBefore && (
            <div className="pointer-events-none absolute inset-x-0 -top-0.75 z-10 h-0.5 rounded bg-primary" />
          )}
          {isAfter && (
            <div className="pointer-events-none absolute inset-x-0 -bottom-0.75 z-10 h-0.5 rounded bg-primary" />
          )}
          {/* Horizontal connector to the parent folder */}
          {lastGuide >= 0 && (
            <div
              className="absolute top-1/2 w-2.5 -translate-y-1/2"
              style={{ left: lastGuide }}
            >
              <div className="h-px w-full bg-border/50" />
            </div>
          )}
          <span
            className={cn(
              "shrink-0 rounded px-1.5 py-px text-xs font-bold leading-tight",
              bg,
              color,
            )}
          >
            {label}
          </span>
          <NameTooltip name={req.name} className="flex-1 text-left" />
          {/* Inline ⋮ button: reuses the context menu (TreeContextMenu) — one implementation */}
          <Button
            variant="ghost"
            size="icon-sm"
            className={cn(
              "opacity-0 group-hover:opacity-100 shrink-0 h-6 w-6",
              isActive && "opacity-100",
            )}
            onClick={(e) => {
              e.stopPropagation();
              onItemContextMenu(e, { kind: "request", item, collectionId });
            }}
          >
            <MoreVertical className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </>
  );
}
