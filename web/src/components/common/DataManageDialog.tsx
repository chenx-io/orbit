import { useEffect, useRef, useState } from "react";
import { Database, Download, Trash2, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useT } from "@/lib/i18n";
import { getPersistenceService } from "@/lib/persistence";
import { isTauri, loadSnapshotJson, exportSnapshotToFile } from "@/lib/bridge";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";

/** Data management: autosave status / export snapshot / import snapshot / clear data */
export function DataManageDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { t, format } = useT();
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [exporting, setExporting] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const [clearOpen, setClearOpen] = useState(false);

  // Read the real snapshot's last save time on open (from localStorage / the file, still valid after a restart,
  // without relying on in-memory state — every autosave writes savedAt into the snapshot).
  // Note: a controlled Dialog's open is driven by external state, so onOpenChange(true) never fires and
  // a useEffect must watch the open change instead.
  const refresh = async () => {
    try {
      const json = await loadSnapshotJson();
      if (!json) {
        setSavedAt(null);
        return;
      }
      const snap = JSON.parse(json) as { savedAt?: number };
      setSavedAt(typeof snap.savedAt === "number" ? snap.savedAt : null);
    } catch {
      setSavedAt(null);
    }
  };

  useEffect(() => {
    if (open) void refresh();
  }, [open]);

  /** Build the snapshot JSON to export: prefer the current store (freshest), falling back to the stored snapshot as-is */
  const buildExportJson = async (): Promise<string | null> => {
    const svc = getPersistenceService();
    if (svc) {
      const json = svc.exportSnapshot();
      if (json) return json;
    }
    // Last-resort fallback: export the stored snapshot as-is when the service is not ready
    try {
      return await loadSnapshotJson();
    } catch {
      return null;
    }
  };

  const handleExport = async () => {
    setExporting(true);
    try {
      const json = await buildExportJson();
      if (!json) {
        toast.error(t("persist.exportFailed"));
        return;
      }
      if (isTauri()) {
        // Tauri: pick a path in the save dialog → Rust writes the file (reliable)
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          title: t("persist.export") as string,
          defaultPath: `orbit-snapshot-${new Date().toISOString().slice(0, 10)}.json`,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return; // the user cancelled
        await exportSnapshotToFile(path, json);
        toast.success(t("persist.exportSuccess"));
      } else {
        // Browser: Blob download
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = `orbit-snapshot-${new Date().toISOString().slice(0, 10)}.json`;
        a.click();
        URL.revokeObjectURL(url);
        toast.success(t("persist.exportSuccess"));
      }
    } catch (e) {
      console.warn("[persist] failed to export the snapshot", e);
      toast.error(t("persist.exportFailed"));
    } finally {
      setExporting(false);
    }
  };

  const handleImportFile = async (file: File) => {
    const text = await file.text();
    const ok = await getPersistenceService()?.importSnapshot(text);
    void refresh();
    if (ok) toast.success(t("persist.importSuccess"));
    else toast.error(t("persist.importFailed"));
  };

  const handleClear = async () => {
    // clearLocal reloads the page internally to return to the initial seed state
    await getPersistenceService()?.clearLocal();
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Database className="h-4 w-4" />
            {t("persist.manage")}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 text-sm">
          <div className="text-xs text-muted-foreground">
            <div className="mb-1 flex items-center gap-1.5 text-emerald-500">
              <span className="inline-block h-1.5 w-1.5 rounded-full bg-emerald-500" />
              {t("persist.autoSave")}
            </div>
            {savedAt
              ? format("persist.savedAt", new Date(savedAt).toLocaleString())
              : t("persist.never")}
          </div>
          <div className="grid grid-cols-1 gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void handleExport()}
              disabled={exporting}
            >
              <Upload className="mr-1.5 h-3.5 w-3.5" />
              {t("persist.export")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => fileRef.current?.click()}
            >
              <Download className="mr-1.5 h-3.5 w-3.5" />
              {t("persist.import")}
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept=".json,application/json"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void handleImportFile(f);
                e.target.value = "";
              }}
            />
            <Button
              variant="outline"
              size="sm"
              className="text-destructive hover:text-destructive"
              onClick={() => setClearOpen(true)}
            >
              <Trash2 className="mr-1.5 h-3.5 w-3.5" />
              {t("persist.clear")}
            </Button>
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t("persist.clearHint")}
          </p>
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
      {/* Clear-data confirmation (a DOM dialog, reliably rendered in the Tauri webview) */}
      <ConfirmDialog
        open={clearOpen}
        onOpenChange={setClearOpen}
        title={t("persist.clear")}
        message={t("persist.clearConfirm")}
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => void handleClear()}
      />
    </Dialog>
  );
}
