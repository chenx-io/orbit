// Tree row context menu: renders different actions by target kind (collection / folder / request).
import { useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import {
  ChevronRight,
  Download,
  FileCode2,
  Folder,
  Package as PackageIcon,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Settings2,
  Trash2,
} from "lucide-react";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import {
  cn,
  PROTOCOL_LABEL,
  PROTOCOL_OPTIONS,
  protocolColor,
} from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import {
  isConnectionCollection,
  type ConnectionConfig,
  type ProtocolKind,
} from "@/data/types";
import { ConnectionEditDialog } from "./ConnectionEditDialog";
import type { CtxPayload, ExportPayload } from "./types";

/** Top-level "+ New request", expanding the protocol submenu on hover/focus (for the custom-drawn menu).
 * Built-in protocols (PROTOCOL_OPTIONS) plus the dynamic protocol ids of loaded plugins. */
function NewRequestSubMenu({
  itemCls,
  onSelect,
}: {
  itemCls: string;
  onSelect: (protocol: string) => void;
}) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const pluginProtocols = useAppStore((s) => s.protocolIds);
  // Plugin protocols = the registered ids that are not built-in
  const extra = pluginProtocols.filter(
    (pid) => !PROTOCOL_OPTIONS.includes(pid as ProtocolKind),
  );
  return (
    <div
      className="relative"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <div className={itemCls} tabIndex={0} onFocus={() => setOpen(true)}>
        <Plus className="h-4 w-4" />
        <span className="flex-1">{t("sidebar.newRequest")}</span>
        <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
      </div>
      {open && (
        <div
          className="absolute left-full top-0 z-50 min-w-36 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md text-sm"
          onClick={(e) => e.stopPropagation()}
        >
          {PROTOCOL_OPTIONS.map((p) => (
            <div
              key={p}
              className={itemCls}
              onClick={(e) => {
                e.stopPropagation();
                onSelect(p);
              }}
            >
              <span
                className={cn(
                  "w-11 font-mono text-xs font-semibold",
                  protocolColor(p),
                )}
              >
                {p === "http" ? "HTTP" : (PROTOCOL_LABEL[p] ?? p)}
              </span>
              <span className="capitalize">{p}</span>
            </div>
          ))}
          {extra.length > 0 && (
            <>
              <div className="my-1 h-px bg-border" />
              {extra.map((pid) => (
                <div
                  key={pid}
                  className={itemCls}
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelect(pid);
                  }}
                >
                  <span className="w-11 font-mono text-xs font-semibold text-fuchsia-400">
                    PLG
                  </span>
                  <span className="font-mono text-xs">{pid}</span>
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}

export function TreeContextMenu({
  ctx,
  onClose,
  onRename,
  onExport,
  onGrpcImport,
}: {
  ctx: CtxPayload | null;
  onClose: () => void;
  onRename: (
    title: string,
    initial: string,
    cb: (name: string) => void,
  ) => void;
  onExport: (payload: ExportPayload) => void;
  /** Open the gRPC collection import dialog (importType picks the default tab: proto / reflection) */
  onGrpcImport: (
    collectionId: string,
    importType: "proto" | "reflection",
  ) => void;
}) {
  const requests = useAppStore((s) => s.requests);
  const collections = useAppStore((s) => s.collections);
  const removeItem = useAppStore((s) => s.removeItemFromCollection);
  const renameFolder = useAppStore((s) => s.renameFolderInCollection);
  const addRequest = useAppStore((s) => s.addRequestToCollection);
  const addFolder = useAppStore((s) => s.addFolderToCollection);
  const renameRequest = useAppStore((s) => s.renameRequest);
  const removeRequest = useAppStore((s) => s.removeRequest);
  const removeGrpcPackage = useAppStore((s) => s.removeGrpcPackage);
  const renameCollection = useAppStore((s) => s.renameCollection);
  const removeCollection = useAppStore((s) => s.removeCollection);
  const restoreDraft = useAppStore((s) => s.restoreDraft);
  const updateConnection = useAppStore((s) => s.updateConnection);
  const { t } = useT();

  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<
    | { kind: "request"; id: string }
    | { kind: "grpc-package"; collectionId: string; packageName: string }
    | null
  >(null);
  /** Collection whose connection is being edited (connectionId + current config) */
  const [connEdit, setConnEdit] = useState<{
    collectionId: string;
    config?: ConnectionConfig;
  } | null>(null);

  // Whether the target collection is a connection type (non-HTTP) — this decides the menu items (new message / edit connection)
  const ctxCol = ctx
    ? collections.find((c) => c.id === ctx.collectionId)
    : undefined;
  const ctxColIsConn = isConnectionCollection(ctxCol);

  useLayoutEffect(() => {
    if (!ctx || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    let left = ctx.x,
      top = ctx.y;
    if (left + r.width > window.innerWidth - 8)
      left = window.innerWidth - r.width - 8;
    if (top + r.height > window.innerHeight - 8)
      top = window.innerHeight - r.height - 8;
    if (left < 8) left = 8;
    if (top < 8) top = 8;
    setPos({ left, top });
  }, [ctx]);

  const close = () => onClose();
  useLayoutEffect(() => {
    if (!ctx) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("contextmenu", close, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx]);

  // The Dialog must still render while the connection editor is open (the menu is already closed);
  // the same goes for deleteConfirm: on a delete click ctx is already cleared by onClose (React batching), so
  // returning null here would unmount the confirmation dialog too, making it lag or never appear.
  if (!ctx && !connEdit && !deleteConfirm) return null;

  const folderItem = ctx?.item?.type === "folder" ? ctx.item : null;
  const reqItem = ctx?.item?.type === "request" ? ctx.item : null;

  const isGrpcCollection =
    ctx && ctx.kind === "collection"
      ? collections.find((c) => c.id === ctx.collectionId)?.kind === "grpc"
      : false;

  const menuCls =
    "z-50 min-w-40 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md text-sm";
  const itemCls =
    "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-accent hover:text-accent-foreground";
  const dangerCls =
    "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-destructive hover:bg-destructive/10";
  const run = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
    onClose();
  };

  return (
    <>
      {ctx && (
        <div
          ref={ref}
          className={menuCls}
          style={{
            position: "fixed",
            left: pos?.left ?? ctx.x,
            top: pos?.top ?? ctx.y,
          }}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          {ctx.kind === "collection" ? (
            <>
              {ctxColIsConn && ctxCol ? (
                <>
                  {/* Connection-type collection: create a message for that protocol (no protocol submenu); gRPC is excluded (import only) */}
                  {ctxCol.kind !== "grpc" && (
                    <div
                      className={itemCls}
                      onClick={run(() =>
                        addRequest(
                          ctx.collectionId,
                          null,
                          ctxCol.kind as ProtocolKind,
                        ),
                      )}
                    >
                      <Plus className="h-4 w-4" />
                      {t("sidebar.newMessage")}
                    </div>
                  )}
                  <div
                    className={itemCls}
                    onClick={run(() =>
                      setConnEdit({
                        collectionId: ctx.collectionId,
                        config: ctxCol.connection,
                      }),
                    )}
                  >
                    <Settings2 className="h-4 w-4" />
                    {t("collection.editConnection")}
                  </div>
                </>
              ) : (
                <NewRequestSubMenu
                  key={`${ctx.collectionId}-root`}
                  itemCls={itemCls}
                  onSelect={(p) => {
                    addRequest(ctx.collectionId, null, p);
                    onClose();
                  }}
                />
              )}
              <div
                className={itemCls}
                onClick={run(() =>
                  onRename(
                    t("sidebar.renameCollection"),
                    collections.find((c) => c.id === ctx.collectionId)?.name ??
                      "",
                    (n) => renameCollection(ctx.collectionId, n),
                  ),
                )}
              >
                <Pencil className="h-4 w-4" />
                {t("common.rename")}
              </div>
              <div
                className={itemCls}
                onClick={run(() =>
                  onExport({
                    kind: "collection",
                    collectionId: ctx.collectionId,
                  }),
                )}
              >
                <Download className="h-4 w-4" />
                {t("export.title")}
              </div>
              {isGrpcCollection && (
                <>
                  <div className="my-1 h-px bg-border" />
                  <div
                    className={itemCls}
                    onClick={run(() => onGrpcImport(ctx.collectionId, "proto"))}
                  >
                    <FileCode2 className="h-4 w-4 text-fuchsia-400" />
                    {t("tree.importProto")}
                  </div>
                  <div
                    className={itemCls}
                    onClick={run(() =>
                      onGrpcImport(ctx.collectionId, "reflection"),
                    )}
                  >
                    <RefreshCw className="h-4 w-4 text-fuchsia-400" />
                    {t("tree.importReflection")}
                  </div>
                </>
              )}
              <div className="my-1 h-px bg-border" />
              <div
                className={dangerCls}
                onClick={run(() => removeCollection(ctx.collectionId))}
              >
                <Trash2 className="h-4 w-4" />
                {t("common.delete")}
              </div>
            </>
          ) : ctx.kind === "grpc-package" ? (
            <>
              <div className="flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-sm">
                <PackageIcon className="h-4 w-4 shrink-0 text-fuchsia-400" />
                <span className="min-w-0 flex-1 truncate">
                  {ctx.packageName || "(default)"}
                </span>
              </div>
              <div className="my-1 h-px bg-border" />
              <div
                className={dangerCls}
                onClick={run(() =>
                  setDeleteConfirm({
                    kind: "grpc-package",
                    collectionId: ctx.collectionId,
                    packageName: ctx.packageName ?? "",
                  }),
                )}
              >
                <Trash2 className="h-4 w-4" />
                {t("common.delete")}
              </div>
            </>
          ) : ctx.kind === "folder" ? (
            <>
              <NewRequestSubMenu
                key={`${ctx.collectionId}-${folderItem!.id}`}
                itemCls={itemCls}
                onSelect={(p) => {
                  addRequest(ctx.collectionId, folderItem!.id, p);
                  onClose();
                }}
              />
              <div
                className={itemCls}
                onClick={run(() =>
                  addFolder(ctx.collectionId, t("sidebar.newFolder")),
                )}
              >
                <Folder className="h-4 w-4" />
                {t("sidebar.newFolder")}
              </div>
              <div className="my-1 h-px bg-border" />
              <div
                className={itemCls}
                onClick={run(() =>
                  onRename(t("sidebar.renameFolder"), folderItem!.name, (n) =>
                    renameFolder(ctx.collectionId, folderItem!.id, n),
                  ),
                )}
              >
                <Pencil className="h-4 w-4" />
                {t("common.rename")}
              </div>
              <div
                className={itemCls}
                onClick={run(() =>
                  onExport({
                    kind: "folder",
                    collectionId: ctx.collectionId,
                    item: folderItem!,
                  }),
                )}
              >
                <Download className="h-4 w-4" />
                {t("export.title")}
              </div>
              <div className="my-1 h-px bg-border" />
              <div
                className={dangerCls}
                onClick={run(() =>
                  removeItem(ctx.collectionId, folderItem!.id),
                )}
              >
                <Trash2 className="h-4 w-4" />
                {t("common.delete")}
              </div>
            </>
          ) : (
            <>
              <div
                className={itemCls}
                onClick={run(() =>
                  onRename(
                    t("sidebar.renameRequest"),
                    requests[reqItem!.requestId]?.name ?? "",
                    (n) => renameRequest(reqItem!.requestId, n),
                  ),
                )}
              >
                <Pencil className="h-4 w-4" />
                {t("common.rename")}
              </div>
              <div
                className={itemCls}
                onClick={run(() => restoreDraft(reqItem!.requestId))}
              >
                <RotateCcw className="h-4 w-4" />
                {t("common.restore")}
              </div>
              <div
                className={itemCls}
                onClick={run(() =>
                  onExport({
                    kind: "request",
                    collectionId: ctx.collectionId,
                    item: reqItem!,
                  }),
                )}
              >
                <Download className="h-4 w-4" />
                {t("export.title")}
              </div>
              <div className="my-1 h-px bg-border" />
              <div
                className={dangerCls}
                onClick={run(() =>
                  setDeleteConfirm({ kind: "request", id: reqItem!.requestId }),
                )}
              >
                <Trash2 className="h-4 w-4" />
                {t("common.delete")}
              </div>
            </>
          )}
        </div>
      )}
      <ConfirmDialog
        open={deleteConfirm !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteConfirm(null);
        }}
        message={
          deleteConfirm?.kind === "grpc-package"
            ? t("tree.confirmDeletePackage")
            : t("tree.confirmDelete")
        }
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (!deleteConfirm) return;
          if (deleteConfirm.kind === "request") removeRequest(deleteConfirm.id);
          else
            removeGrpcPackage(
              deleteConfirm.collectionId,
              deleteConfirm.packageName,
            );
        }}
      />
      <ConnectionEditDialog
        open={connEdit !== null}
        initial={connEdit?.config}
        onOpenChange={(o) => {
          if (!o) setConnEdit(null);
        }}
        onConfirm={(config) => {
          if (connEdit) updateConnection(connEdit.collectionId, config);
        }}
      />
    </>
  );
}
