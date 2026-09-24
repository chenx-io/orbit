// gRPC collection import dialog: import the interface hierarchy into an existing gRPC collection.
// Two sources supported: import proto files (file-picker button) or server reflection import.
// On confirm, rebuildGrpcCollection rebuilds the collection's package/service/rpc tree.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Cable,
  FileCode2,
  FilePlus2,
  Loader2,
  Package as PackageIcon,
  RefreshCw,
  Server,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useAppStore } from "@/store/useStore";
import type { GrpcPackageNode } from "@/data/types";
import { importProto, reflect } from "@/lib/bridge/grpc";
import { isTauri } from "@/lib/bridge";
import { readTextFile } from "@/lib/bridge/importExport";
import { uid } from "@/data/seed";
import { useT } from "@/lib/i18n";

interface ProtoEntry {
  id: string;
  name: string;
  content: string;
}

export function GrpcImportDialog({
  open,
  collectionId,
  initialTab = "proto",
  onOpenChange,
}: {
  open: boolean;
  collectionId: string | null;
  /** The default active import method: proto (import proto file) / reflection (reflection import) */
  initialTab?: "proto" | "reflection";
  onOpenChange: (o: boolean) => void;
}) {
  const { t, format } = useT();
  const collections = useAppStore((s) => s.collections);
  const rebuildGrpcCollection = useAppStore((s) => s.rebuildGrpcCollection);
  const [tab, setTab] = useState<"proto" | "reflection">(initialTab);

  // proto tab
  const [entries, setEntries] = useState<ProtoEntry[]>([]);
  const [parsed, setParsed] = useState<GrpcPackageNode[] | null>(null);
  const [parsedFiles, setParsedFiles] = useState<string[]>([]);

  // reflection tab
  const [target, setTarget] = useState("");
  const [reflected, setReflected] = useState<GrpcPackageNode[] | null>(null);
  const [reflectedFiles, setReflectedFiles] = useState<string[]>([]);

  // shared
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fileRef = useRef<HTMLInputElement>(null);

  const col = collections.find((c) => c.id === collectionId);

  const reset = useCallback(() => {
    setTab("proto");
    setEntries([]);
    setParsed(null);
    setParsedFiles([]);
    setTarget("");
    setReflected(null);
    setReflectedFiles([]);
    setError(null);
    setLoading(false);
  }, []);

  const close = () => {
    onOpenChange(false);
    reset();
  };

  // Sync the default tab on open, based on the entry point
  useEffect(() => {
    if (open) setTab(initialTab);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, initialTab]);

  /** Read a proto file's content: Tauri native dialog / browser input[type=file] */
  const readProtoFile = useCallback(async (file: File) => {
    try {
      if (isTauri()) {
        // Tauri mode: read via the native path (the dialog already picked a real file)
        const path = (file as unknown as { path?: string }).path;
        if (path) {
          const content = await readTextFile(path);
          setEntries((prev) => [
            ...prev,
            { id: uid("gp"), name: file.name, content },
          ]);
          return;
        }
      }
      // Browser mode: read text via FileReader
      const text = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = () => reject(new Error("Failed to read file"));
        reader.readAsText(file);
      });
      setEntries((prev) => [
        ...prev,
        { id: uid("gp"), name: file.name, content: text },
      ]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const handlePickFile = async () => {
    // Tauri: use the native dialog to pick files (absolute paths, supports multiple appends)
    if (isTauri()) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [{ name: "Proto", extensions: ["proto"] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      for (const p of paths) {
        try {
          const content = await readTextFile(p);
          const name = p.split(/[\\/]/).pop() ?? "file.proto";
          setEntries((prev) => [...prev, { id: uid("gp"), name, content }]);
        } catch (e) {
          setError(e instanceof Error ? e.message : String(e));
        }
      }
      return;
    }
    // Browser: trigger the hidden input[type=file]
    fileRef.current?.click();
  };

  const removeEntry = (id: string) =>
    setEntries((prev) => prev.filter((x) => x.id !== id));

  const handleImportProto = async () => {
    const valid = entries.filter((e) => e.content.trim().length > 0);
    if (valid.length === 0) {
      setError(t("grpc.importNoProto"));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const desc = await importProto(
        valid.map((e) => ({ name: e.name, content: e.content })),
      );
      setParsed(desc.packages ?? []);
      setParsedFiles(desc.descriptorFiles ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleReflect = async () => {
    const targetStr = target.trim();
    if (!targetStr) {
      setError(t("grpc.importNoTarget"));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const desc = await reflect(targetStr);
      setReflected(desc.packages ?? []);
      setReflectedFiles(desc.descriptorFiles ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleConfirm = () => {
    if (!collectionId) return;
    const newPackages = tab === "proto" ? parsed : reflected;
    const newDescriptorFiles = tab === "proto" ? parsedFiles : reflectedFiles;
    if (!newPackages || newPackages.length === 0) {
      setError(
        tab === "proto"
          ? t("grpc.importParseFirst")
          : t("grpc.importConnectFirst"),
      );
      return;
    }

    // Append-merge: a gRPC collection can import several protos, shown as distinct package hierarchies.
    // Same-named packages are overwritten by the new import (proto update); different names are appended; existing packages keep their auth/script config.
    const existingPackages = col?.grpc?.packages ?? [];
    const merged = [...existingPackages];
    for (const np of newPackages) {
      const idx = merged.findIndex((p) => p.name === np.name);
      if (idx >= 0) {
        // Same-named package: keep existing config (auth/script), overwrite with the new services/proto
        merged[idx] = {
          ...merged[idx],
          services: np.services,
          proto: np.proto ?? merged[idx].proto,
        };
      } else {
        merged.push(np);
      }
    }

    // Merge and de-duplicate descriptorFiles
    const existingFiles = col?.grpc?.descriptorFiles ?? [];
    const mergedFiles = [...existingFiles];
    for (const f of newDescriptorFiles ?? []) {
      if (!mergedFiles.includes(f)) mergedFiles.push(f);
    }

    rebuildGrpcCollection(collectionId, {
      source:
        tab === "proto"
          ? {
              type: "proto",
              files: entries.map((e) => ({ name: e.name, content: e.content })),
            }
          : { type: "reflection", target: target.trim() },
      packages: merged,
      descriptorFiles: mergedFiles,
    });
    close();
  };

  const currentPackages = tab === "proto" ? parsed : reflected;
  const canImport = !!currentPackages && currentPackages.length > 0;

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) close();
        else onOpenChange(true);
      }}
    >
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Cable className="h-4 w-4 text-fuchsia-400" />
            {t("grpc.importTitle")}
            <span className="text-xs font-normal text-muted-foreground">
              {format("grpc.importCollection", col?.name ?? "-")}
            </span>
          </DialogTitle>
        </DialogHeader>

        <div className="grid gap-4 py-2">
          <Tabs value={tab} onValueChange={(v) => setTab(v as typeof tab)}>
            <TabsList className="w-full">
              <TabsTrigger value="proto" className="flex-1">
                <FileCode2 className="mr-1.5 h-3.5 w-3.5" />
                {t("grpc.tabProto")}
              </TabsTrigger>
              <TabsTrigger value="reflection" className="flex-1">
                <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
                {t("grpc.tabReflection")}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="proto" className="mt-3 space-y-3">
              {/* File-picker button (not an input) */}
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={handlePickFile}
                >
                  <FilePlus2 className="h-3.5 w-3.5" />
                  {t("grpc.selectProtoFile")}
                </Button>
                <input
                  ref={fileRef}
                  type="file"
                  accept=".proto"
                  multiple
                  className="hidden"
                  onChange={(e) => {
                    const files = Array.from(e.target.files ?? []);
                    for (const f of files) void readProtoFile(f);
                    e.target.value = "";
                  }}
                />
                <span className="text-xs text-muted-foreground">
                  {t("grpc.multiImportHint")}
                </span>
              </div>

              {/* Selected file list */}
              {entries.length > 0 && (
                <div className="space-y-1.5">
                  {entries.map((entry) => (
                    <div
                      key={entry.id}
                      className="flex items-center gap-2 rounded-md border border-border px-2 py-1.5"
                    >
                      <FileCode2 className="h-3.5 w-3.5 shrink-0 text-primary" />
                      <span className="min-w-0 flex-1 truncate font-mono text-xs">
                        {entry.name}
                      </span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {format("grpc.charCount", entry.content.length)}
                      </span>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="h-6 w-6"
                        onClick={() => removeEntry(entry.id)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}

              {entries.length > 0 && (
                <Button
                  size="sm"
                  disabled={loading}
                  onClick={handleImportProto}
                >
                  {loading ? (
                    <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  ) : null}
                  {t("grpc.parseProto")}
                </Button>
              )}
            </TabsContent>

            <TabsContent value="reflection" className="mt-3 space-y-3">
              <div className="grid gap-2">
                <Label className="text-xs">{t("grpc.serverAddress")}</Label>
                <div className="flex items-center gap-2">
                  <Input
                    value={target}
                    onChange={(e) => setTarget(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void handleReflect();
                    }}
                    className="flex-1 font-mono text-sm"
                    placeholder="http://localhost:50051"
                  />
                  <Button size="sm" disabled={loading} onClick={handleReflect}>
                    {loading ? (
                      <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                    ) : null}
                    {t("grpc.connectImport")}
                  </Button>
                </div>
              </div>
            </TabsContent>
          </Tabs>

          {error && (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {error}
            </div>
          )}

          {/* Hierarchy preview */}
          {currentPackages && currentPackages.length > 0 && (
            <div className="rounded-md border border-border">
              <div className="border-b border-border bg-muted/40 px-3 py-1.5 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
                {t("grpc.hierarchyLabel")}
              </div>
              <div className="max-h-52 overflow-auto p-2 font-mono text-xs">
                {currentPackages.map((pkg) => (
                  <div key={pkg.name} className="mb-1">
                    <div className="flex items-center gap-1.5 font-semibold text-fuchsia-400">
                      <PackageIcon className="h-3.5 w-3.5" />
                      {pkg.name || "(default)"}
                    </div>
                    {pkg.services.map((svc) => (
                      <div key={svc.name} className="pl-4">
                        <div className="flex items-center gap-1.5 text-primary">
                          <Server className="h-3 w-3" />
                          {svc.name}
                        </div>
                        {svc.methods.map((m) => (
                          <div
                            key={m.name}
                            className="pl-4 text-emerald-400/80"
                          >
                            {m.name}
                          </div>
                        ))}
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>

        <DialogFooter className="mt-2">
          <Button variant="outline" size="sm" onClick={close}>
            <X className="mr-1 h-3.5 w-3.5" />
            {t("common.cancel")}
          </Button>
          <Button size="sm" disabled={!canImport} onClick={handleConfirm}>
            <Cable className="mr-1.5 h-3.5 w-3.5" />
            {t("grpc.importToCollection")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
