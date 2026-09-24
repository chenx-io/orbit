// Connection config form (the ConnectionConfig editor): shared by creating a connection collection and editing its config.
// Covers url / codec / timeoutMs / TLS (insecure_skip_verify / ca_cert / sni).
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import type { ConnectionConfig } from "@/data/types";

/** Built-in codecs (aligned with orbit-codec CodecKind); plugin codecs are injected dynamically */
const BUILTIN_CODECS = [
  "",
  "json",
  "yaml",
  "msgpack",
  "protobuf",
  "xml",
  "form",
  "binary",
];

export function ConnectionConfigForm({
  value,
  onChange,
}: {
  value: ConnectionConfig;
  onChange: (v: ConnectionConfig) => void;
}) {
  const { t } = useT();
  const codecCatalog = useAppStore((s) => s.codecCatalog);
  // Built-in + dynamic plugin codecs
  const codecOptions = [
    "",
    ...codecCatalog.filter((c) => !c.builtin).map((c) => c.name),
    ...BUILTIN_CODECS.filter((c) => c !== ""),
  ].filter((v, i, arr) => arr.indexOf(v) === i);
  const setField = (k: keyof ConnectionConfig, v: unknown) =>
    onChange({ ...value, [k]: v });
  const setTls = (k: keyof NonNullable<ConnectionConfig["tls"]>, v: unknown) =>
    onChange({ ...value, tls: { ...(value.tls ?? {}), [k]: v } });

  return (
    <div className="space-y-3 rounded-md border border-border bg-muted/30 p-3">
      <div className="space-y-1.5">
        <Label className="text-xs text-muted-foreground">
          {t("collection.connectionUrl")}
          <span className="ml-1 text-destructive">*</span>
        </Label>
        <Input
          value={value.url ?? ""}
          onChange={(e) => setField("url", e.target.value)}
          placeholder={t("collection.urlPlaceholder")}
          className="text-sm font-mono"
        />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label className="text-xs text-muted-foreground">
            {t("collection.codec")}
          </Label>
          <Select
            value={value.codec ?? ""}
            onValueChange={(v) => setField("codec", v || undefined)}
          >
            <SelectTrigger className="text-sm">
              <SelectValue placeholder={t("collection.codecNone")} />
            </SelectTrigger>
            <SelectContent>
              {codecOptions.map((c) => (
                <SelectItem key={c} value={c}>
                  {c === "" ? t("collection.codecNone") : c}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs text-muted-foreground">
            {t("collection.timeoutMs")}
          </Label>
          <Input
            type="number"
            value={value.timeoutMs ?? ""}
            onChange={(e) =>
              setField(
                "timeoutMs",
                e.target.value ? Number(e.target.value) : undefined,
              )
            }
            placeholder="30000"
            className="text-sm"
          />
        </div>
      </div>

      {/* TLS config */}
      <div className="space-y-2 rounded border border-border/60 bg-background/40 p-2">
        <Label className="text-xs font-medium text-foreground">
          {t("collection.tls")}
        </Label>
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">
            {t("collection.tlsSkipVerify")}
          </span>
          <Switch
            checked={value.tls?.insecureSkipVerify ?? false}
            onCheckedChange={(v) => setTls("insecureSkipVerify", v)}
          />
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs text-muted-foreground">
            {t("collection.tlsSni")}
          </Label>
          <Input
            value={value.tls?.sni ?? ""}
            onChange={(e) => setTls("sni", e.target.value || undefined)}
            placeholder="example.com"
            className="text-sm"
          />
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs text-muted-foreground">
            {t("collection.tlsCaCert")}
          </Label>
          <Textarea
            value={value.tls?.caCert ?? ""}
            onChange={(e) => setTls("caCert", e.target.value || undefined)}
            placeholder="-----BEGIN CERTIFICATE-----"
            className="h-16 text-xs font-mono"
          />
        </div>
      </div>
    </div>
  );
}
