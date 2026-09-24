// Sidebar collection tree: hierarchical display of collections / folders / requests, drag reordering, context menu and import/export entries.
// It also hosts the model list of the "Data Models" tab. UI details are split into subcomponents in this directory.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Boxes,
  ChevronDown,
  ChevronRight,
  Download,
  FoldVertical,
  Folder,
  MoreVertical,
  Plus,
  Trash2,
  UnfoldVertical,
} from "lucide-react";
import { cn, PROTOCOL_LABEL, protocolColor } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProtocolNewMenu } from "@/components/common/ProtocolMenu";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import {
  isConnectionCollection,
  type Collection,
  type CollectionItem,
  type DataModel,
  type ProtocolKind,
} from "@/data/types";
import {
  collectExportRequests,
  collectReferencedModels,
  type ExportRequest,
} from "@/lib/exporters";
import { ExportDialog } from "@/components/export/ExportDialog";
import { GrpcImportDialog } from "@/components/modules/interface/GrpcImportDialog";
import { findItemInCollections, descendantIds } from "./treeUtils";
import { TreeContextMenu } from "./TreeContextMenu";
import { TreeItem } from "./TreeItem";
import { GrpcTree } from "./GrpcTree";
import { CollectionDialog } from "./CollectionDialog";
import { NameTooltip } from "./NameTooltip";
import type {
  CtxOpen,
  CtxPayload,
  DropTarget,
  ExportPayload,
  RenameState,
} from "./types";

export function CollectionTree() {
  const collections = useAppStore((s) => s.collections);
  const requests = useAppStore((s) => s.requests);
  const addCollection = useAppStore((s) => s.addCollection);
  const addRequest = useAppStore((s) => s.addRequestToCollection);
  const addFolder = useAppStore((s) => s.addFolderToCollection);
  const activeCollectionId = useAppStore((s) => s.activeCollectionId);
  const setActiveCollection = useAppStore((s) => s.setActiveCollection);
  const setImportOpen = useAppStore((s) => s.setImportOpen);
  const openTab = useAppStore((s) => s.openTab);
  const openGrpcTab = useAppStore((s) => s.openGrpcTab);
  const activeGrpcNode = useAppStore((s) => s.activeGrpcNode);
  const setActiveGrpcNode = useAppStore((s) => s.setActiveGrpcNode);
  // Currently active request (the rpc row's selected state, linked to tabs)
  const activeTabId = useAppStore((s) => s.activeTabId);
  const tabs = useAppStore((s) => s.tabs);
  const activeRequestId =
    tabs.find((t) => t.id === activeTabId)?.requestId || undefined;
  const move = useAppStore((s) => s.moveCollectionItem);
  const sidebarTab = useAppStore((s) => s.sidebarTab);
  const setSidebarTab = useAppStore((s) => s.setSidebarTab);
  const models = useAppStore((s) => s.models);
  const addModel = useAppStore((s) => s.addModel);
  const removeModel = useAppStore((s) => s.removeModel);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const setActiveModel = useAppStore((s) => s.setActiveModel);
  const rootRef = useRef<HTMLDivElement>(null);
  const [ctx, setCtx] = useState<CtxPayload | null>(null);
  const [exportState, setExportState] = useState<{
    requests: ExportRequest[];
    models: DataModel[];
    title: string;
    collectionId: string;
    itemId?: string;
  } | null>(null);
  const [rename, setRename] = useState<RenameState>({
    open: false,
    title: "",
    initialValue: "",
    onConfirm: () => {},
  });
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);
  // gRPC collection import dialog: collectionId + the default import method
  const [grpcImport, setGrpcImport] = useState<{
    collectionId: string;
    tab: "proto" | "reflection";
  } | null>(null);
  const [colDialog, setColDialog] = useState(false);
  const { t } = useT();

  // Current default target collection (where new requests/folders land); on a connection collection "new request" means a new message for that protocol
  const activeCol = useMemo(
    () =>
      collections.find((c) => c.id === activeCollectionId) ??
      collections[0] ??
      null,
    [collections, activeCollectionId],
  );
  const activeColIsConn = isConnectionCollection(activeCol);

  // ── Folder / collection expansion state (managed centrally, supporting "expand all / collapse all") ──
  // Not recorded = expanded (keeping the original default-expanded behavior)
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const allCollapsibleIds = useCallback((): string[] => {
    const ids: string[] = [];
    const walk = (its: CollectionItem[]) => {
      for (const it of its) {
        if (it.type === "folder") {
          ids.push(it.id);
          walk(it.items);
        }
      }
    };
    for (const c of collections) {
      ids.push(c.id); // the collection level is collapsible too
      walk(c.items);
      if (c.kind === "grpc") {
        // grpc collection: package / service are collapsible
        for (const pkg of c.grpc?.packages ?? []) {
          ids.push(`pkg-${c.id}-${pkg.name}`);
          for (const svc of pkg.services) {
            ids.push(`svc-${c.id}-${pkg.name}-${svc.name}`);
          }
        }
      }
    }
    return ids;
  }, [collections]);
  const expandAll = useCallback(() => {
    setExpanded(
      Object.fromEntries(allCollapsibleIds().map((id) => [id, true])),
    );
  }, [allCollapsibleIds]);
  const collapseAll = useCallback(() => {
    setExpanded(
      Object.fromEntries(allCollapsibleIds().map((id) => [id, false])),
    );
  }, [allCollapsibleIds]);
  // collection: true=expanded, false=collapsed, unrecorded=default expanded (consistent with the render-side colOpen `!== false`).
  // When unrecorded a single click collapses it; otherwise it toggles.
  const toggleCollection = useCallback((id: string) => {
    setExpanded((p) => ({ ...p, [id]: p[id] !== false ? false : true }));
  }, []);
  // folder: true=expanded, false/unrecorded=collapsed (folders default to collapsed, consistent with TreeItem `=== true`).
  const toggleFolder = useCallback((id: string) => {
    setExpanded((p) => ({ ...p, [id]: p[id] === true ? false : true }));
  }, []);

  // ── Dragging (Pointer Events, matching AutomationModule: no HTML5 DnD dependency, so browser / Tauri behave alike) ──
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget>(null);
  const dragRef = useRef<{
    id: string;
    x: number;
    y: number;
    active: boolean;
    activate?: () => void;
  } | null>(null);
  const dropTargetRef = useRef<DropTarget>(null);
  const collectionsRef = useRef(collections);
  collectionsRef.current = collections;
  const moveRef = useRef(move);
  moveRef.current = move;

  const isRootDrop = dragId !== null && dropTarget?.id === null;

  const onRowPointerDown = useCallback(
    (id: string, e: React.PointerEvent, activate?: () => void) => {
      // Left-button dragging only; the right button belongs to the context menu
      if (e.button !== 0) return;
      // Interactive elements (dropdowns, buttons, inputs ...) keep native behavior and never start a drag
      const target = e.target as HTMLElement;
      if (
        target.closest("button, input, select, textarea, a, [role='menuitem']")
      )
        return;
      e.preventDefault();
      dragRef.current = {
        id,
        x: e.clientX,
        y: e.clientY,
        active: false,
        activate,
      };
      document.body.style.userSelect = "none";

      const onMove = (ev: PointerEvent) => {
        const d = dragRef.current;
        if (!d) return;
        if (!d.active) {
          const dist = Math.hypot(ev.clientX - d.x, ev.clientY - d.y);
          if (dist < 5) return; // drag threshold, avoiding accidental drags
          d.active = true;
          setDragId(d.id); // marked only once dragging really starts, avoiding a translucent flicker of the row on a plain click
        }
        // Locate the row directly under the pointer through every row's bounding rect (no reliance on event bubbling, reliable under Tauri too)
        const rows = Array.from(
          document.querySelectorAll<HTMLElement>("[data-tree-id]"),
        );
        let targetEl: HTMLElement | null = null;
        for (const el of rows) {
          const r = el.getBoundingClientRect();
          if (ev.clientY >= r.top && ev.clientY <= r.bottom) {
            targetEl = el;
            break;
          }
        }
        const rootEl = rootRef.current;
        const inRoot =
          !!rootEl &&
          (() => {
            const r = rootEl.getBoundingClientRect();
            return (
              ev.clientY >= r.top &&
              ev.clientY <= r.bottom &&
              ev.clientX >= r.left &&
              ev.clientX <= r.right
            );
          })();

        if (!targetEl) {
          // Not on a specific row but still inside the list area → drop at the end of the root
          if (inRoot) {
            dropTargetRef.current = { id: null, pos: "after" };
            setDropTarget({ id: null, pos: "after" });
          } else {
            dropTargetRef.current = null;
            setDropTarget(null);
          }
          return;
        }

        const tid = targetEl.getAttribute("data-tree-id")!;
        if (tid === d.id) {
          dropTargetRef.current = null;
          setDropTarget(null);
          return;
        }

        // Forbid dropping a folder into its own subtree (which would create a cycle / lose nodes)
        const srcItem = findItemInCollections(collectionsRef.current, d.id);
        if (
          srcItem?.type === "folder" &&
          descendantIds(srcItem).includes(tid)
        ) {
          dropTargetRef.current = null;
          setDropTarget(null);
          return;
        }

        const canNest = targetEl.getAttribute("data-can-nest") === "1";
        const r = targetEl.getBoundingClientRect();
        let pos: "before" | "after" | "inside";
        if (
          canNest &&
          ev.clientY > r.top + r.height * 0.3 &&
          ev.clientY < r.top + r.height * 0.7
        ) {
          pos = "inside";
        } else {
          pos = ev.clientY < r.top + r.height / 2 ? "before" : "after";
        }
        dropTargetRef.current = { id: tid, pos };
        setDropTarget({ id: tid, pos });
      };

      const onUp = () => {
        const d = dragRef.current;
        if (d) {
          if (d.active && dropTargetRef.current) {
            const dt = dropTargetRef.current;
            moveRef.current(d.id, dt.id, dt.pos);
          } else if (!d.active) {
            // Below the drag threshold → treated as a click and the original behavior runs (expand the folder / open the tab)
            d.activate?.();
          }
        }
        dragRef.current = null;
        dropTargetRef.current = null;
        setDragId(null);
        setDropTarget(null);
        document.body.style.userSelect = "";
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
      };

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [],
  );

  const openRename = (
    title: string,
    initialValue: string,
    onConfirm: (name: string) => void,
  ) => {
    setRename({ open: true, title, initialValue, onConfirm });
    setRenameValue(initialValue);
    setTimeout(() => renameInputRef.current?.select(), 50);
  };
  const commitRename = () => {
    const v = renameValue.trim();
    if (v) rename.onConfirm(v);
    setRename((r) => ({ ...r, open: false }));
  };
  const openCtx: CtxOpen = (e, p) => {
    e.preventDefault();
    setCtx({ ...p, x: e.clientX, y: e.clientY });
  };

  // Context-menu "Export": collect endpoints by target level and open the export dialog
  const handleExport = (payload: ExportPayload) => {
    const reqs = collectExportRequests(
      collections,
      requests,
      payload.collectionId,
      payload.item,
    );
    const referencedModels = collectReferencedModels(reqs, models);
    const title =
      payload.kind === "request" && payload.item?.type === "request"
        ? requests[payload.item.requestId]?.name || "export"
        : payload.item?.type === "folder"
          ? payload.item.name
          : collections.find((c) => c.id === payload.collectionId)?.name ||
            "export";
    setExportState({
      requests: reqs,
      models: referencedModels,
      title,
      collectionId: payload.collectionId,
      itemId: payload.item?.id,
    });
    setCtx(null);
  };

  const tabBtn = (active: boolean) =>
    cn(
      "rounded px-2 py-1 text-xs font-medium transition-colors",
      active
        ? "bg-accent/20 text-foreground"
        : "text-muted-foreground hover:text-foreground",
    );

  useEffect(() => {
    return () => {
      document.body.style.userSelect = "";
    };
  }, []);

  return (
    <div className="flex h-full w-full flex-col border-r border-border bg-card/40">
      <div className="flex items-center justify-between border-b border-border px-2 py-1.5">
        <div className="flex items-center gap-1">
          <button
            className={tabBtn(sidebarTab === "collections")}
            onClick={() => setSidebarTab("collections")}
          >
            {t("sidebar.collections")}
          </button>
          <button
            className={tabBtn(sidebarTab === "models")}
            onClick={() => setSidebarTab("models")}
          >
            {t("sidebar.models")}
          </button>
        </div>
        {sidebarTab === "collections" ? (
          <div className="flex items-center gap-0.5">
            <Button
              variant="ghost"
              size="icon-sm"
              title={t("common.expandAll")}
              onClick={expandAll}
            >
              <UnfoldVertical className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              title={t("common.collapseAll")}
              onClick={collapseAll}
            >
              <FoldVertical className="h-3.5 w-3.5" />
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  title={t("tree.newOrImport")}
                >
                  <Plus className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={() => setColDialog(true)}>
                  <Folder className="h-4 w-4" />
                  {t("sidebar.newCollection")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {activeColIsConn && activeCol ? (
                  <DropdownMenuItem
                    onClick={() =>
                      addRequest(
                        activeCol.id,
                        null,
                        activeCol.kind as ProtocolKind,
                      )
                    }
                  >
                    <Plus className="h-4 w-4" />
                    {t("sidebar.newMessage")}
                  </DropdownMenuItem>
                ) : (
                  <ProtocolNewMenu
                    label={t("sidebar.newRequest")}
                    onSelect={(p) => addRequest(activeCol?.id, null, p)}
                  />
                )}
                <DropdownMenuItem
                  onClick={() =>
                    addFolder(
                      activeCollectionId ?? collections[0]?.id,
                      t("sidebar.newFolder"),
                    )
                  }
                >
                  <Folder className="h-4 w-4" />
                  {t("sidebar.newFolder")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => setImportOpen(true)}>
                  <Download className="h-4 w-4" />
                  {t("sidebar.importPrompt")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        ) : (
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const id = addModel();
              setActiveModel(id);
            }}
          >
            {t("common.new")}
          </Button>
        )}
      </div>

      {sidebarTab === "collections" ? (
        <ScrollArea className="flex-1">
          <div
            ref={rootRef}
            className={cn(
              "relative min-h-full p-1.5",
              isRootDrop && "rounded-sm ring-1 ring-inset ring-primary/40",
            )}
          >
            {collections.map((c: Collection) => {
              const colOpen = expanded[c.id] !== false; // unrecorded defaults to expanded
              const hasItems = c.items.length > 0;
              const isActive = activeCollectionId === c.id;
              return (
                <div key={c.id} className="mb-2">
                  <div
                    className={cn(
                      "group flex cursor-pointer select-none items-center gap-1 rounded px-1 py-0.5 hover:bg-accent/10",
                      isActive && "bg-accent/15",
                    )}
                    onClick={() => {
                      setActiveCollection(c.id); // selected: the default target for new requests/folders
                      if (hasItems) toggleCollection(c.id);
                    }}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      setCtx({
                        x: e.clientX,
                        y: e.clientY,
                        kind: "collection",
                        collectionId: c.id,
                      });
                    }}
                  >
                    {hasItems ? (
                      colOpen ? (
                        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                      )
                    ) : (
                      <span className="h-3.5 w-3.5 shrink-0" />
                    )}
                    {/* Connection-type collection: protocol badge */}
                    {isConnectionCollection(c) && (
                      <span
                        className={cn(
                          "shrink-0 rounded bg-muted/60 px-1.5 py-px text-xs font-bold leading-tight",
                          protocolColor(c.kind ?? "http"),
                        )}
                      >
                        {PROTOCOL_LABEL[c.kind ?? ""] ?? c.kind}
                      </span>
                    )}
                    <NameTooltip
                      name={c.name}
                      className="flex-1 text-xs font-semibold text-foreground/90"
                    />
                    {/* Connection status dot: configured = green; connection type unconfigured = yellow */}
                    {isConnectionCollection(c) && (
                      <span
                        title={
                          c.connection?.url
                            ? t("collection.connectionReady")
                            : t("collection.connectionMissing")
                        }
                        className={cn(
                          "h-1.5 w-1.5 shrink-0 rounded-full",
                          c.connection?.url ? "bg-emerald-500" : "bg-amber-500",
                        )}
                      />
                    )}
                    {/* Inline ⋮ button: reuses the context menu (TreeContextMenu) — one implementation including multi-protocol new requests */}
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="h-5 w-5 shrink-0 opacity-0 group-hover:opacity-100"
                      onClick={(e) => {
                        e.stopPropagation();
                        setCtx({
                          x: e.clientX,
                          y: e.clientY,
                          kind: "collection",
                          collectionId: c.id,
                        });
                      }}
                    >
                      <MoreVertical className="h-3 w-3" />
                    </Button>
                  </div>
                  {colOpen &&
                    (c.kind === "grpc" ? (
                      <GrpcTree
                        collection={c}
                        requests={requests}
                        expanded={expanded}
                        onToggle={toggleFolder}
                        activeGrpcNode={activeGrpcNode}
                        activeRequestId={activeRequestId}
                        onSelectNode={(n) => {
                          // Highlight the grpc tree node and open a dedicated tab (package/service have their own tabs too)
                          setActiveGrpcNode({ collectionId: c.id, ...n });
                          openGrpcTab({
                            type: n.type,
                            collectionId: c.id,
                            nodeId: n.id,
                          });
                        }}
                        onOpenRpc={(requestId) => {
                          setActiveGrpcNode(null);
                          openTab(requestId);
                        }}
                        onPackageContextMenu={(e, packageName) => {
                          e.preventDefault();
                          e.stopPropagation();
                          setCtx({
                            x: e.clientX,
                            y: e.clientY,
                            kind: "grpc-package",
                            collectionId: c.id,
                            packageName,
                          });
                        }}
                      />
                    ) : (
                      c.items.map((it) => (
                        <TreeItem
                          key={it.id}
                          collectionId={c.id}
                          item={it}
                          depth={1}
                          guides={[]}
                          onItemContextMenu={openCtx}
                          dragId={dragId}
                          dropTarget={dropTarget}
                          onRowPointerDown={onRowPointerDown}
                          expanded={expanded}
                          onToggle={toggleFolder}
                        />
                      ))
                    ))}
                </div>
              );
            })}
          </div>
        </ScrollArea>
      ) : (
        <ScrollArea className="flex-1">
          <div className="p-1.5">
            {models.map((m) => (
              <div
                key={m.id}
                onClick={() => setActiveModel(m.id)}
                className={cn(
                  "group flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-sm hover:bg-accent/10",
                  activeModelId === m.id && "bg-accent/15 text-foreground",
                )}
              >
                <Boxes className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="flex min-w-0 flex-1 flex-col text-left">
                  <span className="truncate">{m.name}</span>
                  {m.description ? (
                    <span className="truncate text-xs text-muted-foreground">
                      {m.description}
                    </span>
                  ) : null}
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="opacity-0 group-hover:opacity-100 shrink-0"
                  onClick={(e) => {
                    e.stopPropagation();
                    removeModel(m.id);
                  }}
                  title={t("model.delete")}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
            {models.length === 0 && (
              <div className="px-2 py-6 text-center text-xs text-muted-foreground">
                {t("sidebar.noModels")}
              </div>
            )}
          </div>
        </ScrollArea>
      )}

      <TreeContextMenu
        ctx={ctx}
        onClose={() => setCtx(null)}
        onRename={openRename}
        onExport={handleExport}
        onGrpcImport={(collectionId, tab) => {
          setGrpcImport({ collectionId, tab });
          setCtx(null);
        }}
      />
      <ExportDialog
        open={exportState !== null}
        onOpenChange={(o) => {
          if (!o) setExportState(null);
        }}
        requests={exportState?.requests ?? []}
        models={exportState?.models ?? []}
        title={exportState?.title ?? ""}
        collectionId={exportState?.collectionId ?? ""}
        itemId={exportState?.itemId}
      />
      <GrpcImportDialog
        open={grpcImport !== null}
        collectionId={grpcImport?.collectionId ?? null}
        initialTab={grpcImport?.tab ?? "proto"}
        onOpenChange={(o) => {
          if (!o) setGrpcImport(null);
        }}
      />
      <CollectionDialog
        open={colDialog}
        onOpenChange={setColDialog}
        onConfirm={(name, kind, connection) => {
          const id = addCollection(name, kind, connection);
          if (id) setActiveCollection(id);
        }}
      />
      <Dialog
        open={rename.open}
        onOpenChange={(o) => {
          if (!o) setRename((r) => ({ ...r, open: false }));
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{rename.title}</DialogTitle>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <Label className="text-xs">{t("common.name" as any)}</Label>
            <Input
              ref={renameInputRef}
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitRename();
                if (e.key === "Escape")
                  setRename((r) => ({ ...r, open: false }));
              }}
              className="text-sm"
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setRename((r) => ({ ...r, open: false }))}
            >
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              onClick={commitRename}
              disabled={!renameValue.trim()}
            >
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
