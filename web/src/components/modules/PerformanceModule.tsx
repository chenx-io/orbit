import { useEffect, useState, useRef, useCallback } from "react";
import {
  CheckCircle2,
  XCircle,
  Trash2,
  Star,
  StarOff,
  Pencil,
  Download,
  Loader2,
  Search,
  X,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { useT, type TKey } from "@/lib/i18n";
import { tFormat } from "@/lib/localeDict";
import { exportSavedReport, isTauri, writeExportFile } from "@/lib/bridge";
import type { SavedReport } from "@/data/types";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";

/** Build the percentile point list from SavedReport.summary */
function buildPercentiles(report: SavedReport, baseline?: SavedReport | null) {
  const s = report.summary;
  const b = baseline?.summary;
  return [
    {
      p: 50,
      value: Math.round(s.p50_ms),
      baseline: b ? Math.round(b.p50_ms) : undefined,
    },
    {
      p: 75,
      value: Math.round((s.p50_ms + s.p95_ms) / 2),
      baseline: b ? Math.round((b.p50_ms + b.p95_ms) / 2) : undefined,
    },
    {
      p: 90,
      value: Math.round(s.p90_ms),
      baseline: b ? Math.round(b.p90_ms) : undefined,
    },
    {
      p: 95,
      value: Math.round(s.p95_ms),
      baseline: b ? Math.round(b.p95_ms) : undefined,
    },
    {
      p: 99,
      value: Math.round(s.p99_ms),
      baseline: b ? Math.round(b.p99_ms) : undefined,
    },
  ];
}

/** Report export formats (saved reports contain summaries only, no raw samples) */
const REPORT_EXPORT_FORMATS = [
  { value: "html", labelKey: "load.report.html", ext: "html" },
  { value: "json", labelKey: "load.report.json", ext: "json" },
  { value: "csv", labelKey: "load.report.csv", ext: "csv" },
  { value: "junit", labelKey: "load.report.junit", ext: "xml" },
] as const;

function formatCreatedAt(iso: string) {
  try {
    let d: Date;
    // Try ISO 8601
    d = new Date(iso);
    if (!isNaN(d.getTime())) {
      return formatDate(d);
    }
    // Try a Unix timestamp (milliseconds)
    const ts = Number(iso);
    if (!isNaN(ts) && ts > 1000000000000) {
      d = new Date(ts);
      if (!isNaN(d.getTime())) return formatDate(d);
    }
    // Legacy compat for SystemTime { intervals: N } (FILETIME: 100-ns intervals since 1601-01-01)
    const m = iso.match(/intervals:\s*(\d+)/);
    if (m) {
      const filetime = BigInt(m[1]);
      const FILETIME_EPOCH_OFFSET = 116444736000000000n; // 100-ns intervals between 1601 and 1970
      const epochMs = Number((filetime - FILETIME_EPOCH_OFFSET) / 10000n);
      if (epochMs > 0) {
        d = new Date(epochMs);
        if (!isNaN(d.getTime())) return formatDate(d);
      }
    }
    return iso || "N/A";
  } catch {
    return iso || "N/A";
  }
}

function formatDate(d: Date) {
  const y = d.getFullYear();
  const mo = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${y}${mo}${dd} ${hh}:${mm}`;
}

// ── Context menu ──

type CtxState = { x: number; y: number; report: SavedReport } | null;

function useContextMenu(onClose: () => void, ctx: CtxState) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    if (!ctx || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    let left = ctx.x,
      top = ctx.y;
    if (left + r.width > window.innerWidth - 8)
      left = window.innerWidth - r.width - 8;
    if (top + r.height > window.innerHeight - 8)
      top = window.innerHeight - r.height - 8;
    if (left < 8) left = 8;
    if (top < 8) top = 8;
    setPos({ left, top });
  }, [ctx]);

  useEffect(() => {
    if (!ctx) return;
    const handler = () => onClose();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") handler();
    };
    window.addEventListener("click", handler);
    window.addEventListener("contextmenu", handler, true);
    window.addEventListener("scroll", handler, true);
    window.addEventListener("resize", handler);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", handler);
      window.removeEventListener("contextmenu", handler, true);
      window.removeEventListener("scroll", handler, true);
      window.removeEventListener("resize", handler);
      window.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx]);

  return { ref, pos };
}

export function PerformanceModule() {
  const { t } = useT();
  const reports = useAppStore((s) => s.savedReports);
  const baselines = useAppStore((s) => s.baselines);
  const fetchReports = useAppStore((s) => s.fetchReports);
  const fetchBaselines = useAppStore((s) => s.fetchBaselines);
  const deleteReport = useAppStore((s) => s.deleteReport);
  const renameReport = useAppStore((s) => s.renameReport);
  const markBaseline = useAppStore((s) => s.markBaseline);
  const unmarkBaseline = useAppStore((s) => s.unmarkBaseline);

  const [selectedId, setSelectedId] = useState("");
  const [baselineId, setBaselineId] = useState("");
  const [query, setQuery] = useState("");

  // Context menu
  const [ctx, setCtx] = useState<CtxState>(null);
  const closeCtx = useCallback(() => setCtx(null), []);
  const { ref: ctxRef, pos: ctxPos } = useContextMenu(closeCtx, ctx);

  // Rename
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{
    id: string;
    name: string;
  } | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportFormat, setExportFormat] = useState<string>("html");
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState("");

  useEffect(() => {
    fetchReports();
    fetchBaselines();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync baselines into local state
  useEffect(() => {
    if (!baselineId && baselines.length > 0) {
      setBaselineId(baselines[0].id);
    }
  }, [baselines, baselineId]);

  const report = reports.find((r) => r.id === selectedId) ?? reports[0];
  const baseline = baselines.find((b) => b.id === baselineId) ?? null;
  const filtered = query.trim()
    ? reports.filter((r) =>
        r.name.toLowerCase().includes(query.trim().toLowerCase()),
      )
    : reports;

  if (!report) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-muted-foreground">
        <p>{t("perf.noReports")}</p>
        <p className="text-xs">{t("perf.noReportsHint")}</p>
      </div>
    );
  }

  const onExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      const { content, filename } = await exportSavedReport(
        report.id,
        exportFormat,
      );
      const fmt = REPORT_EXPORT_FORMATS.find((f) => f.value === exportFormat)!;
      if (isTauri()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath: filename,
          filters: [{ name: t(fmt.labelKey as TKey), extensions: [fmt.ext] }],
        });
        if (!path) {
          setExporting(false);
          return;
        }
        await writeExportFile(path, content);
        setExportMsg(tFormat("load.exported", path));
      } else {
        writeExportFile(filename, content);
        setExportMsg(tFormat("load.exported", filename));
      }
    } catch (e) {
      setExportMsg(`❌ ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setExporting(false);
    }
  };

  const percentiles = buildPercentiles(report, baseline);
  const maxVal = Math.max(
    ...percentiles.map((p) => Math.max(p.value, p.baseline ?? 0)),
    1,
  );
  const thresholds = report.thresholds || [];

  const menuCls =
    "z-50 min-w-35 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md text-sm";
  const itemCls =
    "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-accent hover:text-accent-foreground";
  const dangerCls =
    "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-destructive hover:bg-destructive/10";

  return (
    <PanelGroup
      direction="horizontal"
      autoSaveId="performance-layout"
      className="h-full"
    >
      {/* Left: report list */}
      <Panel
        defaultSize={25}
        minSize={25}
        className="border-r border-border bg-card/40"
      >
        <div className="flex h-full flex-col">
          {/* Search box (consistent with the history list) */}
          <div className="shrink-0 border-b border-border p-2">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("perf.search")}
                className="h-8 pl-8 pr-8 text-xs"
              />
              {query && (
                <button
                  onClick={() => setQuery("")}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              )}
            </div>
          </div>
          <ScrollArea className="flex-1">
            <div className="space-y-1 p-2">
              {filtered.length === 0 && (
                <div className="p-6 text-center text-xs text-muted-foreground">
                  {t("perf.noMatch")}
                </div>
              )}
              {filtered.map((r) => (
                <button
                  key={r.id}
                  onClick={() => setSelectedId(r.id)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    setCtx({ x: e.clientX, y: e.clientY, report: r });
                  }}
                  className={`w-full cursor-pointer rounded-md border px-2 py-1.5 text-left text-sm transition-colors ${report?.id === r.id ? "border-primary/40 bg-accent/20" : "border-border hover:bg-accent/10"}`}
                >
                  <div className="flex items-center gap-1.5">
                    {r.is_baseline && (
                      <Star className="h-3 w-3 shrink-0 text-amber-400" />
                    )}
                    <span className="min-w-0 flex-1 truncate font-medium">
                      {r.name}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {formatCreatedAt(r.created_at)}
                    </span>
                  </div>
                </button>
              ))}
            </div>
          </ScrollArea>
        </div>

        {/* Context menu */}
        {ctx && (
          <div
            ref={ctxRef}
            className={menuCls}
            style={{ position: "fixed", ...ctxPos }}
          >
            <div
              className={itemCls}
              onClick={() => {
                setRenameTarget({ id: ctx.report.id, name: ctx.report.name });
                setRenameValue(ctx.report.name);
                setRenameOpen(true);
                setCtx(null);
              }}
            >
              <Pencil className="h-4 w-4" />
              {t("common.rename")}
            </div>
            <div
              className={dangerCls}
              onClick={() => {
                setDeleteConfirm(ctx.report.id);
                setCtx(null);
              }}
            >
              <Trash2 className="h-4 w-4" />
              {t("common.delete")}
            </div>
          </div>
        )}

        {/* Rename dialog */}
        <Dialog
          open={renameOpen}
          onOpenChange={(o) => {
            if (!o) setRenameOpen(false);
          }}
        >
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle className="text-sm">
                {t("common.rename")}
              </DialogTitle>
            </DialogHeader>
            <Input
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              placeholder={t("common.renameHint")}
              className="mt-2"
            />
            <DialogFooter>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setRenameOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                size="sm"
                onClick={async () => {
                  if (renameTarget && renameValue.trim()) {
                    await renameReport(renameTarget.id, renameValue.trim());
                    setRenameOpen(false);
                  }
                }}
                disabled={!renameValue.trim()}
              >
                {t("common.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
        <ConfirmDialog
          open={deleteConfirm !== null}
          onOpenChange={(o) => {
            if (!o) setDeleteConfirm(null);
          }}
          message={t("perf.confirmDelete")}
          danger
          confirmLabel={t("common.delete")}
          onConfirm={() => {
            if (deleteConfirm) deleteReport(deleteConfirm);
          }}
        />
      </Panel>

      <PanelResizeHandle className="w-1 bg-border transition-colors hover:bg-primary" />

      {/* Right: details */}
      <Panel minSize={40} className="overflow-auto p-4">
        {/* Title bar */}
        <div className="mb-3 flex items-center gap-2">
          <h2 className="text-lg font-semibold">{report.name}</h2>
          <Badge variant="outline" className="font-mono">
            {report.endpoint}
          </Badge>
          {report.is_baseline && (
            <Badge variant="success" className="gap-1 text-xs">
              <Star className="h-3 w-3" />
              {report.baseline_name ?? t("perf.baseline")}
            </Badge>
          )}
        </div>

        {/* Overview cards */}
        <div className="mb-4 grid grid-cols-4 gap-2 text-sm">
          <div>
            <div className="text-sm text-muted-foreground">
              {t("perf.samples")}
            </div>
            <div className="font-mono font-bold">
              {report.summary.total_requests.toLocaleString()}
            </div>
          </div>
          <div>
            <div className="text-sm text-muted-foreground">
              {t("perf.throughput")}
            </div>
            <div className="font-mono font-bold">
              {Math.round(report.summary.rps)} RPS
            </div>
          </div>
          <div>
            <div className="text-sm text-muted-foreground">
              {t("perf.errorRate")}
            </div>
            <div
              className={`font-mono font-bold ${report.summary.error_rate > 1 ? "text-rose-400" : "text-emerald-400"}`}
            >
              {report.summary.error_rate}%
            </div>
          </div>
          <div>
            <div className="text-sm text-muted-foreground">
              {t("perf.config")}
            </div>
            <div className="font-mono font-bold">
              {report.config ?? `${report.vus}VU ${report.duration}`}
            </div>
          </div>
        </div>

        {/* Baseline picker */}
        <div className="mb-3 flex items-center gap-2">
          <span className="text-xs text-muted-foreground">
            {t("perf.compareBaseline")}:
          </span>
          <Select value={baselineId} onValueChange={setBaselineId}>
            <SelectTrigger className="h-7 w-48 text-xs">
              <SelectValue placeholder={t("perf.selectBaseline")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="">{t("perf.noBaseline")}</SelectItem>
              {baselines.map((b) => (
                <SelectItem key={b.id} value={b.id}>
                  {b.baseline_name ?? b.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {/* Percentile comparison */}
        <Card className="mb-3">
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">
              {t("perf.percentileTitle")}
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {percentiles.map((p) => (
              <div key={p.p}>
                <div className="mb-0.5 flex items-center justify-between text-xs">
                  <span className="font-medium">P{p.p}</span>
                  <span className="font-mono">
                    {p.value}ms
                    {p.baseline != null && (
                      <span
                        className={`ml-2 ${p.value <= p.baseline ? "text-emerald-400" : "text-rose-400"}`}
                      >
                        {t("perf.baseline")}: {p.baseline}ms (
                        {p.value <= p.baseline ? "↓" : "↑"}
                        {Math.abs(p.value - p.baseline)}ms)
                      </span>
                    )}
                  </span>
                </div>
                <div className="relative h-3 overflow-hidden rounded bg-secondary">
                  <div
                    className="h-full rounded bg-primary"
                    style={{ width: `${(p.value / maxVal) * 100}%` }}
                  />
                  {p.baseline != null && (
                    <div
                      className="absolute top-0 h-full w-0.5 bg-muted-foreground/60"
                      style={{ left: `${(p.baseline / maxVal) * 100}%` }}
                    />
                  )}
                </div>
              </div>
            ))}
            <div className="flex gap-4 pt-1 text-xs text-muted-foreground">
              <span className="flex items-center gap-1">
                <span className="h-2 w-2 rounded bg-primary" />
                {t("perf.actual")}
              </span>
              {baseline && (
                <span className="flex items-center gap-1">
                  <span className="h-2 w-2 rounded bg-muted-foreground/60" />
                  {t("perf.baseline")}
                </span>
              )}
            </div>
          </CardContent>
        </Card>

        {/* SLA gate + actions */}
        <div className="grid grid-cols-2 gap-3">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("perf.slaTitle")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-1.5">
              {thresholds.length === 0 && (
                <div className="text-xs text-muted-foreground">
                  {t("perf.noSla")}
                </div>
              )}
              {thresholds.map((g, i) => (
                <div
                  key={i}
                  className="flex items-center gap-2 rounded border border-border px-2 py-1.5 text-xs"
                >
                  {g.passed ? (
                    <CheckCircle2 className="h-4 w-4 text-emerald-400" />
                  ) : (
                    <XCircle className="h-4 w-4 text-rose-400" />
                  )}
                  <span className="flex-1">{g.label}</span>
                  <span className="font-mono text-muted-foreground">
                    {g.actual} / {g.target}
                  </span>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("perf.actions")}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-1.5">
              {!report.is_baseline ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="w-full justify-start gap-1"
                  onClick={() => markBaseline(report.id, report.name)}
                >
                  <Star className="h-3.5 w-3.5" />
                  {t("perf.setBaseline")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="outline"
                  className="w-full justify-start gap-1"
                  onClick={() => unmarkBaseline(report.id)}
                >
                  <StarOff className="h-3.5 w-3.5" />
                  {t("perf.unsetBaseline")}
                </Button>
              )}
              {isTauri() && (
                <Button
                  size="sm"
                  variant="outline"
                  className="w-full justify-start gap-1"
                  onClick={() => {
                    setExportMsg("");
                    setExportOpen(true);
                  }}
                >
                  <Download className="h-3.5 w-3.5" />
                  {t("load.exportReport")}
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                className="w-full justify-start gap-1 text-rose-400 hover:text-rose-400"
                onClick={() => setDeleteConfirm(report.id)}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("perf.delete")}
              </Button>
            </CardContent>
          </Card>
        </div>

        <div className="mt-3 truncate text-xs text-muted-foreground">
          {report.config ??
            `VU: ${report.vus} · ${t("load.duration")}: ${report.duration}`}{" "}
          · {formatCreatedAt(report.created_at)}
        </div>

        {/* Export report dialog */}
        <Dialog
          open={exportOpen}
          onOpenChange={(o) => {
            if (!o) {
              setExportMsg("");
            }
            setExportOpen(o);
          }}
        >
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>{t("load.exportReport")}</DialogTitle>
            </DialogHeader>
            <div className="space-y-2 py-2">
              <Label className="text-xs text-muted-foreground">
                {t("load.exportFormat")}
              </Label>
              <Select
                value={exportFormat}
                onValueChange={(v) => {
                  setExportFormat(v);
                  setExportMsg("");
                }}
              >
                <SelectTrigger className="h-8 w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {REPORT_EXPORT_FORMATS.map((f) => (
                    <SelectItem key={f.value} value={f.value}>
                      {t(f.labelKey as TKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs leading-snug text-muted-foreground">
                {t("perf.exportHint")}
              </p>
              {exportMsg && (
                <div className="rounded-md border border-border px-3 py-2 text-xs text-muted-foreground">
                  {exportMsg}
                </div>
              )}
            </div>
            <DialogFooter>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setExportOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                size="sm"
                className="gap-1"
                onClick={onExport}
                disabled={exporting}
              >
                {exporting ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                {t("load.export")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </Panel>
    </PanelGroup>
  );
}
