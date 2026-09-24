// Three-level tree rendering for grpc collections: package → service → rpc.
// A grpc collection stores its items in a flat structure (package/service/rpc nodes share one level),
// This component renders straight from collection.grpc.packages (the hierarchy source) and
// resolves each rpc's requestId from the grpc-rpc nodes in items.
import {
  ArrowDownToLine,
  ArrowRightLeft,
  ArrowUpFromLine,
  ChevronDown,
  ChevronRight,
  Package as PackageIcon,
  Repeat,
  Server,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type {
  ApiRequest,
  Collection,
  CollectionItem,
  GrpcRequest,
} from "@/data/types";
import { INDENT } from "./types";
import { NameTooltip } from "./NameTooltip";

/** rpc streaming mode: unary / server_streaming / client_streaming / bidirectional */
function rpcStreamMode(rpc: {
  clientStreaming: boolean;
  serverStreaming: boolean;
}): string {
  if (rpc.clientStreaming && rpc.serverStreaming) return "bidirectional";
  if (rpc.clientStreaming) return "client_streaming";
  if (rpc.serverStreaming) return "server_streaming";
  return "unary";
}

/** Icon and hint per streaming mode (four modes, four icons) */
function StreamIcon({ mode }: { mode: string }) {
  const cls = "h-3.5 w-3.5 shrink-0";
  switch (mode) {
    case "server_streaming":
      return (
        <span title="Server Streaming">
          <ArrowDownToLine className={`${cls} text-emerald-400`} />
        </span>
      );
    case "client_streaming":
      return (
        <span title="Client Streaming">
          <ArrowUpFromLine className={`${cls} text-blue-400`} />
        </span>
      );
    case "bidirectional":
      return (
        <span title="Bidirectional">
          <Repeat className={`${cls} text-amber-400`} />
        </span>
      );
    default:
      return (
        <span title="Unary">
          <ArrowRightLeft className={`${cls} text-muted-foreground`} />
        </span>
      );
  }
}

/** Resolve a stable key (package/service/method) → requestId from the collection's flat items */
function buildRequestIdByKey(
  collection: Collection,
  requests: Record<string, ApiRequest>,
): Map<string, string> {
  const map = new Map<string, string>();
  const walk = (its: CollectionItem[]) => {
    for (const it of its) {
      if (it.type === "grpc-rpc") {
        const req = requests[it.requestId];
        if (req?.protocol === "grpc" && "packageName" in req) {
          const key = `${req.packageName ?? ""}/${req.serviceName ?? ""}/${req.method ?? ""}`;
          map.set(key, it.requestId);
        }
      } else if (it.type === "folder") {
        walk(it.items);
      }
    }
  };
  walk(collection.items);
  return map;
}

export function GrpcTree({
  collection,
  requests,
  expanded,
  onToggle,
  activeGrpcNode,
  onSelectNode,
  onOpenRpc,
  onPackageContextMenu,
  activeRequestId,
}: {
  collection: Collection;
  requests: Record<string, ApiRequest>;
  /** Top-level unified expansion state (unrecorded = expanded, consistent with CollectionTree) */
  expanded: Record<string, boolean>;
  onToggle: (id: string) => void;
  activeGrpcNode: { type: "grpc-package" | "grpc-service"; id: string } | null;
  onSelectNode: (n: {
    type: "grpc-package" | "grpc-service";
    id: string;
  }) => void;
  onOpenRpc: (requestId: string) => void;
  /** Context menu for the package row */
  onPackageContextMenu?: (e: React.MouseEvent, packageName: string) => void;
  /** Currently active request id (the rpc row's selected state, linked to tabs) */
  activeRequestId?: string;
}) {
  const { t } = useT();
  const packages = collection.grpc?.packages ?? [];
  const requestIdByKey = buildRequestIdByKey(collection, requests);
  // The grpc-rpc nodes in items (build order = the packages → services → methods traversal order, matching the rendering below)
  const rpcNodes = collection.items.filter((it) => it.type === "grpc-rpc");
  let rpcNodeIdx = 0; // the rpc node index while rendering (corresponding to rpcNodes in order)
  if (packages.length === 0) {
    return (
      <div className="px-2 py-4 text-center text-xs text-muted-foreground">
        {t("grpc.empty")}
      </div>
    );
  }

  return (
    <div>
      {packages.map((pkg) => {
        const pkgId = `pkg-${collection.id}-${pkg.name}`;
        const pkgOpen = expanded[pkgId] !== false; // unrecorded defaults to expanded
        return (
          <div key={pkgId}>
            {" "}
            {/* package row */}
            <GrpcNodeRow
              id={pkgId}
              depth={1}
              isLeaf={pkg.services.length === 0}
              open={pkgOpen}
              active={
                activeGrpcNode?.id === pkgId &&
                activeGrpcNode.type === "grpc-package"
              }
              onClick={() => {
                onToggle(pkgId);
                onSelectNode({ type: "grpc-package", id: pkgId });
              }}
              onContextMenu={
                onPackageContextMenu
                  ? (e) => onPackageContextMenu(e, pkg.name)
                  : undefined
              }
            >
              <PackageIcon className="h-4 w-4 shrink-0 text-fuchsia-400" />
              <NameTooltip
                name={pkg.name || "(default)"}
                className="flex-1 font-medium"
              />
            </GrpcNodeRow>
            {pkgOpen &&
              pkg.services.map((svc) => {
                const svcId = `svc-${collection.id}-${pkg.name}-${svc.name}`;
                const svcOpen = expanded[svcId] !== false;
                return (
                  <div key={svcId}>
                    {/* service row */}
                    <GrpcNodeRow
                      id={svcId}
                      depth={2}
                      isLeaf={svc.methods.length === 0}
                      open={svcOpen}
                      active={
                        activeGrpcNode?.id === svcId &&
                        activeGrpcNode.type === "grpc-service"
                      }
                      onClick={() => {
                        onToggle(svcId);
                        onSelectNode({ type: "grpc-service", id: svcId });
                      }}
                    >
                      <Server className="h-3.5 w-3.5 shrink-0 text-blue-400" />
                      <NameTooltip name={svc.name} className="flex-1" />
                    </GrpcNodeRow>
                    {svcOpen &&
                      svc.methods.map((rpc) => {
                        const key = `${pkg.name}/${svc.name}/${rpc.name}`;
                        // Corresponds to the grpc-rpc nodes in items by build order (buildGrpcTree traverses the same way)
                        const rpcNode = rpcNodes[rpcNodeIdx] ?? null;
                        rpcNodeIdx++;
                        // requestId priority: (1) match by key (items node + a requests check)
                        // (2) the items node's original requestId (possibly stale) (3) a global (package, service, method) match
                        let requestId: string | undefined =
                          requestIdByKey.get(key) ?? rpcNode?.requestId;
                        if (!requestId) {
                          requestId = Object.values(requests).find(
                            (r): r is GrpcRequest =>
                              r.protocol === "grpc" &&
                              "packageName" in r &&
                              (r.packageName ?? "") === pkg.name &&
                              (r.serviceName ?? "") === svc.name &&
                              (r.method ?? "") === rpc.name,
                          )?.id;
                        }
                        return (
                          <GrpcNodeRow
                            key={`rpc-${key}`}
                            id={`rpc-${key}`}
                            depth={3}
                            isLeaf
                            active={
                              !!requestId && requestId === activeRequestId
                            }
                            onClick={() => requestId && onOpenRpc(requestId)}
                          >
                            {/* A streaming-mode icon replaces the left icon: four streaming modes, four icons */}
                            <StreamIcon mode={rpcStreamMode(rpc)} />
                            <NameTooltip name={rpc.name} className="flex-1" />
                          </GrpcNodeRow>
                        );
                      })}
                  </div>
                );
              })}
          </div>
        );
      })}
    </div>
  );
}

/** One grpc tree row (with indentation, an expand arrow, the selected state and hover) */
function GrpcNodeRow({
  id,
  depth,
  isLeaf,
  open,
  active,
  onClick,
  onContextMenu,
  children,
}: {
  id: string;
  depth: number;
  isLeaf: boolean;
  open?: boolean;
  active?: boolean;
  onClick: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  children: React.ReactNode;
}) {
  const pl = depth * INDENT;
  return (
    <div
      data-tree-id={id}
      data-can-nest="0"
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      onContextMenu={(e) => {
        if (!onContextMenu) return;
        e.preventDefault();
        e.stopPropagation();
        onContextMenu(e);
      }}
      className={cn(
        "group flex cursor-pointer select-none items-center gap-1.5 rounded py-1 pr-1 text-sm",
        // Consistent with the request tree: the selected row uses the primary background with its own hover override (so a hover variant cannot cover it)
        active
          ? "bg-primary/15 font-medium text-foreground hover:bg-primary/25"
          : "hover:bg-accent/10",
      )}
      style={{ paddingLeft: pl + 4, touchAction: "none" }}
    >
      {isLeaf ? (
        <span className="h-3.5 w-3.5 shrink-0" />
      ) : open ? (
        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      ) : (
        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      )}
      {children}
    </div>
  );
}
