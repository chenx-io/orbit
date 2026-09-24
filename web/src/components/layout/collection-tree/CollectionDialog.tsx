// New collection dialog: pick a protocol (HTTP or a connection type); connection types require basic connection config (address/codec/timeout).
// Detailed connection config such as framing/TLS is set in the connection section of the M2 editor panel; only creation-required fields are here.
import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn, PROTOCOL_LABEL, protocolColor } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { createDefaultConnection, type ConnectionConfig } from "@/data/types";
import { useAppStore } from "@/store/useStore";
import { SchemaForm } from "@/components/common/SchemaForm";
import { ConnectionConfigForm } from "./ConnectionConfigForm";

/** Selectable connection-type collection protocols (gRPC only; websocket/tcp/udp/sse/graphql cannot be created yet);
 * plugin protocols are injected dynamically by protocolCatalog */
const CONNECTION_PROTOCOLS = ["grpc"];

export function CollectionDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onConfirm: (
    name: string,
    kind: string,
    connection?: ConnectionConfig,
  ) => void;
}) {
  const { t } = useT();
  const protocolCatalog = useAppStore((s) => s.protocolCatalog);
  const [name, setName] = useState("");
  const [kind, setKind] = useState<string>("http");
  const [conn, setConn] = useState<ConnectionConfig>({});

  // Plugin protocol ids (the non-built-in part of the catalog)
  const pluginProtocols = protocolCatalog
    .filter((p) => !p.builtin)
    .map((p) => p.id);
  // Connection parameter schema for the current protocol (plugin protocols)
  const connSchema =
    protocolCatalog.find((p) => p.id === kind)?.connectionConfigSchema ?? null;

  // Reset the form on every open
  useEffect(() => {
    if (open) {
      setName("");
      setKind("http");
      setConn({});
    }
  }, [open]);

  const isConn = kind !== "http";
  const isPluginProtocol = !["http", ...CONNECTION_PROTOCOLS].includes(kind);
  const canSubmit =
    name.trim().length > 0 &&
    (!isConn || (conn.url ?? "").trim().length > 0 || isPluginProtocol);

  const submit = () => {
    if (!canSubmit) return;
    onConfirm(name.trim(), kind, isConn ? conn : undefined);
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onOpenChange(false);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("sidebar.newCollection")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">
              {t("collection.name")}
            </Label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
                if (e.key === "Escape") onOpenChange(false);
              }}
              placeholder={t("collection.namePlaceholder")}
              className="text-sm"
              autoFocus
            />
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">
              {t("collection.protocol")}
            </Label>
            <Select
              value={kind}
              onValueChange={(v) => {
                setKind(v);
                setConn(createDefaultConnection(v));
              }}
            >
              <SelectTrigger className="text-sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="http">
                  <span className="font-mono text-xs font-semibold text-primary">
                    HTTP
                  </span>
                  <span className="ml-2">HTTP</span>
                </SelectItem>
                {CONNECTION_PROTOCOLS.map((p) => (
                  <SelectItem key={p} value={p}>
                    <span
                      className={cn(
                        "font-mono text-xs font-semibold",
                        protocolColor(p),
                      )}
                    >
                      {PROTOCOL_LABEL[p] ?? p}
                    </span>
                    <span className="ml-2 capitalize">{p}</span>
                  </SelectItem>
                ))}
                {pluginProtocols.map((pid) => (
                  <SelectItem key={pid} value={pid}>
                    <span className="font-mono text-xs font-semibold text-fuchsia-400">
                      PLG
                    </span>
                    <span className="ml-2 font-mono text-xs">{pid}</span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {isConn && (
              <p className="text-xs text-muted-foreground">
                {t("collection.connectionHint")}
              </p>
            )}
          </div>

          {isConn && !isPluginProtocol && (
            <ConnectionConfigForm value={conn} onChange={setConn} />
          )}
          {isConn && isPluginProtocol && connSchema && (
            <div className="rounded-md border border-border bg-muted/30 p-3">
              <SchemaForm
                schema={connSchema}
                value={conn as Record<string, unknown>}
                onChange={(v) => setConn(v as ConnectionConfig)}
              />
            </div>
          )}
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={submit} disabled={!canSubmit}>
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
