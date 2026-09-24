// Test dataset management dialog: dataset list on the left + shared DataSetEditor on the right.
import { useEffect, useRef, useState } from "react";
import { FileSpreadsheet, Plus, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { parseCsvText } from "@/lib/csv";
import { DataSetEditor } from "./DataSetEditor";

const SAMPLE_CSV = "username,password,expect\nalice,123456,200\nbob,abc123,401";

export function DataSetDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useT();
  const dataSets = useAppStore((s) => s.scenarioDataSets);
  const addDataSet = useAppStore((s) => s.addDataSet);
  const updateDataSet = useAppStore((s) => s.updateDataSet);
  const removeDataSet = useAppStore((s) => s.removeDataSet);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setSelectedId((cur) =>
        cur && dataSets.some((d) => d.id === cur)
          ? cur
          : (dataSets[0]?.id ?? null),
      );
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, dataSets.length]);

  const draft = dataSets.find((d) => d.id === selectedId) ?? null;

  const onImportFile = (file: File | undefined): void => {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result ?? "");
      try {
        const parsed = parseCsvText(text);
        if (draft) {
          updateDataSet(draft.id, {
            csv: text,
            columns: parsed.columns,
            rowCount: parsed.rowCount,
            name: file.name.replace(/\.csv$/i, ""),
          });
        } else {
          const id = addDataSet(file.name.replace(/\.csv$/i, ""), text);
          updateDataSet(id, {
            columns: parsed.columns,
            rowCount: parsed.rowCount,
          });
          setSelectedId(id);
        }
      } catch (e) {
        console.error("[dataSet] CSV parse failed", e);
        // On parse failure still allow importing the raw text; the editor shows the error inline.
        if (draft) {
          updateDataSet(draft.id, { csv: text });
        } else {
          setSelectedId(addDataSet(file.name.replace(/\.csv$/i, ""), text));
        }
      }
    };
    reader.readAsText(file);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {t("scenario.dataset.title")}
          </DialogTitle>
        </DialogHeader>

        <div className="flex gap-3">
          {/* Left: dataset list */}
          <div className="flex w-44 shrink-0 flex-col gap-1.5">
            <div className="flex gap-1">
              <Button
                size="sm"
                variant="outline"
                className="h-7 flex-1 text-xs"
                onClick={() => {
                  const id = addDataSet(
                    `${t("scenario.dataset.new")} ${dataSets.length + 1}`,
                    SAMPLE_CSV,
                  );
                  setSelectedId(id);
                }}
              >
                <Plus className="h-3.5 w-3.5" /> {t("scenario.dataset.new")}
              </Button>
              <Button
                size="icon-sm"
                variant="outline"
                className="h-7 w-7"
                title={t("scenario.dataset.import")}
                onClick={() => fileRef.current?.click()}
              >
                <Upload className="h-3.5 w-3.5" />
              </Button>
              <input
                ref={fileRef}
                type="file"
                accept=".csv,text/csv"
                className="hidden"
                onChange={(e) => {
                  onImportFile(e.target.files?.[0]);
                  e.target.value = "";
                }}
              />
            </div>
            <ScrollArea className="h-64">
              <div className="space-y-1">
                {dataSets.length === 0 && (
                  <div className="py-6 text-center text-xs text-muted-foreground">
                    {t("scenario.dataset.empty")}
                  </div>
                )}
                {dataSets.map((d) => (
                  <button
                    key={d.id}
                    onClick={() => setSelectedId(d.id)}
                    className={cn(
                      "flex w-full items-center gap-1.5 rounded-md border px-2 py-1.5 text-left text-xs transition-colors",
                      d.id === selectedId
                        ? "border-primary/40 bg-accent/20"
                        : "border-border hover:bg-accent/10",
                    )}
                  >
                    <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate">{d.name}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {d.rowCount}
                    </span>
                  </button>
                ))}
              </div>
            </ScrollArea>
          </div>

          {/* Right: shared editor */}
          {draft ? (
            <DataSetEditor
              dataset={draft}
              onPatch={(patch) => updateDataSet(draft.id, patch)}
              onDelete={() => setConfirmDelete(draft.id)}
            />
          ) : (
            <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
              {t("scenario.dataset.empty")}
            </div>
          )}
        </div>

        <ConfirmDialog
          open={confirmDelete !== null}
          onOpenChange={(o) => {
            if (!o) setConfirmDelete(null);
          }}
          message={t("scenario.dataset.confirmDelete")}
          danger
          confirmLabel={t("common.delete")}
          onConfirm={() => {
            if (confirmDelete) {
              removeDataSet(confirmDelete);
              setSelectedId(null);
            }
          }}
        />
      </DialogContent>
    </Dialog>
  );
}
