import { useState, type ReactNode } from "react";
import {
  Boxes,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Folder,
  FolderOpen,
  Globe,
  Package,
  UnfoldVertical,
  FoldVertical,
} from "lucide-react";
import { cn, methodColor, protocolColor, PROTOCOL_LABEL } from "@/lib/utils";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import {
  isHttpRequest,
  requestProtocol,
  type Collection,
  type CollectionItem,
  type ProtocolKind,
} from "@/data/types";

const INDENT = 14;

interface Props {
  /** Currently selected requestId */
  value: string;
  onChange: (requestId: string) => void;
  placeholder?: string;
  /** Trigger button styling (width, height, etc.) */
  className?: string;
  align?: "start" | "center" | "end";
  /** Show only requests of the given protocol (e.g. HTTP only for load tests) */
  filterProtocol?: ProtocolKind;
}

/** Request tree picker: shows every endpoint as a "collection → folder → request" tree with expand/collapse and scrolling.
 *  Used as the request dropdown in load tests / automation (replacing a flat Select, which gets messy and unscrollable with many endpoints). */
export function RequestTreeSelect({
  value,
  onChange,
  placeholder,
  className,
  align = "start",
  filterProtocol,
}: Props) {
  const { t } = useT();
  const collections = useAppStore((s) => s.collections);
  const requests = useAppStore((s) => s.requests);
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const selected = value ? requests[value] : undefined;

  // Reset everything to collapsed on open (collections/folders start collapsed, keeping long lists tidy)
  const handleOpenChange = (v: boolean) => {
    setOpen(v);
    if (!v) return;
    setExpanded({});
  };

  const toggle = (id: string) => setExpanded((p) => ({ ...p, [id]: !p[id] }));

  // Collect every expandable node id (collections + folders + grpc package/service)
  const collectIds = (): string[] => {
    const ids: string[] = [];
    const walk = (items: CollectionItem[]) => {
      for (const it of items) {
        if (it.type === "folder") {
          ids.push(it.id);
          walk(it.items);
        } else if (it.type === "grpc-package" || it.type === "grpc-service") {
          ids.push(it.id);
        }
      }
    };
    for (const col of collections) {
      ids.push(col.id);
      walk(col.items);
    }
    return ids;
  };
  const expandAll = () =>
    setExpanded(Object.fromEntries(collectIds().map((id) => [id, true])));
  const collapseAll = () =>
    setExpanded(Object.fromEntries(collectIds().map((id) => [id, false])));

  const renderRequest = (item: CollectionItem, depth: number) => {
    if (item.type !== "request") return null;
    const req = requests[item.requestId];
    if (!req) return null;
    const protocol = requestProtocol(req);
    if (filterProtocol && protocol !== filterProtocol) return null;
    const isHttp = isHttpRequest(req);
    const label = isHttp ? req.method : (PROTOCOL_LABEL[protocol] ?? protocol);
    const color = isHttp ? methodColor(req.method) : protocolColor(protocol);
    return (
      <button
        key={item.id}
        type="button"
        onClick={() => {
          onChange(item.requestId);
          setOpen(false);
        }}
        className={cn(
          "flex w-full items-center gap-1.5 rounded py-1 pr-2 text-left text-xs hover:bg-accent/15",
          value === item.requestId && "bg-accent/20",
        )}
        style={{ paddingLeft: depth * INDENT + 8 }}
      >
        <span className={cn("w-11 shrink-0 text-xs font-semibold", color)}>
          {label.toUpperCase()}
        </span>
        <span className="min-w-0 flex-1 truncate">{req.name}</span>
      </button>
    );
  };

  const renderFolder = (item: CollectionItem, depth: number) => {
    if (item.type !== "folder") return null;
    const isOpen = !!expanded[item.id];
    return (
      <div key={item.id}>
        <button
          type="button"
          onClick={() => toggle(item.id)}
          className="flex w-full items-center gap-1 rounded py-1 pr-2 text-left text-xs text-muted-foreground hover:bg-accent/10"
          style={{ paddingLeft: depth * INDENT + 4 }}
        >
          {isOpen ? (
            <ChevronDown className="h-3 w-3 shrink-0" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0" />
          )}
          {isOpen ? (
            <FolderOpen className="h-3.5 w-3.5 shrink-0 text-amber-400" />
          ) : (
            <Folder className="h-3.5 w-3.5 shrink-0 text-amber-400" />
          )}
          <span className="min-w-0 flex-1 truncate">{item.name}</span>
        </button>
        {isOpen &&
          item.items.map((c) =>
            c.type === "folder"
              ? renderFolder(c, depth + 1)
              : renderRequest(c, depth + 1),
          )}
      </div>
    );
  };

  // ─── gRPC collection (items are a flat ordered package → service → rpc list) ───
  const renderGrpcRpc = (item: CollectionItem, depth: number) => {
    if (item.type !== "grpc-rpc" || !item.requestId) return null;
    const req = requests[item.requestId];
    if (!req) return null;
    const protocol = requestProtocol(req);
    if (filterProtocol && protocol !== filterProtocol) return null;
    return (
      <button
        key={item.id}
        type="button"
        onClick={() => {
          onChange(item.requestId);
          setOpen(false);
        }}
        className={cn(
          "flex w-full items-center gap-1.5 rounded py-1 pr-2 text-left text-xs hover:bg-accent/15",
          value === item.requestId && "bg-accent/20",
        )}
        style={{ paddingLeft: depth * INDENT + 8 }}
      >
        <span
          className={cn(
            "w-11 shrink-0 text-xs font-semibold",
            protocolColor("grpc"),
          )}
        >
          RPC
        </span>
        <span className="min-w-0 flex-1 truncate">{req.name}</span>
      </button>
    );
  };

  const renderGrpcTree = (col: Collection) => {
    const items = col.items;
    const out: ReactNode[] = [];
    // Start index of the next package (skipping service/rpc)
    const nextPkg = (from: number): number => {
      for (let i = from; i < items.length; i++) {
        if (items[i].type === "grpc-package") return i;
      }
      return items.length;
    };
    for (let i = 0; i < items.length; i++) {
      const pkg = items[i];
      if (pkg.type !== "grpc-package") continue;
      const pkgEnd = nextPkg(i + 1);
      const pkgOpen = !!expanded[pkg.id];
      out.push(
        <div key={pkg.id}>
          <button
            type="button"
            onClick={() => toggle(pkg.id)}
            className="flex w-full items-center gap-1 rounded py-1 pr-2 text-left text-xs font-medium text-muted-foreground hover:bg-accent/10"
            style={{ paddingLeft: 4 }}
          >
            {pkgOpen ? (
              <ChevronDown className="h-3 w-3 shrink-0" />
            ) : (
              <ChevronRight className="h-3 w-3 shrink-0" />
            )}
            <Package className="h-3.5 w-3.5 shrink-0 text-blue-400" />
            <span className="min-w-0 flex-1 truncate">{pkg.name}</span>
          </button>
          {pkgOpen && (
            <div>
              {items.slice(i + 1, pkgEnd).map((svc, idx) => {
                if (svc.type !== "grpc-service") return null;
                const svcOpen = !!expanded[svc.id];
                // This service's rpcs: the consecutive grpc-rpc nodes after svc
                const svcPos = i + 1 + idx;
                const rpcs: CollectionItem[] = [];
                for (
                  let k = svcPos + 1;
                  k < pkgEnd && items[k].type === "grpc-rpc";
                  k++
                ) {
                  rpcs.push(items[k]);
                }
                return (
                  <div key={svc.id}>
                    <button
                      type="button"
                      onClick={() => toggle(svc.id)}
                      className="flex w-full items-center gap-1 rounded py-1 pr-2 text-left text-xs text-muted-foreground hover:bg-accent/10"
                      style={{ paddingLeft: INDENT + 4 }}
                    >
                      {svcOpen ? (
                        <ChevronDown className="h-3 w-3 shrink-0" />
                      ) : (
                        <ChevronRight className="h-3 w-3 shrink-0" />
                      )}
                      <Boxes className="h-3.5 w-3.5 shrink-0 text-indigo-400" />
                      <span className="min-w-0 flex-1 truncate">
                        {svc.name}
                      </span>
                    </button>
                    {svcOpen && (
                      <div>{rpcs.map((rpc) => renderGrpcRpc(rpc, 3))}</div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>,
      );
      i = pkgEnd - 1;
    }
    return out;
  };

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            "flex h-8 items-center justify-between gap-1 rounded-md border border-input bg-background px-3 text-sm shadow-sm outline-none transition-colors hover:bg-accent/10 focus-visible:ring-2 focus-visible:ring-ring",
            className,
          )}
        >
          <span
            className={cn(
              "min-w-0 flex-1 truncate text-left",
              !selected && "text-muted-foreground",
            )}
          >
            {selected
              ? `${(isHttpRequest(selected) ? selected.method : (PROTOCOL_LABEL[requestProtocol(selected)] ?? requestProtocol(selected))).toUpperCase()} · ${selected.name}`
              : (placeholder ?? t("common.selectRequest"))}
          </span>
          <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>
      <PopoverContent align={align} className="w-72 p-1">
        {/* Toolbar: expand all / collapse all */}
        <div className="mb-1 flex items-center justify-end gap-0.5 border-b border-border px-1 pb-1">
          <button
            type="button"
            title={t("common.expandAll")}
            onClick={expandAll}
            className="rounded p-1 text-muted-foreground hover:bg-accent/15 hover:text-foreground"
          >
            <UnfoldVertical className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            title={t("common.collapseAll")}
            onClick={collapseAll}
            className="rounded p-1 text-muted-foreground hover:bg-accent/15 hover:text-foreground"
          >
            <FoldVertical className="h-3.5 w-3.5" />
          </button>
        </div>
        <div className="max-h-72 overflow-y-auto">
          {collections.length === 0 ? (
            <div className="px-2 py-3 text-center text-xs text-muted-foreground">
              {t("common.noRequests")}
            </div>
          ) : (
            collections.map((col) => {
              const isOpen = !!expanded[col.id];
              const isGrpc = col.kind === "grpc";
              return (
                <div key={col.id}>
                  <button
                    type="button"
                    onClick={() => toggle(col.id)}
                    className="flex w-full items-center gap-1 rounded py-1 pr-2 text-left text-xs font-medium hover:bg-accent/10"
                    style={{ paddingLeft: 4 }}
                  >
                    {isOpen ? (
                      <ChevronDown className="h-3 w-3 shrink-0" />
                    ) : (
                      <ChevronRight className="h-3 w-3 shrink-0" />
                    )}
                    <Globe className="h-3.5 w-3.5 shrink-0 text-primary" />
                    <span className="min-w-0 flex-1 truncate">{col.name}</span>
                  </button>
                  {isOpen &&
                    (isGrpc
                      ? renderGrpcTree(col)
                      : col.items.map((c) =>
                          c.type === "folder"
                            ? renderFolder(c, 1)
                            : renderRequest(c, 1),
                        ))}
                </div>
              );
            })
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
