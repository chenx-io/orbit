// Scenario tree sidebar: search + new dropdown + tree rendering (folder collapse / case priority badge / hover run button / drag to move).
// Dragging uses custom Pointer Events (same approach as the step editor); the "inside" drop target is folders only, with cycle prevention.
import { useMemo, useRef, useState } from "react";
import {
  Download,
  FileCode2,
  Folder,
  FolderOpen,
  Loader2,
  Pencil,
  Play,
  Plus,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { ScenarioPriority } from "@/data/types";
import { ContextMenu } from "./ContextMenu";
import { RenameDialog } from "./RenameDialog";
import { NewCaseDialog } from "./NewCaseDialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  treeFolderIcon,
  treeLeafIcon,
  treeMeta,
} from "@/components/common/treeStyle";
import { TreeRow } from "@/components/common/Tree";
import { PriorityBadge } from "./PriorityBadge";
import {
  buildScenarioTree,
  isDescendantFolder,
  type DropPos,
  type DropTarget,
  type ScenarioTreeNode,
} from "./treeUtils";

interface DialogState {
  type: "newCase" | "newFolder" | "renameFolder" | "renameCase";
  id?: string;
  name?: string;
  parentId?: string | null;
}

export function ScenarioTree() {
  const { t } = useT();
  const scenarios = useAppStore((s) => s.scenarios);
  const folders = useAppStore((s) => s.scenarioFolders);
  const activeId = useAppStore((s) => s.activeScenarioId);
  const dirty = useAppStore((s) => s.dirtyScenarios);
  const running = useAppStore((s) => s.scenarioRunning);
  const setActive = useAppStore((s) => s.setActiveScenario);
  const addScenario = useAppStore((s) => s.addScenario);
  const updateScenario = useAppStore((s) => s.updateScenario);
  const removeScenario = useAppStore((s) => s.removeScenario);
  const addFolder = useAppStore((s) => s.addFolder);
  const updateFolder = useAppStore((s) => s.updateFolder);
  const removeFolder = useAppStore((s) => s.removeFolder);
  const setFolderCollapsed = useAppStore((s) => s.setFolderCollapsed);
  const moveScenario = useAppStore((s) => s.moveScenario);
  const runTarget = useAppStore((s) => s.runTarget);
  const setImportOpen = useAppStore((s) => s.setScenarioImportOpen);

  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [deleteFolderId, setDeleteFolderId] = useState<string | null>(null);
  const [deleteCaseId, setDeleteCaseId] = useState<string | null>(null);

  // Drag state
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  const dragRef = useRef<{
    id: string;
    kind: "folder" | "case";
    x: number;
    y: number;
    active: boolean;
  } | null>(null);
  const dropRef = useRef<DropTarget | null>(null);

  const nodes = useMemo(
    () => buildScenarioTree(scenarios, folders, query),
    [scenarios, folders, query],
  );

  const isNodeRunning = (node: ScenarioTreeNode): boolean =>
    running === `${node.kind}:${node.id}`;

  // ── Dragging ──
  const onRowPointerDown = (
    node: ScenarioTreeNode,
    e: React.PointerEvent,
  ): void => {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement;
    if (el.closest("button, input, select, a, [role='menuitem']")) return;
    e.preventDefault();
    dragRef.current = {
      id: node.id,
      kind: node.kind,
      x: e.clientX,
      y: e.clientY,
      active: false,
    };
    document.body.style.userSelect = "none";

    const onMove = (ev: PointerEvent): void => {
      const d = dragRef.current;
      if (!d) return;
      if (!d.active) {
        if (Math.hypot(ev.clientX - d.x, ev.clientY - d.y) < 5) return;
        d.active = true;
        setDragId(d.id);
      }
      const rows = Array.from(
        document.querySelectorAll<HTMLElement>("[data-node-id]"),
      );
      let targetEl: HTMLElement | null = null;
      for (const r of rows) {
        const rect = r.getBoundingClientRect();
        if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
          targetEl = r;
          break;
        }
      }
      if (!targetEl) {
        dropRef.current = null;
        setDropTarget(null);
        return;
      }
      const tid = targetEl.getAttribute("data-node-id") ?? "";
      const tkind = (targetEl.getAttribute("data-node-kind") ?? "case") as
        "folder" | "case";
      if (tid === d.id) {
        dropRef.current = null;
        setDropTarget(null);
        return;
      }
      const rect = targetEl.getBoundingClientRect();
      const ratio = (ev.clientY - rect.top) / rect.height;
      let pos: DropPos;
      if (tkind === "folder" && ratio > 0.3 && ratio < 0.7) pos = "inside";
      else pos = ratio < 0.5 ? "before" : "after";
      dropRef.current = { id: tid, kind: tkind, pos };
      setDropTarget(dropRef.current);
    };

    const onUp = (): void => {
      const d = dragRef.current;
      const drop = dropRef.current;
      if (d && d.active && drop) applyDrop(d, drop);
      dragRef.current = null;
      dropRef.current = null;
      setDragId(null);
      setDropTarget(null);
      document.body.style.userSelect = "";
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const applyDrop = (
    d: { id: string; kind: "folder" | "case" },
    drop: DropTarget,
  ): void => {
    const targetNode = nodes.find((n) => n.id === drop.id);
    if (!targetNode) return;
    if (d.kind === "folder") {
      if (isDescendantFolder(folders, drop.id, d.id)) return; // Cannot drag a folder into itself / its own subtree
      if (drop.pos === "inside") {
        updateFolder(d.id, { parentId: drop.id });
      } else if (targetNode.kind === "folder") {
        updateFolder(d.id, { parentId: targetNode.parentId });
      } else {
        updateFolder(d.id, { parentId: targetNode.parentId });
      }
    } else {
      if (drop.pos === "inside") {
        if (targetNode.kind !== "folder") return;
        moveScenario(d.id, drop.id);
      } else if (targetNode.kind === "folder") {
        moveScenario(d.id, targetNode.parentId);
      } else {
        moveScenario(d.id, targetNode.parentId);
      }
    }
  };

  const confirmDialog = (name: string): void => {
    if (!dialog) return;
    if (dialog.type === "newFolder") {
      addFolder(name, dialog.parentId ?? null);
    } else if (dialog.type === "renameFolder" && dialog.id) {
      updateFolder(dialog.id, { name });
    } else if (dialog.type === "renameCase" && dialog.id) {
      updateScenario(dialog.id, { name });
    }
    setDialog(null);
  };

  const confirmNewCase = (name: string, priority: ScenarioPriority): void => {
    const id = addScenario(name, dialog?.parentId ?? null, priority);
    setActive(id);
    setDialog(null);
  };

  return (
    <div className="flex h-full flex-col">
      {/* Search + create */}
      <div className="flex shrink-0 items-center gap-1 border-b border-border p-2">
        <div className="relative flex-1">
          <Search className="absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            placeholder={t("scenario.searchCases")}
            className="h-8 pl-7 pr-7 text-xs"
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              className="shrink-0"
              title={t("scenario.newCase")}
            >
              <Plus className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-32">
            <DropdownMenuItem onClick={() => setDialog({ type: "newCase" })}>
              <Plus className="h-3.5 w-3.5" /> {t("scenario.newCase")}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setDialog({ type: "newFolder" })}>
              <Folder className="h-3.5 w-3.5" /> {t("scenario.newFolder")}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setImportOpen(true)}>
              <Download className="h-3.5 w-3.5" />{" "}
              {t("scenario.importScenario")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Tree */}
      <ScrollArea className="flex-1">
        <div className="p-1.5">
          {nodes.length === 0 && (
            <div className="px-2 py-8 text-center text-xs text-muted-foreground">
              {query ? t("scenario.noMatch") : t("scenario.empty")}
            </div>
          )}
          {nodes.map((node) => {
            const runningNode = isNodeRunning(node);
            const isDropTarget =
              dropTarget?.id === node.id && dragId !== node.id;
            const dropPos = isDropTarget ? dropTarget?.pos : null;
            return (
              <ContextMenu
                key={`${node.kind}:${node.id}`}
                items={
                  node.kind === "folder"
                    ? [
                        {
                          label: t("scenario.runFolder"),
                          icon: Play,
                          action: () =>
                            void runTarget({ type: "folder", id: node.id }),
                        },
                        {
                          label: t("scenario.newCase"),
                          icon: Plus,
                          action: () =>
                            setDialog({ type: "newCase", parentId: node.id }),
                        },
                        {
                          label: t("scenario.newFolder"),
                          icon: Folder,
                          action: () =>
                            setDialog({ type: "newFolder", parentId: node.id }),
                        },
                        {
                          label: t("scenario.rename"),
                          icon: Pencil,
                          action: () =>
                            setDialog({
                              type: "renameFolder",
                              id: node.id,
                              name: node.name,
                            }),
                        },
                        {
                          label: t("scenario.deleteFolder"),
                          icon: Trash2,
                          danger: true,
                          action: () => setDeleteFolderId(node.id),
                        },
                      ]
                    : [
                        {
                          label: t("scenario.runCase"),
                          icon: Play,
                          action: () =>
                            void runTarget({ type: "case", id: node.id }),
                        },
                        {
                          label: t("scenario.rename"),
                          icon: Pencil,
                          action: () =>
                            setDialog({
                              type: "renameCase",
                              id: node.id,
                              name: node.name,
                            }),
                        },
                        {
                          label: t("scenario.delete"),
                          icon: Trash2,
                          danger: true,
                          action: () => setDeleteCaseId(node.id),
                        },
                      ]
                }
              >
                <TreeRow
                  dataAttrs={{
                    "data-node-id": node.id,
                    "data-node-kind": node.kind,
                  }}
                  depth={node.depth}
                  selected={node.kind === "case" && activeId === node.id}
                  dragging={dragId === node.id}
                  dropPos={dropPos}
                  hasChildren={node.kind === "folder"}
                  collapsed={!!node.collapsed}
                  onToggle={() =>
                    setFolderCollapsed(node.id, !(node.collapsed ?? false))
                  }
                  onClick={() => {
                    if (node.kind === "folder")
                      setFolderCollapsed(node.id, !(node.collapsed ?? false));
                    else setActive(node.id);
                  }}
                  onPointerDown={(e) => onRowPointerDown(node, e)}
                  leading={
                    node.kind === "folder" ? (
                      node.collapsed ? (
                        <Folder className={treeFolderIcon} />
                      ) : (
                        <FolderOpen className={treeFolderIcon} />
                      )
                    ) : (
                      <FileCode2 className={treeLeafIcon} />
                    )
                  }
                  actions={
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="h-6 w-6"
                      title={
                        node.kind === "folder"
                          ? t("scenario.runFolder")
                          : t("scenario.runCase")
                      }
                      disabled={!!running}
                      onClick={(e) => {
                        e.stopPropagation();
                        void runTarget({ type: node.kind, id: node.id });
                      }}
                    >
                      {runningNode ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Play className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  }
                >
                  {node.kind === "folder" ? (
                    <>
                      <span className="min-w-0 flex-1 truncate">
                        {node.name}
                      </span>
                      <span className={treeMeta}>{node.caseCount}</span>
                    </>
                  ) : (
                    <>
                      <PriorityBadge priority={node.scenario?.priority} />
                      <span className="min-w-0 flex-1 truncate">
                        {node.name}
                      </span>
                      {dirty.has(node.id) && (
                        <span
                          className={cn(treeMeta, "leading-none text-warning")}
                          title={t("scenario.dirty")}
                        >
                          ●
                        </span>
                      )}
                    </>
                  )}
                </TreeRow>
              </ContextMenu>
            );
          })}
        </div>
      </ScrollArea>

      {/* New case dialog (name + priority) */}
      <NewCaseDialog
        open={dialog?.type === "newCase"}
        onConfirm={confirmNewCase}
        onCancel={() => setDialog(null)}
      />
      {/* New folder / rename dialog */}
      <RenameDialog
        open={dialog !== null && dialog.type !== "newCase"}
        name={dialog?.name ?? ""}
        onConfirm={confirmDialog}
        onCancel={() => setDialog(null)}
        t={t}
      />
      <ConfirmDialog
        open={deleteFolderId !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteFolderId(null);
        }}
        message={t("scenario.deleteFolder")}
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (deleteFolderId) removeFolder(deleteFolderId);
        }}
      />
      <ConfirmDialog
        open={deleteCaseId !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteCaseId(null);
        }}
        message={t("scenario.confirmDelete")}
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (deleteCaseId) removeScenario(deleteCaseId);
        }}
      />
    </div>
  );
}
