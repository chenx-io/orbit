// Historical report panel: run-record list (status dot + target + time + pass rate) + detail drawer (case results / failed steps).
import { useEffect, useState } from "react";
import {
  ArrowLeft,
  CheckCircle2,
  FileCode2,
  Folder,
  Layers,
  Trash2,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { ScenarioRunRecord } from "@/data/types";
import {
  filterSteps,
  ResultFilterBar,
  RunMetricsBar,
  StepList,
  type ResultFilterValue,
  type RunMetrics,
} from "./RunResultView";

const TYPE_ICON = {
  case: FileCode2,
  folder: Folder,
  suite: Layers,
} as const;

function fmtTime(ts: number): string {
  const d = new Date(ts);
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function ReportPanel() {
  const { t } = useT();
  const reports = useAppStore((s) => s.scenarioReports);
  const detail = useAppStore((s) => s.scenarioReportDetail);
  const fetchReports = useAppStore((s) => s.fetchReports);
  const openReport = useAppStore((s) => s.openReport);
  const closeReport = useAppStore((s) => s.closeReport);
  const removeReport = useAppStore((s) => s.removeReport);

  const [confirmId, setConfirmId] = useState<string | null>(null);

  useEffect(() => {
    void fetchReports();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (detail) return <ReportDetail record={detail} onBack={closeReport} />;

  return (
    <div className="flex h-full flex-col">
      <ScrollArea className="flex-1">
        <div className="space-y-0.5 p-1.5">
          {reports.length === 0 && (
            <div className="py-8 text-center text-xs text-muted-foreground">
              {t("report.empty")}
            </div>
          )}
          {reports.map((r) => {
            const Icon = TYPE_ICON[r.targetType] ?? FileCode2;
            const total = r.totalPass + r.totalFail;
            const rate =
              total > 0 ? Math.round((r.totalPass / total) * 100) : 0;
            return (
              <div
                key={r.id}
                onClick={() => void openReport(r.id)}
                className={cn(
                  "group flex h-8 cursor-pointer items-center gap-1.5 rounded-md border px-2 text-sm transition-colors",
                  "border-transparent hover:bg-accent/10",
                )}
              >
                {r.status === "pass" ? (
                  <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
                ) : (
                  <XCircle className="h-3.5 w-3.5 shrink-0 text-rose-500" />
                )}
                <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate">{r.targetName}</span>
                <span className="shrink-0 text-xs text-muted-foreground">
                  {fmtTime(r.startedAt)}
                </span>
                <span
                  className={cn(
                    "w-10 shrink-0 text-right text-xs font-medium",
                    rate >= 100 ? "text-emerald-500" : "text-rose-500",
                  )}
                >
                  {rate}%
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100"
                  title={t("common.delete")}
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmId(r.id);
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            );
          })}
        </div>
      </ScrollArea>
      <ConfirmDialog
        open={confirmId !== null}
        onOpenChange={(o) => {
          if (!o) setConfirmId(null);
        }}
        message={t("report.confirmDelete")}
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (confirmId) void removeReport(confirmId);
        }}
      />
    </div>
  );
}

function ReportDetail({
  record,
  onBack,
}: {
  record: ScenarioRunRecord;
  onBack: () => void;
}) {
  const { t, format } = useT();
  const [filter, setFilter] = useState<ResultFilterValue>("all");

  const metrics: RunMetrics = {
    envName: record.envName ?? record.cases[0]?.envName ?? null,
    iterations:
      record.iterations ??
      record.cases.reduce((n, c) => n + (c.iterations ?? 1), 0),
    pass: record.totalPass,
    fail: record.totalFail,
    durationMs: record.durationMs,
    requestMs: record.totalRequestMs ?? 0,
    requestCount: record.totalRequestCount ?? 0,
    assertCount: record.totalAssertions ?? 0,
  };
  const shownCases =
    filter === "all"
      ? record.cases
      : record.cases.filter((c) =>
          filter === "pass" ? c.fail === 0 : c.fail > 0,
        );
  const steps = filterSteps(record.steps, filter);

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <Button
          variant="ghost"
          size="icon-sm"
          className="h-6 w-6"
          onClick={onBack}
        >
          <ArrowLeft className="h-3.5 w-3.5" />
        </Button>
        <span className="text-sm font-semibold">{record.targetName}</span>
        <span className="text-xs text-muted-foreground">
          {fmtTime(record.startedAt)}
        </span>
        {record.aborted && (
          <span className="rounded-full border border-border px-1.5 text-xs text-muted-foreground">
            {t("report.aborted")}
          </span>
        )}
        <span className="ml-auto text-xs text-muted-foreground">
          {format("run.elapsed", (record.durationMs / 1000).toFixed(1))}
          {record.envName ? ` · ${t("report.env")}: ${record.envName}` : ""}
        </span>
      </div>
      {/* Metrics bar identical to the "current run" view */}
      <RunMetricsBar m={metrics} />
      <div className="flex shrink-0 items-center gap-1 px-3 py-1">
        <ResultFilterBar value={filter} onChange={setFilter} />
        <span className="ml-auto text-xs text-muted-foreground">
          {steps.length}
        </span>
      </div>
      <ScrollArea className="flex-1">
        <div className="space-y-2 p-2">
          <div className="text-xs font-medium text-muted-foreground">
            {t("report.cases")}
          </div>
          {shownCases.length === 0 && (
            <div className="py-4 text-center text-xs text-muted-foreground">
              {t("run.noSteps")}
            </div>
          )}
          {shownCases.map((c) => (
            <div
              key={c.scenarioId}
              className="rounded-md border border-border px-2 py-1.5"
            >
              <div className="flex items-center gap-1.5 text-sm">
                {c.fail > 0 ? (
                  <XCircle className="h-3.5 w-3.5 text-rose-500" />
                ) : (
                  <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
                )}
                <span className="min-w-0 flex-1 truncate">
                  {c.scenarioName}
                </span>
                <span className="text-xs text-muted-foreground">
                  {t("run.pass")} {c.pass} · {t("run.fail")} {c.fail}
                </span>
              </div>
              <div className="mt-0.5 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                <span>
                  {t("report.env")}: {c.envName ?? "—"}
                </span>
                {c.dataSetName && (
                  <span>
                    {t("report.dataset")}: {c.dataSetName}
                  </span>
                )}
                <span>
                  {t("report.iterations")}: {c.iterations}
                </span>
                {c.rows > 0 && (
                  <span>
                    {t("report.rows")}: {c.rows}
                  </span>
                )}
                {c.failedRows.length > 0 && (
                  <span className="text-rose-500">
                    {t("report.failedRows")}:{" "}
                    {c.failedRows.map((n) => n + 1).join(", ")}
                  </span>
                )}
              </div>
            </div>
          ))}
          <div className="pt-1 text-xs font-medium text-muted-foreground">
            {t("report.steps")}
          </div>
          <StepList steps={steps} />
        </div>
      </ScrollArea>
    </div>
  );
}
