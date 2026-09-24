// CSV dataset editor (shared component): name / read mode / CSV text (live validation) / first-50-rows preview.
// Reused by the "manage datasets" dialog (DataSetDialog) and the case-bottom "test data" tab (CaseDataPanel).
import { useMemo } from "react";
import { Table2, Trash2, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { DATASET_SIZE_WARN_BYTES, parseCsvText, previewRows } from "@/lib/csv";
import { DATA_SET_MODES } from "@/store/utils/scenarioRunner";
import type { ScenarioDataSet } from "@/data/types";

export function DataSetEditor({
  dataset,
  onPatch,
  onDelete,
  csvHeight = "h-28",
}: {
  dataset: ScenarioDataSet;
  /** Change write-back (parsing and columns/rowCount computation happen inside the component; an invalid CSV is still written back and flagged) */
  onPatch: (patch: Partial<ScenarioDataSet>) => void;
  onDelete?: () => void;
  csvHeight?: string;
}) {
  const { t, format } = useT();

  const parsed = useMemo(() => {
    if (!dataset.csv?.trim()) return null;
    try {
      return parseCsvText(dataset.csv);
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e) };
    }
  }, [dataset.csv]);

  const parseError =
    dataset.csv?.trim() && parsed && "error" in parsed ? parsed.error : null;
  const ok = parsed && !("error" in parsed) ? parsed : null;
  const tooLarge = (dataset.csv.length ?? 0) > DATASET_SIZE_WARN_BYTES;
  const preview = ok ? previewRows(ok.rows) : [];

  const writeCsv = (csv: string): void => {
    try {
      const p = parseCsvText(csv);
      onPatch({ csv, columns: p.columns, rowCount: p.rowCount });
    } catch {
      onPatch({ csv });
    }
  };

  return (
    <div className="min-w-0 flex-1 space-y-2">
      <div className="flex items-center gap-2">
        <Input
          value={dataset.name}
          onChange={(e) => onPatch({ name: e.target.value })}
          placeholder={t("scenario.dataset.namePlaceholder")}
          className="h-8 flex-1 text-xs"
        />
        <Select
          value={dataset.mode ?? "sequential"}
          onValueChange={(v) => onPatch({ mode: v as never })}
        >
          <SelectTrigger size="sm" className="h-8 w-24 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {DATA_SET_MODES.map((m) => (
              <SelectItem key={m} value={m} className="text-xs">
                {t(`scenario.dataset.mode.${m}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {onDelete && (
          <Button
            size="icon-sm"
            variant="ghost"
            className="h-8 w-8 text-destructive hover:text-destructive"
            title={t("scenario.delete")}
            onClick={onDelete}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>

      <div className="space-y-1">
        <Label className="text-xs text-muted-foreground">
          {t("scenario.dataset.csvLabel")}
          {ok && (
            <span className="ml-2">
              {format("scenario.dataset.stats", ok.rowCount, ok.columns.length)}
            </span>
          )}
          {tooLarge && (
            <span className="ml-2 text-warning">
              {t("scenario.dataset.tooLarge")}
            </span>
          )}
        </Label>
        <textarea
          value={dataset.csv}
          onChange={(e) => writeCsv(e.target.value)}
          spellCheck={false}
          className={`w-full resize-y rounded-md border border-input bg-transparent px-2.5 py-1.5 font-mono text-xs focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none ${csvHeight}`}
        />
        {parseError && (
          <div className="text-xs text-destructive">
            {format("scenario.dataset.invalid", parseError)}
          </div>
        )}
      </div>

      {preview.length > 0 && (
        <div className="space-y-1">
          <div className="flex items-center gap-1 text-xs text-muted-foreground">
            <Table2 className="h-3.5 w-3.5" /> {t("scenario.dataset.preview")}
          </div>
          <div className="overflow-auto rounded-md border border-border">
            <table className="w-full border-collapse text-xs">
              <thead className="sticky top-0 bg-muted">
                <tr>
                  {ok?.columns.map((c) => (
                    <th
                      key={c}
                      className="border-b border-border px-2 py-1 text-left font-medium whitespace-nowrap"
                    >
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {preview.map((row, i) => (
                  <tr key={i} className="hover:bg-accent/10">
                    {row.map((cell, j) => (
                      <td
                        key={j}
                        className="border-b border-border/60 px-2 py-1 font-mono whitespace-nowrap"
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

/** Case-bottom "test data" tab: edit the dataset bound to this case (create and bind one in one click if unbound). */
export function CaseDataPanel({ scenarioId }: { scenarioId: string }) {
  const { t } = useT();
  const scenario = useAppStore((s) =>
    s.scenarios.find((x) => x.id === scenarioId),
  );
  const dataSets = useAppStore((s) => s.scenarioDataSets);
  const addDataSet = useAppStore((s) => s.addDataSet);
  const updateDataSet = useAppStore((s) => s.updateDataSet);
  const removeDataSet = useAppStore((s) => s.removeDataSet);
  const updateScenario = useAppStore((s) => s.updateScenario);

  if (!scenario) return null;
  const dataset = dataSets.find((d) => d.id === scenario.dataSetId);

  if (!dataset) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
        <span>{t("scenario.dataset.bindNone")}</span>
        <Button
          size="sm"
          variant="outline"
          className="h-7 text-xs"
          onClick={() => {
            const id = addDataSet(
              `${scenario.name} · ${t("scenario.dataset")}`,
              "username,password\nalice,123456\nbob,abc123",
            );
            updateScenario(scenario.id, { dataSetId: id, useDataSet: true });
          }}
        >
          <Upload className="h-3.5 w-3.5" /> {t("scenario.dataset.createBind")}
        </Button>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col p-2">
      <DataSetEditor
        dataset={dataset}
        onPatch={(patch) => updateDataSet(dataset.id, patch)}
        onDelete={() => {
          removeDataSet(dataset.id);
        }}
        csvHeight="h-32"
      />
    </div>
  );
}
