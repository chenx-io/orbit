// Service editor area: shows the package name + service name (read-only info card).
import { Server } from "lucide-react";
import { useAppStore } from "@/store/useStore";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { useT } from "@/lib/i18n";

export function GrpcServiceEditor({
  collectionId,
  id,
}: {
  collectionId: string;
  id: string;
}) {
  const { t } = useT();
  const collections = useAppStore((s) => s.collections);
  const col = collections.find((c) => c.id === collectionId);
  // id looks like `svc-${collectionId}-${packageName}-${serviceName}`; packageName may contain `-`,
  // so we reverse-match the longest package-name prefix from the collection metadata to avoid wrong truncation.
  const prefix = `svc-${collectionId}-`;
  const rest = id.startsWith(prefix) ? id.slice(prefix.length) : "";

  // Match the correct package + service (packageName may contain hyphens; try longest first)
  let packageName = "";
  let serviceName = rest;
  let svc: { name: string; methods: { name: string }[] } | undefined;
  for (const pkg of col?.grpc?.packages ?? []) {
    if (pkg.name && rest.startsWith(`${pkg.name}-`)) {
      const cand = rest.slice(pkg.name.length + 1);
      const match = pkg.services.find((s) => s.name === cand);
      if (match) {
        packageName = pkg.name;
        serviceName = cand;
        svc = match;
        break;
      }
    } else if (!pkg.name) {
      // Default package (no package name)
      const match = pkg.services.find((s) => s.name === rest);
      if (match) {
        packageName = "";
        serviceName = rest;
        svc = match;
        break;
      }
    }
  }

  return (
    <div className="flex h-full flex-col overflow-auto p-4">
      <div className="mb-4 flex items-center gap-2">
        <Server className="h-5 w-5 text-primary" />
        <h2 className="text-sm font-semibold">{serviceName || "service"}</h2>
        <Badge variant="outline" className="text-xs">
          service
        </Badge>
      </div>
      <div className="max-w-xl rounded-md border border-border bg-card/50">
        <div className="grid grid-cols-2 gap-0">
          <div className="border-b border-border p-3">
            <div className="text-xs text-muted-foreground">Package</div>
            <div className="mt-1 font-mono text-xs text-fuchsia-400">
              {packageName || "(default)"}
            </div>
          </div>
          <div className="border-b border-l border-border p-3">
            <div className="text-xs text-muted-foreground">Service</div>
            <div className="mt-1 font-mono text-xs text-primary">
              {serviceName}
            </div>
          </div>
        </div>
        {svc && (
          <>
            <Separator />
            <div className="p-3">
              <div className="text-xs text-muted-foreground">
                {t("grpc.rpcMethodCount")}
              </div>
              <div className="mt-1 text-xs">{svc.methods.length}</div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
