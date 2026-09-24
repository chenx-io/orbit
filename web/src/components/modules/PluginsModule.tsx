// WASM plugin management backed by the real backend (orbit-server HTTP / Tauri commands).
// Features: list / scan a directory / load a .wasm component / unload / inspect dynamically registered protocols and codecs.
import { useEffect, useRef, useState } from "react";
import {
  Boxes,
  RefreshCw,
  FolderSearch,
  Upload,
  Trash2,
  Loader2,
  PackagePlus,
} from "lucide-react";
import { toast } from "sonner";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import type { PluginDescriptor } from "@/data/types";

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const r = reader.result as string;
      // Strip the data URL prefix (if any)
      const idx = r.indexOf(",");
      resolve(idx >= 0 ? r.slice(idx + 1) : r);
    };
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

function KindBadge({ kind }: { kind: string }) {
  return kind === "protocol" ? (
    <Badge variant="outline" className="border-primary/40 text-primary">
      protocol
    </Badge>
  ) : (
    <Badge variant="outline" className="border-fuchsia-500/40 text-fuchsia-500">
      codec
    </Badge>
  );
}

function StatusBadge({ status }: { status: string }) {
  if (status === "enabled") return <Badge variant="success">enabled</Badge>;
  if (status === "disabled") return <Badge variant="secondary">disabled</Badge>;
  return <Badge variant="destructive">error</Badge>;
}

export function PluginsModule() {
  const { t } = useT();
  const plugins = useAppStore((s) => s.plugins);
  const pluginsLoading = useAppStore((s) => s.pluginsLoading);
  const pluginsError = useAppStore((s) => s.pluginsError);
  const loadPlugins = useAppStore((s) => s.loadPlugins);
  const scanPlugins = useAppStore((s) => s.scanPlugins);
  const loadPlugin = useAppStore((s) => s.loadPlugin);
  const loadNativePlugin = useAppStore((s) => s.loadNativePlugin);
  const unloadPlugin = useAppStore((s) => s.unloadPlugin);
  const installZip = useAppStore((s) => s.installZip);
  const enablePlugin = useAppStore((s) => s.enablePlugin);
  const disablePlugin = useAppStore((s) => s.disablePlugin);

  const [scanDir, setScanDir] = useState("");
  const [scanning, setScanning] = useState(false);
  const [loading, setLoading] = useState(false);
  const [pluginId, setPluginId] = useState("");
  const [nativePath, setNativePath] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);
  const zipRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    loadPlugins();
  }, [loadPlugins]);

  const onScan = async () => {
    const dir = scanDir.trim();
    if (!dir) {
      toast.error(t("plugins.dirRequired"));
      return;
    }
    setScanning(true);
    try {
      const report = await scanPlugins(dir);
      const ok = report.loaded.length;
      const fail = report.failed.length;
      toast.success(
        `${t("plugins.scanDone")}: ${t("plugins.scanLoaded")} ${ok}, ${t("plugins.scanFailed")} ${fail}`,
      );
      if (fail > 0) {
        const first = report.failed[0];
        if (first) toast.error(`${first[0]}: ${first[1]}`);
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setScanning(false);
    }
  };

  const onPickWasm = async (file: File) => {
    setLoading(true);
    try {
      const id = pluginId.trim() || file.name.replace(/\.wasm$/i, "");
      const b64 = await fileToBase64(file);
      const result = await loadPlugin(id, b64);
      toast.success(
        `${t("plugins.loadDone")} (${result.kind}: ${result.capabilities.join(", ")})`,
      );
      setPluginId("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const onUnload = async (p: PluginDescriptor) => {
    try {
      await unloadPlugin(p.id);
      toast.success(`${t("plugins.unloadDone")}: ${p.id}`);
    } catch (e) {
      toast.error(String(e));
    }
  };

  const onLoadNative = async () => {
    const path = nativePath.trim();
    if (!path) {
      toast.error(t("plugins.nativePathRequired"));
      return;
    }
    const id =
      pluginId.trim() ||
      path
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.(dll|so|dylib)$/i, "") ||
      "native";
    setLoading(true);
    try {
      const result = await loadNativePlugin(id, path);
      const protocols = (result as { protocols?: string[] }).protocols ?? [];
      toast.success(
        `${t("plugins.nativeLoaded")}: ${id} (${protocols.join(", ") || t("plugins.noProtocols")})`,
      );
      setNativePath("");
      setPluginId("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  };

  const onPickZip = async (file: File) => {
    setLoading(true);
    try {
      const id = pluginId.trim() || file.name.replace(/\.zip$/i, "");
      const b64 = await fileToBase64(file);
      await installZip(id, b64);
      toast.success(`${t("plugins.installDone")}: ${id}`);
      setPluginId("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
      if (zipRef.current) zipRef.current.value = "";
    }
  };

  const onToggleEnabled = async (p: PluginDescriptor) => {
    try {
      if (p.status === "disabled") {
        await enablePlugin(p.id);
        toast.success(`${t("plugins.enabled")}: ${p.id}`);
      } else if (p.status === "enabled") {
        await disablePlugin(p.id);
        toast.success(`${t("plugins.disabled")}: ${p.id}`);
      }
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="h-full overflow-auto p-4">
      <div className="mb-4 flex items-center gap-2">
        <Boxes className="h-5 w-5 text-fuchsia-400" />
        <h2 className="text-lg font-semibold">{t("plugins.title")}</h2>
        <span className="text-xs text-muted-foreground">
          {t("plugins.subtitle")}
        </span>
        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={loadPlugins}
            disabled={pluginsLoading}
            title={t("plugins.refresh")}
          >
            <RefreshCw
              className={`h-3.5 w-3.5 ${pluginsLoading ? "animate-spin" : ""}`}
            />
          </Button>
        </div>
      </div>

      {/* Actions: scan a directory + load a component */}
      <div className="mb-4 flex flex-wrap items-center gap-2 rounded-lg border border-border bg-muted/30 p-3">
        <Input
          className="h-8 w-56 text-xs"
          placeholder={t("plugins.scanDirPlaceholder")}
          value={scanDir}
          onChange={(e) => setScanDir(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onScan()}
        />
        <Button
          size="sm"
          variant="outline"
          onClick={onScan}
          disabled={scanning}
        >
          {scanning ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <FolderSearch className="h-3.5 w-3.5" />
          )}
          {t("plugins.scan")}
        </Button>

        <div className="mx-1 h-5 w-px bg-border" />

        <Input
          className="h-8 w-44 text-xs"
          placeholder={t("plugins.idPlaceholder")}
          value={pluginId}
          onChange={(e) => setPluginId(e.target.value)}
        />
        <Button
          size="sm"
          onClick={() => fileRef.current?.click()}
          disabled={loading}
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Upload className="h-3.5 w-3.5" />
          )}
          {t("plugins.load")}
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".wasm"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) onPickWasm(file);
          }}
        />

        <div className="mx-1 h-5 w-px bg-border" />

        <Button
          size="sm"
          variant="outline"
          onClick={() => zipRef.current?.click()}
          disabled={loading}
          title={t("plugins.installZipHint")}
        >
          <PackagePlus className="h-3.5 w-3.5" />
          {t("plugins.installZip")}
        </Button>
        <input
          ref={zipRef}
          type="file"
          accept=".zip"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) onPickZip(file);
          }}
        />

        <div className="mx-1 h-5 w-px bg-border" />

        {/* Native (dynamic library) protocol plugin loading */}
        <Input
          className="h-8 w-56 text-xs font-mono"
          placeholder={t("plugins.nativePathPlaceholder")}
          value={nativePath}
          onChange={(e) => setNativePath(e.target.value)}
          title={t("plugins.nativePathHint")}
        />
        <Button
          size="sm"
          variant="outline"
          onClick={onLoadNative}
          disabled={loading}
          title={t("plugins.nativePathHint")}
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Boxes className="h-3.5 w-3.5" />
          )}
          {t("plugins.loadNative")}
        </Button>
      </div>

      {pluginsError && (
        <div className="mb-3 rounded-md border border-destructive/40 bg-destructive/5 p-2 text-xs text-destructive">
          {pluginsError}
        </div>
      )}

      {plugins.length === 0 && !pluginsLoading ? (
        <div className="flex h-40 flex-col items-center justify-center gap-1 rounded-lg border border-dashed text-xs text-muted-foreground">
          <Boxes className="h-6 w-6 opacity-40" />
          <span>{t("plugins.empty")}</span>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {plugins.map((p) => (
            <Card key={p.id} className="flex flex-col">
              <CardContent className="flex flex-1 flex-col gap-2 p-3">
                <div className="flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="truncate font-mono text-sm font-semibold">
                      {p.id}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">
                      {p.name || p.id} · v{p.version || "0.0.0"}
                    </div>
                  </div>
                  <div className="flex shrink-0 flex-col items-end gap-1">
                    <div className="flex gap-1">
                      <KindBadge kind={p.kind} />
                      <StatusBadge status={p.status} />
                    </div>
                  </div>
                </div>

                {p.description && (
                  <p className="line-clamp-2 text-xs text-muted-foreground">
                    {p.description}
                  </p>
                )}

                {p.error && (
                  <p className="line-clamp-3 rounded-md border border-destructive/30 bg-destructive/5 p-1.5 text-xs text-destructive">
                    {p.error}
                  </p>
                )}

                <div className="flex flex-wrap gap-1">
                  {p.protocols.map((pid) => (
                    <Badge
                      key={`p-${pid}`}
                      variant="outline"
                      className="font-mono text-xs text-primary"
                    >
                      {pid}
                    </Badge>
                  ))}
                  {p.codecs.map((c) => (
                    <Badge
                      key={`c-${c}`}
                      variant="outline"
                      className="font-mono text-xs text-fuchsia-500"
                    >
                      {c}
                    </Badge>
                  ))}
                </div>

                <div className="mt-auto flex items-center justify-between gap-2 pt-1">
                  {p.installedDir ? (
                    <label className="flex items-center gap-1.5 text-sm text-muted-foreground">
                      <Switch
                        checked={p.status === "enabled"}
                        disabled={p.status === "error"}
                        onCheckedChange={() => onToggleEnabled(p)}
                      />
                      {p.status === "enabled"
                        ? t("plugins.enabled")
                        : p.status === "disabled"
                          ? t("plugins.disabled")
                          : t("plugins.error")}
                    </label>
                  ) : (
                    <span className="text-xs text-muted-foreground/60">
                      {t("plugins.notInstalled")}
                    </span>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs text-destructive hover:text-destructive"
                    onClick={() => onUnload(p)}
                  >
                    <Trash2 className="h-3 w-3" />
                    {t("plugins.unload")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
