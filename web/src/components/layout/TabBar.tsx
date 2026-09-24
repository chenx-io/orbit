import { useEffect, useRef, useState } from "react";
import { X, Plus } from "lucide-react";
import { cn, methodColor } from "@/lib/utils";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ProtocolNewMenu } from "@/components/common/ProtocolMenu";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import {
  isHttpRequest,
  requestProtocol,
  type Collection,
  type Tab,
} from "@/data/types";
import { PROTOCOL_LABEL } from "@/lib/utils";

/** Resolve the tab name of a grpc node (nodeId looks like pkg-{collectionId}-{pkg} / svc-{collectionId}-{pkg}-{svc}) */
function grpcNodeLabel(collections: Collection[], tab: Tab): string {
  const g = tab.grpcNode;
  if (!g) return "";
  const col = collections.find((c) => c.id === g.collectionId);
  const packages = col?.grpc?.packages ?? [];
  if (g.type === "grpc-package") {
    for (const p of packages) {
      if (`pkg-${g.collectionId}-${p.name}` === g.nodeId)
        return p.name || "(default)";
    }
    return g.nodeId;
  }
  for (const p of packages) {
    for (const svc of p.services) {
      if (`svc-${g.collectionId}-${p.name}-${svc.name}` === g.nodeId)
        return svc.name;
    }
  }
  return g.nodeId;
}

interface CtxMenuState {
  x: number;
  y: number;
  tabId: string;
}

export function TabBar() {
  const tabs = useAppStore((s) => s.tabs);
  const requests = useAppStore((s) => s.requests);
  const dirty = useAppStore((s) => s.dirty);
  const collections = useAppStore((s) => s.collections);
  const activeTabId = useAppStore((s) => s.activeTabId);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const setActiveGrpcNode = useAppStore((s) => s.setActiveGrpcNode);
  const closeTab = useAppStore((s) => s.closeTab);
  const closeAllTabs = useAppStore((s) => s.closeAllTabs);
  const closeOtherTabs = useAppStore((s) => s.closeOtherTabs);
  const closeTabsLeft = useAppStore((s) => s.closeTabsLeft);
  const closeTabsRight = useAppStore((s) => s.closeTabsRight);
  const closeCleanTabs = useAppStore((s) => s.closeCleanTabs);
  const addRequestToCollection = useAppStore((s) => s.addRequestToCollection);
  const activeCollectionId = useAppStore((s) => s.activeCollectionId);
  const renameRequest = useAppStore((s) => s.renameRequest);
  const { t } = useT();

  const [editingTabId, setEditingTabId] = useState<string | null>(null);
  const [ctx, setCtx] = useState<CtxMenuState | null>(null);
  const editRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLDivElement>(null);
  const prevTabCount = useRef(tabs.length);

  // New tabs scroll to the far right automatically (without disturbing a user scrolling back through history)
  useEffect(() => {
    const el = scrollRef.current;
    if (el && tabs.length > prevTabCount.current) {
      el.scrollTo({ left: el.scrollWidth, behavior: "smooth" });
    }
    prevTabCount.current = tabs.length;
  }, [tabs.length]);

  // Context menu: closes on an outside click / blur / window change
  useEffect(() => {
    if (!ctx) return;
    const close = () => setCtx(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [ctx]);

  // Mouse wheel → scroll tabs horizontally (a non-passive native listener, so the default can be prevented).
  // The listener is bound to the outer container barRef (always present, never unmounted by the empty tabs state) while scrolling applies to the scroll container scrollRef.
  useEffect(() => {
    const bar = barRef.current;
    if (!bar) return;
    const onWheel = (e: WheelEvent) => {
      const el = scrollRef.current;
      if (!el) return;
      const delta =
        Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (delta !== 0) {
        e.preventDefault();
        el.scrollBy({ left: delta });
      }
    };
    bar.addEventListener("wheel", onWheel, { passive: false });
    return () => bar.removeEventListener("wheel", onWheel);
  }, []);

  const startRename = (tabId: string) => {
    setEditingTabId(tabId);
    requestAnimationFrame(() => editRef.current?.select());
  };

  const commitRename = (requestId: string) => {
    const val = editRef.current?.value.trim();
    if (val) renameRequest(requestId, val);
    setEditingTabId(null);
  };

  // Switch tabs with the arrow keys / Home-End (effective while the container is focused)
  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const idx = tabs.findIndex((t) => t.id === activeTabId);
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      if (idx > 0) setActiveTab(tabs[idx - 1].id);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      if (idx >= 0 && idx < tabs.length - 1) setActiveTab(tabs[idx + 1].id);
    } else if (e.key === "Home") {
      e.preventDefault();
      if (tabs[0]) setActiveTab(tabs[0].id);
    } else if (e.key === "End") {
      e.preventDefault();
      if (tabs[tabs.length - 1]) setActiveTab(tabs[tabs.length - 1].id);
    }
  };

  if (tabs.length === 0) {
    return (
      <div
        ref={barRef}
        className="flex h-9 items-center border-b border-border bg-card/20 px-3 text-xs text-muted-foreground"
      >
        {t("tab.empty")}
      </div>
    );
  }

  const runCtxAction = (fn: () => void) => {
    fn();
    setCtx(null);
  };

  return (
    <div
      ref={barRef}
      className="flex h-9 shrink-0 items-center border-b border-border bg-card/20"
    >
      {/* Horizontally scrollable tab strip: no scrollbar, wheel scrolls sideways, arrow keys switch */}
      <div
        ref={scrollRef}
        tabIndex={0}
        onKeyDown={handleKeyDown}
        className="tab-scroll flex min-w-0 flex-1 items-center gap-1 px-2 outline-none"
      >
        {tabs.map((tab) => {
          const isGrpcNode = !!tab.grpcNode;
          const req = isGrpcNode ? undefined : requests[tab.requestId];
          const isActive = tab.id === activeTabId;
          const isEditing = tab.id === editingTabId && !isGrpcNode;
          return (
            <div
              key={tab.id}
              onClick={() => {
                setActiveTab(tab.id);
                // grpc node tab: sync the tree highlight (the package/service node's selected state)
                if (tab.grpcNode) {
                  setActiveGrpcNode({
                    collectionId: tab.grpcNode.collectionId,
                    type: tab.grpcNode.type,
                    id: tab.grpcNode.nodeId,
                  });
                }
                // Focus the container to enable arrow-key switching (editing in an input already stops propagation and does not steal focus)
                scrollRef.current?.focus({ preventScroll: true });
              }}
              onDoubleClick={(e) => {
                e.stopPropagation();
                if (!isEditing && !isGrpcNode) startRename(tab.id);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setCtx({ x: e.clientX, y: e.clientY, tabId: tab.id });
              }}
              className={cn(
                "group flex shrink-0 cursor-pointer items-center gap-2 rounded-t-md border-b-2 px-3 py-1.5 text-xs",
                isActive
                  ? "border-primary bg-background text-foreground"
                  : "border-transparent text-muted-foreground hover:bg-accent/10",
              )}
            >
              {!isGrpcNode && dirty[tab.requestId] ? (
                <span
                  className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
                  title={t("common.save")}
                />
              ) : (
                <span
                  className={cn(
                    "font-semibold",
                    isGrpcNode
                      ? "text-fuchsia-500"
                      : req
                        ? isHttpRequest(req)
                          ? methodColor(req.method)
                          : "text-muted-foreground"
                        : "",
                  )}
                >
                  {isGrpcNode
                    ? tab.grpcNode!.type === "grpc-package"
                      ? "PKG"
                      : "SVC"
                    : req
                      ? isHttpRequest(req)
                        ? req.method
                        : (PROTOCOL_LABEL[requestProtocol(req)] ??
                          requestProtocol(req))
                      : "?"}
                </span>
              )}
              {isEditing ? (
                <input
                  ref={editRef}
                  defaultValue={req?.name ?? ""}
                  onBlur={() => commitRename(tab.requestId)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename(tab.requestId);
                    if (e.key === "Escape") setEditingTabId(null);
                  }}
                  onClick={(e) => e.stopPropagation()}
                  className="h-5 w-35 rounded border border-primary bg-background px-1 text-xs outline-none"
                />
              ) : (
                <span
                  className="max-w-40 truncate"
                  title={isGrpcNode ? undefined : t("tab.renameHint")}
                >
                  {isGrpcNode
                    ? grpcNodeLabel(collections, tab)
                    : (req?.name ?? t("tab.deleted"))}
                </span>
              )}
              {!isEditing && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(tab.id);
                  }}
                  className="opacity-0 group-hover:opacity-100 hover:text-foreground"
                >
                  <X className="h-3 w-3" />
                </button>
              )}
            </div>
          );
        })}
      </div>

      {/* Quick new request */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            title={t("tab.newRequest")}
            className="flex h-full w-9 shrink-0 items-center justify-center border-l border-border text-muted-foreground hover:bg-accent/10 hover:text-foreground"
          >
            <Plus className="h-4 w-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-36">
          <ProtocolNewMenu
            label={t("tab.newRequest")}
            onSelect={(p) => {
              const colId = activeCollectionId ?? collections[0]?.id;
              if (!colId) return;
              addRequestToCollection(colId, null, p);
            }}
          />
        </DropdownMenuContent>
      </DropdownMenu>

      {/* Tab context menu */}
      {ctx && (
        <div
          className="fixed z-50 min-w-42.5 rounded-md border border-border bg-popover p-1 shadow-lg"
          style={{ left: ctx.x, top: ctx.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {(
            [
              { key: "close", fn: () => closeTab(ctx.tabId) },
              { key: "closeOthers", fn: () => closeOtherTabs(ctx.tabId) },
              { key: "closeLeft", fn: () => closeTabsLeft(ctx.tabId) },
              { key: "closeRight", fn: () => closeTabsRight(ctx.tabId) },
              { key: "closeClean", fn: () => closeCleanTabs() },
              { key: "closeAll", fn: () => closeAllTabs() },
            ] as { key: string; fn: () => void }[]
          ).map((item) => (
            <button
              key={item.key}
              className="flex w-full items-center rounded px-2 py-1.5 text-left text-xs hover:bg-accent/15"
              onClick={() => runCtxAction(item.fn)}
            >
              {t(`tab.${item.key}` as any)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
