import {
  Play,
  Square,
  Activity,
  Save,
  Star,
  Trash2,
  Plus,
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
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
import { RequestTreeSelect } from "@/components/common/RequestTreeSelect";
import { AgentExecutionTarget } from "@/components/common/AgentExecutionTarget";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { LiveChart } from "@/components/charts/LiveChart";
import { useAppStore } from "@/store/useStore";
import { useT, type TKey } from "@/lib/i18n";
import { tFormat } from "@/lib/localeDict";
import { exportLoadReport, isTauri, writeExportFile } from "@/lib/bridge";
import { isHttpRequest, requestProtocol } from "@/data/types";
import { PROTOCOL_LABEL } from "@/lib/utils";
import type {
  LoadExecutor,
  LoadRampMode,
  LoadStage,
  ThresholdRule,
  TimingBreakdown,
  ErrorGroup,
} from "@/data/types";
import { listAgents, type AgentInfo } from "@/lib/bridge/distributed";
import { useEffect, useRef, useState } from "react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";

const EXECUTOR_KEYS: Record<LoadExecutor, string> = {
  "constant-vus": "load.executorConstant",
  "ramping-vus": "load.executorRamping",
  "constant-arrival-rate": "load.executorArrival",
};

let _ruleId = 0;
function newId() {
  return `thr-${Date.now()}-${++_ruleId}`;
}

const METRIC_OPTIONS = [
  { value: "p50", labelKey: "load.metric.p50" },
  { value: "p90", labelKey: "load.metric.p90" },
  { value: "p95", labelKey: "load.metric.p95" },
  { value: "p99", labelKey: "load.metric.p99" },
  { value: "avg", labelKey: "load.metric.avg" },
  { value: "errorRate", labelKey: "load.option.errorRate" },
] as const;

/** Report export formats (summary formats go through the summary, raw-sample formats through JTL/JSONL). */
const EXPORT_FORMATS = [
  { value: "html", labelKey: "load.report.html", ext: "html" },
  { value: "json", labelKey: "load.report.json", ext: "json" },
  { value: "csv", labelKey: "load.report.csv", ext: "csv" },
  { value: "junit", labelKey: "load.report.junit", ext: "xml" },
  { value: "jtl", labelKey: "load.report.jtl", ext: "jtl" },
  { value: "raw-json", labelKey: "load.report.rawJson", ext: "jsonl" },
] as const;

/** Error type key → i18n key for its display name. */
const ERROR_LABEL_KEY: Record<string, string> = {
  connect: "load.err.connect",
  dns: "load.err.dns",
  tls: "load.err.tls",
  timeout: "load.err.timeout",
  send: "load.err.send",
  receive: "load.err.receive",
  protocol: "load.err.protocol",
  codec: "load.err.codec",
  server_error: "load.err.server_error",
  assertion: "load.err.assertion",
  post_script: "load.err.post_script",
  internal: "load.err.internal",
  other: "load.err.other",
};

/** Error type distribution (shared by the hover content and the summary area). */
function ErrorBreakdown({
  errors,
  total,
}: {
  errors: ErrorGroup[];
  total?: number;
}) {
  const { t } = useT();
  const sum = total ?? errors.reduce((a, b) => a + b.count, 0);
  return (
    <div>
      <div className="mb-1.5 text-xs font-medium">
        {t("load.errDistribution")}
      </div>
      <div className="space-y-1.5">
        {errors.map((g) => {
          const pct = sum > 0 ? ((g.count / sum) * 100).toFixed(1) : "0.0";
          return (
            <div key={g.type} className="text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="text-foreground">
                  {ERROR_LABEL_KEY[g.type]
                    ? t(ERROR_LABEL_KEY[g.type] as TKey)
                    : g.type}
                </span>
                <span className="shrink-0 font-mono tabular-nums text-rose-300">
                  {g.count} · {pct}%
                </span>
              </div>
              <div className="truncate text-muted-foreground" title={g.sample}>
                {g.sample}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Metric({
  label,
  value,
  unit,
  color,
  timing,
  errors,
}: {
  label: string;
  value: string;
  unit?: string;
  color?: string;
  timing?: TimingBreakdown;
  errors?: ErrorGroup[];
}) {
  const { t } = useT();
  const inner = (
    <div className="rounded-md border border-border bg-card/60 p-3">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div
        className={`font-mono text-2xl font-bold ${color ?? "text-foreground"}`}
      >
        {value}
        {unit && (
          <span className="ml-1 text-sm font-normal text-muted-foreground">
            {unit}
          </span>
        )}
      </div>
    </div>
  );

  if (timing && timing.total_ms > 0) {
    const phases: { key: string; label: string; ms: number; bar: number }[] = [
      { key: "dns", label: t("load.phase.dns"), ms: timing.dns_ms, bar: 0 },
      { key: "tcp", label: t("load.phase.tcp"), ms: timing.tcp_ms, bar: 0 },
      { key: "tls", label: t("load.phase.tls"), ms: timing.tls_ms, bar: 0 },
      { key: "send", label: t("load.phase.send"), ms: timing.send_ms, bar: 0 },
      { key: "ttfb", label: t("load.phase.ttfb"), ms: timing.ttfb_ms, bar: 0 },
      {
        key: "download",
        label: t("load.phase.download"),
        ms: timing.download_ms,
        bar: 0,
      },
    ];
    const maxMs = Math.max(...phases.map((p) => p.ms), 1);
    for (const p of phases)
      p.bar = Math.max(1, Math.round((p.ms / maxMs) * 100));

    return (
      <Tooltip>
        <TooltipTrigger asChild>{inner}</TooltipTrigger>
        <TooltipContent side="bottom" className="w-60 p-3">
          <div className="mb-1.5 text-xs font-medium">
            {t("load.timingBreakdown")}
          </div>
          <div className="space-y-1">
            {phases.map((p) => (
              <div key={p.key} className="flex items-center gap-2 text-xs">
                <span className="w-20 shrink-0 text-muted-foreground">
                  {p.label}
                </span>
                <div className="flex-1 h-2.5 rounded-full bg-muted overflow-hidden">
                  <div
                    className="h-full rounded-full bg-primary/60"
                    style={{ width: `${p.bar}%` }}
                  />
                </div>
                <span className="w-12 text-right font-mono tabular-nums text-foreground">
                  {p.ms.toFixed(1)}ms
                </span>
              </div>
            ))}
          </div>
          <div className="mt-1.5 border-t border-border pt-1 text-xs text-muted-foreground">
            {tFormat("load.timingTotal", timing.total_ms.toFixed(1))}
          </div>
        </TooltipContent>
      </Tooltip>
    );
  }

  if (errors && errors.length > 0) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>{inner}</TooltipTrigger>
        <TooltipContent side="bottom" className="w-72 p-3">
          <ErrorBreakdown errors={errors} />
        </TooltipContent>
      </Tooltip>
    );
  }

  return inner;
}

/** Ramp mode labels; "JMeter" keeps its literal wording, the rest are i18n keys. */
const RAMP_LABEL_KEY: Record<LoadRampMode, string> = {
  gradual: "load.ramp.gradual",
  instant: "load.ramp.instant",
  jmeter: "JMeter",
};

/**
 * Integer number input: keeps the user's raw text while editing (clearing is allowed) and only
 * parses on blur / Enter. This avoids the controlled-input pitfall where deleting forces a write-back
 * of 0 and the next keystroke starts with a leading zero.
 */
function NumberField({
  value,
  min = 0,
  title,
  className,
  placeholder,
  onChange,
}: {
  value: number;
  min?: number;
  title?: string;
  className?: string;
  placeholder?: string;
  onChange: (v: number) => void;
}) {
  const [text, setText] = useState(String(value));
  const editing = useRef(false);

  // Sync external value changes into the display (without interrupting an edit in progress)
  useEffect(() => {
    if (!editing.current) setText(String(value));
  }, [value]);

  const commit = () => {
    const parsed = text.trim() === "" ? min : parseInt(text, 10);
    const v = Number.isFinite(parsed) ? Math.max(min, parsed) : min;
    editing.current = false;
    onChange(v);
    setText(String(v));
  };

  return (
    <Input
      type="number"
      min={min}
      value={text}
      title={title}
      placeholder={placeholder}
      onFocus={() => {
        editing.current = true;
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
      onChange={(e) => setText(e.target.value)}
      className={className}
    />
  );
}

function StageList({
  stages,
  withRamp,
  onChange,
}: {
  stages: LoadStage[];
  withRamp?: boolean;
  onChange: (stages: LoadStage[]) => void;
}) {
  const { t } = useT();
  const list = stages ?? [];
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const patch = (id: string, p: Partial<LoadStage>) =>
    onChange(list.map((x) => (x.id === id ? { ...x, ...p } : x)));
  const toggleCollapse = (id: string) =>
    setCollapsed((c) => ({ ...c, [id]: !c[id] }));
  return (
    <div className="space-y-1.5">
      {list.map((st, i) => (
        <div key={st.id} className="rounded-md border border-border/60 p-1.5">
          <div className="flex items-center justify-between text-xs">
            <button
              type="button"
              className="flex min-w-0 flex-1 items-center gap-1 text-left"
              onClick={() => toggleCollapse(st.id)}
              title={collapsed[st.id] ? t("load.expand") : t("load.collapse")}
            >
              {collapsed[st.id] ? (
                <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
              ) : (
                <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
              )}
              <span className="shrink-0 text-xs text-muted-foreground">
                {tFormat("load.stageIndex", i + 1)}
              </span>
              {collapsed[st.id] && (
                <span className="truncate text-xs text-muted-foreground">
                  {st.target} VU · {st.duration}
                  {withRamp
                    ? ` · ${t(RAMP_LABEL_KEY[st.ramp ?? "gradual"] as TKey)}`
                    : ""}
                </span>
              )}
            </button>
            <Button
              variant="ghost"
              size="icon-sm"
              className="h-6 w-6 shrink-0"
              onClick={() => onChange(list.filter((x) => x.id !== st.id))}
            >
              <Trash2 className="h-3 w-3" />
            </Button>
          </div>
          {!collapsed[st.id] && (
            <>
              <div className="mt-1.5 flex items-center gap-1.5 text-xs">
                <span className="w-12 shrink-0 text-xs text-muted-foreground">
                  {t("load.targetVu")}
                </span>
                <NumberField
                  value={st.target}
                  title={t("load.targetVuTitle")}
                  onChange={(v) => patch(st.id, { target: v })}
                  className="h-7 min-w-0 flex-1"
                />
              </div>
              <div className="mt-1.5 flex items-center gap-1.5 text-xs">
                <span className="w-12 shrink-0 text-xs text-muted-foreground">
                  {t("load.durationLabel")}
                </span>
                <Input
                  value={st.duration}
                  title={t("load.durationTitle")}
                  placeholder="30s"
                  onChange={(e) => patch(st.id, { duration: e.target.value })}
                  className="h-7 min-w-0 flex-1"
                />
              </div>
              {withRamp && (
                <div className="mt-1.5 flex items-center gap-1.5 text-xs">
                  <span className="w-12 shrink-0 text-xs text-muted-foreground">
                    {t("load.rampLabel")}
                  </span>
                  <Select
                    value={st.ramp ?? "gradual"}
                    onValueChange={(v) =>
                      patch(st.id, { ramp: v as LoadRampMode })
                    }
                  >
                    <SelectTrigger className="h-7 w-[76px] text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="gradual">
                        {t("load.ramp.gradual")}
                      </SelectItem>
                      <SelectItem value="instant">
                        {t("load.ramp.instant")}
                      </SelectItem>
                      <SelectItem value="jmeter">JMeter</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              )}
              {withRamp && st.ramp === "jmeter" && (
                <div className="mt-1.5 text-xs">
                  <div className="flex items-center gap-1.5">
                    <span className="w-12 shrink-0 text-xs text-muted-foreground">
                      {t("load.rampUpLabel")}
                    </span>
                    <Input
                      value={st.rampUp ?? st.duration}
                      title={t("load.rampUpHint")}
                      placeholder={t("load.rampUpPlaceholder")}
                      onChange={(e) => patch(st.id, { rampUp: e.target.value })}
                      className="h-7 min-w-0 flex-1"
                    />
                  </div>
                  <div className="mt-1 pl-12 text-xs leading-snug text-muted-foreground">
                    {t("load.rampUpHint")}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      ))}
      <Button
        variant="outline"
        size="sm"
        className="h-7 w-full text-xs"
        onClick={() =>
          onChange([
            ...list,
            {
              id: `st-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
              target: 10,
              duration: "30s",
              ramp: "gradual",
            },
          ])
        }
      >
        <Plus className="mr-1 h-3 w-3" /> {t("load.addStage")}
      </Button>
    </div>
  );
}

export function LoadModule() {
  const { t } = useT();
  const cfg = useAppStore((s) => s.loadTestConfig);
  const update = useAppStore((s) => s.updateLoadConfig);
  const metrics = useAppStore((s) => s.loadTestMetrics);
  const running = useAppStore((s) => s.loadTestRunning);
  const summary = useAppStore((s) => s.loadTestSummary);
  const start = useAppStore((s) => s.startLoadTest);
  const stop = useAppStore((s) => s.stopLoadTest);
  const requests = useAppStore((s) => s.requests);
  const saveReport = useAppStore((s) => s.saveLoadReport);
  const markBaseline = useAppStore((s) => s.markBaseline);
  const lastResult = useAppStore((s) => s._lastLoadTestResult);

  const [saveOpen, setSaveOpen] = useState(false);
  const [reportName, setReportName] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState("");
  const [baselineMsg, setBaselineMsg] = useState("");
  const [baselineAfterSave, setBaselineAfterSave] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportFormat, setExportFormat] = useState<string>("html");
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState("");
  const [agents, setAgents] = useState<AgentInfo[]>([]);

  useEffect(() => {
    let alive = true;
    const refresh = () =>
      listAgents()
        .then((a) => alive && setAgents(a))
        .catch(() => {});
    refresh();
    const iv = setInterval(refresh, 5000);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, []);

  const onExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      const cfgReq = cfg.requestId ? requests[cfg.requestId] : null;
      const { content, filename, truncated } = await exportLoadReport(
        exportFormat,
        cfgReq?.name ?? "Load Test",
      );
      const fmt = EXPORT_FORMATS.find((f) => f.value === exportFormat)!;
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
        setExportMsg(
          tFormat("load.exported", path) +
            (truncated ? t("load.exportedTruncated") : ""),
        );
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

  const latest = metrics[metrics.length - 1];

  return (
    <PanelGroup
      direction="horizontal"
      autoSaveId="load-layout"
      className="h-full"
    >
      <Panel
        defaultSize={25}
        minSize={25}
        className="overflow-auto border-r border-border bg-card/40 p-3"
      >
        <h3 className="mb-3 text-sm font-semibold">{t("load.title")}</h3>
        <div className="space-y-3">
          <div>
            <Label className="text-xs">{t("load.executor")}</Label>
            <Select
              value={cfg.executor}
              onValueChange={(v) => update({ executor: v as LoadExecutor })}
            >
              <SelectTrigger className="h-8 mt-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(
                  [
                    "constant-vus",
                    "ramping-vus",
                    "constant-arrival-rate",
                  ] as LoadExecutor[]
                ).map((k) => (
                  <SelectItem key={k} value={k}>
                    {t(EXECUTOR_KEYS[k])}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {cfg.executor === "constant-vus" && (
            <>
              <div>
                <Label className="text-xs">{t("load.vus")}</Label>
                <NumberField
                  value={cfg.vus}
                  onChange={(v) => update({ vus: v })}
                  className="h-8 mt-1"
                />
              </div>
              <div>
                <Label className="text-xs">{t("load.duration")}</Label>
                <Input
                  value={cfg.duration}
                  onChange={(e) => update({ duration: e.target.value })}
                  className="h-8 mt-1"
                  placeholder="30s / 5m"
                />
              </div>
              <div>
                <Label className="text-xs">{t("load.rampUp")}</Label>
                <Input
                  value={cfg.rampUp}
                  onChange={(e) => update({ rampUp: e.target.value })}
                  className="h-8 mt-1"
                  placeholder="5s"
                />
              </div>
            </>
          )}
          {cfg.executor === "ramping-vus" && (
            <>
              <div>
                <Label className="text-xs">{t("load.initialVu")}</Label>
                <NumberField
                  value={cfg.startVus}
                  onChange={(v) => update({ startVus: v })}
                  className="h-8 mt-1"
                />
              </div>
              <div>
                <Label className="text-xs">{t("load.maxVu")}</Label>
                <NumberField
                  value={cfg.maxVus}
                  onChange={(v) => update({ maxVus: v })}
                  className="h-8 mt-1"
                />
              </div>
              <div>
                <Label className="text-xs">{t("load.stages")}</Label>
                <div className="mt-1">
                  <StageList
                    withRamp
                    stages={cfg.stages}
                    onChange={(stages) => update({ stages })}
                  />
                </div>
              </div>
            </>
          )}
          {cfg.executor === "constant-arrival-rate" && (
            <div>
              <Label className="text-xs">{t("load.rate")}</Label>
              <NumberField
                value={cfg.rate}
                onChange={(v) => update({ rate: v })}
                className="h-8 mt-1"
              />
              <Label className="text-xs mt-2">{t("load.preallocVu")}</Label>
              <NumberField
                min={1}
                value={cfg.preAllocatedVus}
                onChange={(v) => update({ preAllocatedVus: v })}
                className="h-8 mt-1"
              />
              <Label className="text-xs mt-2">{t("load.duration")}</Label>
              <Input
                value={cfg.duration}
                onChange={(e) => update({ duration: e.target.value })}
                className="h-8 mt-1"
                placeholder="30s / 5m"
              />
              <Label className="text-xs mt-2">{t("load.rampUp")}</Label>
              <Input
                value={cfg.rampUp}
                onChange={(e) => update({ rampUp: e.target.value })}
                className="h-8 mt-1"
                placeholder="5s"
              />
            </div>
          )}
          <div>
            <Label className="text-xs">{t("load.targetRequest")}</Label>
            <RequestTreeSelect
              value={cfg.requestId ?? ""}
              onChange={(v) => update({ requestId: v || null })}
              placeholder={t("load.selectRequest")}
              className="mt-1 w-full"
            />
          </div>
          {/* Execution mode: local / distributed via agents */}
          <div className="rounded-md border border-border p-2">
            <div className="mb-1.5 text-xs font-medium">
              {t("load.executionMode")}
            </div>
            <AgentExecutionTarget agents={agents} />
          </div>
          <div className="flex items-center justify-between rounded-md border border-border p-2">
            <span className="text-xs">{t("load.ignoreBody")}</span>
            <Switch
              checked={cfg.ignoreBody}
              onCheckedChange={(c) => update({ ignoreBody: !!c })}
            />
          </div>

          {/* SLA gate configuration */}
          <style>{`input.no-spinner::-webkit-outer-spin-button,input.no-spinner::-webkit-inner-spin-button{-webkit-appearance:none;margin:0}input.no-spinner{appearance:textfield}`}</style>
          <div className="rounded-md border border-border p-2">
            <div className="mb-1.5 flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">
                {t("load.slaGate")}
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                className="h-5 w-5"
                onClick={() => {
                  update({
                    thresholds: [
                      ...(cfg.thresholds ?? []),
                      {
                        id: newId(),
                        metric: "p95",
                        condition: "<",
                        value: 500,
                      },
                    ],
                  });
                }}
              >
                <Plus className="h-3 w-3" />
              </Button>
            </div>
            <div className="space-y-1">
              {(cfg.thresholds ?? []).map((rule) => (
                <div key={rule.id} className="flex items-center gap-1 text-xs">
                  <select
                    value={rule.metric}
                    onChange={(e) =>
                      update({
                        thresholds: (cfg.thresholds ?? []).map((r) =>
                          r.id === rule.id
                            ? {
                                ...r,
                                metric: e.target
                                  .value as ThresholdRule["metric"],
                              }
                            : r,
                        ),
                      })
                    }
                    className="h-6 w-16 rounded border border-border bg-background px-1 text-xs text-foreground"
                  >
                    {METRIC_OPTIONS.map((opt) => (
                      <option
                        key={opt.value}
                        value={opt.value}
                        className="text-foreground"
                      >
                        {t(opt.labelKey as TKey)}
                      </option>
                    ))}
                  </select>
                  <select
                    value={rule.condition}
                    onChange={(e) =>
                      update({
                        thresholds: (cfg.thresholds ?? []).map((r) =>
                          r.id === rule.id
                            ? {
                                ...r,
                                condition: e.target
                                  .value as ThresholdRule["condition"],
                              }
                            : r,
                        ),
                      })
                    }
                    className="h-6 w-12 rounded border border-border bg-background px-1 text-xs text-foreground"
                  >
                    <option value="<" className="text-foreground">
                      &lt;
                    </option>
                    <option value="<=" className="text-foreground">
                      &le;
                    </option>
                    <option value=">" className="text-foreground">
                      &gt;
                    </option>
                    <option value=">=" className="text-foreground">
                      &ge;
                    </option>
                  </select>
                  <div className="relative flex-1">
                    <input
                      type="number"
                      step="any"
                      value={rule.value}
                      onChange={(e) =>
                        update({
                          thresholds: (cfg.thresholds ?? []).map((r) =>
                            r.id === rule.id
                              ? { ...r, value: parseFloat(e.target.value) || 0 }
                              : r,
                          ),
                        })
                      }
                      className="no-spinner h-6 w-full rounded border border-border bg-background pr-5 text-right text-xs font-mono tabular-nums text-foreground"
                    />
                    <span className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">
                      {rule.metric === "errorRate" ? "%" : "ms"}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="h-5 w-5 shrink-0"
                    onClick={() => {
                      update({
                        thresholds: (cfg.thresholds ?? []).filter(
                          (r) => r.id !== rule.id,
                        ),
                      });
                    }}
                  >
                    <Trash2 className="h-3 w-3 text-rose-400" />
                  </Button>
                </div>
              ))}
              {(cfg.thresholds ?? []).length === 0 && (
                <div className="text-xs text-muted-foreground">
                  {t("load.addThresholdHint")}
                </div>
              )}
            </div>
          </div>

          <Button className="w-full" onClick={start} disabled={running}>
            <Play className="h-4 w-4" />{" "}
            {running ? t("load.starting") : t("load.start")}
          </Button>
          {running && (
            <Button variant="outline" className="w-full" onClick={stop}>
              <Square className="h-4 w-4" /> {t("load.stop")}
            </Button>
          )}
        </div>
      </Panel>

      <PanelResizeHandle className="w-1 bg-border transition-colors hover:bg-primary" />

      <Panel minSize={35} className="overflow-auto p-3">
        <div className="mb-3 flex items-center gap-2">
          <Activity
            className={`h-4 w-4 ${running ? "animate-pulse text-emerald-400" : "text-muted-foreground"}`}
          />
          <span className="text-sm font-semibold">{t("load.metrics")}</span>
          {running && <Badge variant="success">{t("load.running")}</Badge>}
        </div>

        <div className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-4">
          <Metric
            label={t("load.vuLabel")}
            value={String(latest?.vus ?? 0)}
            unit="VU"
            color="text-blue-400"
          />
          <Metric
            label={t("load.throughputLabel")}
            value={String(latest?.rps ?? 0)}
            unit="RPS"
            color="text-emerald-400"
          />
          <Metric
            label={t("load.p95Label")}
            value={String(latest?.p95 ?? 0)}
            unit="ms"
            color="text-amber-400"
            timing={latest?.timing}
          />
          <Metric
            label={t("load.errorRateLabel")}
            value={String(latest?.errorRate ?? 0)}
            unit="%"
            color="text-rose-400"
            errors={latest?.error_breakdown}
          />
        </div>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">{t("load.chartTitle")}</CardTitle>
          </CardHeader>
          <CardContent>
            <LiveChart
              data={metrics}
              series={[
                { key: "rps", color: "#34d399", label: "RPS" },
                { key: "vus", color: "#38bdf8", label: "VU" },
                { key: "p99", color: "#fb7185", label: "P99(ms)" },
              ]}
              height={240}
            />
          </CardContent>
        </Card>

        {summary && (
          <Card className="mt-3">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("load.summary")}</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-2 text-sm md:grid-cols-4">
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryTotal")}
                  </div>
                  <div className="font-mono font-bold">
                    {summary.total_requests.toLocaleString()}
                  </div>
                </div>
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryAvgRps")}
                  </div>
                  <div className="font-mono font-bold">{summary.rps}</div>
                </div>
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryP50")}
                  </div>
                  <div className="font-mono font-bold">{summary.p50_ms}ms</div>
                </div>
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryP95")}
                  </div>
                  <div className="font-mono font-bold">{summary.p95_ms}ms</div>
                </div>
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryP99")}
                  </div>
                  <div className="font-mono font-bold">{summary.p99_ms}ms</div>
                </div>
                {summary.error_breakdown &&
                summary.error_breakdown.length > 0 ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <div className="cursor-help">
                        <div className="text-sm text-muted-foreground">
                          {t("load.summaryErrors")}
                        </div>
                        <div className="font-mono font-bold text-rose-400">
                          {summary.total_failures}
                        </div>
                      </div>
                    </TooltipTrigger>
                    <TooltipContent side="bottom" className="w-72 p-3">
                      <ErrorBreakdown
                        errors={summary.error_breakdown}
                        total={summary.total_failures}
                      />
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <div>
                    <div className="text-sm text-muted-foreground">
                      {t("load.summaryErrors")}
                    </div>
                    <div className="font-mono font-bold text-rose-400">
                      {summary.total_failures}
                    </div>
                  </div>
                )}
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryErrorRate")}
                  </div>
                  <div className="font-mono font-bold">
                    {summary.error_rate}%
                  </div>
                </div>
                <div>
                  <div className="text-sm text-muted-foreground">
                    {t("load.summaryDuration")}
                  </div>
                  <div className="font-mono font-bold">
                    {(summary.total_duration_ms / 1000).toFixed(0)}s
                  </div>
                </div>
              </div>

              {/* Error type distribution (always visible; hover for representative details) */}
              {summary.error_breakdown &&
                summary.error_breakdown.length > 0 && (
                  <div className="mt-3 border-t border-border pt-2">
                    <div className="mb-1.5 flex items-center gap-2 text-xs font-medium text-muted-foreground">
                      <span>{t("load.errDistribution")}</span>
                      <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-xs text-rose-400">
                        {t("load.hoverForDetail")}
                      </span>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {summary.error_breakdown.map((g) => {
                        const pct =
                          summary.total_failures > 0
                            ? (
                                (g.count / summary.total_failures) *
                                100
                              ).toFixed(1)
                            : "0.0";
                        return (
                          <Tooltip key={g.type}>
                            <TooltipTrigger asChild>
                              <span className="cursor-help rounded-full border border-border bg-card/60 px-2 py-0.5 text-xs">
                                <span className="text-foreground">
                                  {ERROR_LABEL_KEY[g.type]
                                    ? t(ERROR_LABEL_KEY[g.type] as TKey)
                                    : g.type}
                                </span>
                                <span className="ml-1 font-mono text-rose-400">
                                  {g.count}
                                </span>
                                <span className="ml-0.5 text-muted-foreground">
                                  ·{pct}%
                                </span>
                              </span>
                            </TooltipTrigger>
                            <TooltipContent side="bottom" className="w-72 p-3">
                              <ErrorBreakdown
                                errors={[g]}
                                total={summary.total_failures}
                              />
                            </TooltipContent>
                          </Tooltip>
                        );
                      })}
                    </div>
                  </div>
                )}

              {/* Threshold results */}
              {lastResult && lastResult.thresholds.length > 0 && (
                <div className="mt-3 border-t border-border pt-2">
                  <div className="text-xs font-medium text-muted-foreground mb-1">
                    {lastResult.all_thresholds_passed ? "✅" : "❌"}{" "}
                    {t("load.slaGate")}
                  </div>
                  {lastResult.thresholds.map((g, i) => (
                    <div
                      key={i}
                      className="flex items-center gap-1 text-xs text-muted-foreground"
                    >
                      <span
                        className={
                          g.passed ? "text-emerald-400" : "text-rose-400"
                        }
                      >
                        {g.passed ? "✓" : "✗"}
                      </span>
                      <span>{g.label}</span>
                      <span className="ml-auto font-mono">
                        {g.actual} / {g.target}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {/* Save buttons */}
              {lastResult && (
                <div className="mt-3 flex gap-2">
                  <Button
                    size="sm"
                    className="gap-1"
                    onClick={() => {
                      setReportName(
                        cfg.requestId
                          ? (requests[cfg.requestId]?.name ??
                              t("load.reportName"))
                          : t("load.reportName"),
                      );
                      setSaveOpen(true);
                    }}
                  >
                    <Save className="h-3.5 w-3.5" />
                    {t("perf.saveReport")}
                  </Button>
                  {isTauri() && (
                    <Button
                      size="sm"
                      variant="outline"
                      className="gap-1"
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
                    variant="outline"
                    className="gap-1"
                    onClick={() => {
                      setBaselineMsg("");
                      setBaselineAfterSave(true);
                      setReportName(
                        cfg.requestId
                          ? (requests[cfg.requestId]?.name ??
                              t("load.reportName"))
                          : t("load.reportName"),
                      );
                      setSaveOpen(true);
                    }}
                  >
                    <Star className="h-3.5 w-3.5" />
                    {t("perf.setBaseline")}
                  </Button>
                  {baselineMsg && (
                    <span className="text-xs text-muted-foreground">
                      {baselineMsg}
                    </span>
                  )}
                </div>
              )}
            </CardContent>
          </Card>
        )}

        {/* Save report dialog */}
        <Dialog
          open={saveOpen}
          onOpenChange={(o) => {
            if (!o) {
              setBaselineAfterSave(false);
              setSaveMsg("");
            }
            setSaveOpen(o);
          }}
        >
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>{t("perf.saveReport")}</DialogTitle>
            </DialogHeader>
            <Input
              value={reportName}
              onChange={(e) => setReportName(e.target.value)}
              placeholder={t("perf.reportNamePlaceholder")}
              className="mt-2"
            />
            {saveMsg && (
              <p className="mt-1 text-xs text-muted-foreground">{saveMsg}</p>
            )}
            <DialogFooter>
              <Button
                variant="ghost"
                onClick={() => {
                  setSaveOpen(false);
                  setSaveMsg("");
                  setBaselineAfterSave(false);
                }}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={async () => {
                  setSaving(true);
                  setSaveMsg("");
                  const cfgReq = cfg.requestId ? requests[cfg.requestId] : null;
                  const cfgHttp =
                    cfgReq && isHttpRequest(cfgReq) ? cfgReq : null;
                  const cfgLabel = cfgReq
                    ? cfgHttp
                      ? cfgHttp.method
                      : (PROTOCOL_LABEL[requestProtocol(cfgReq)] ??
                        requestProtocol(cfgReq))
                    : "";
                  const ep = cfgReq ? `${cfgLabel} ${cfgReq.url}` : "unknown";
                  try {
                    const id = await saveReport(
                      reportName || t("load.untitledReport"),
                      ep,
                      cfgHttp ? cfgHttp.method : "GET",
                    );
                    if (id) {
                      if (baselineAfterSave) {
                        await markBaseline(
                          id,
                          reportName || t("load.untitledReport"),
                        );
                        setBaselineMsg(t("load.baselineSet"));
                        setBaselineAfterSave(false);
                      }
                      setSaveMsg(t("load.saveOk"));
                      setTimeout(() => setSaveOpen(false), 800);
                    } else {
                      setSaveMsg(t("load.saveFailed"));
                    }
                  } catch (e: unknown) {
                    setSaveMsg(
                      tFormat(
                        "load.saveError",
                        (e as Error).message || String(e),
                      ),
                    );
                  } finally {
                    setSaving(false);
                  }
                }}
                disabled={saving}
              >
                {saving ? "..." : t("common.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

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
                  {EXPORT_FORMATS.map((f) => (
                    <SelectItem key={f.value} value={f.value}>
                      {t(f.labelKey as TKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {(exportFormat === "jtl" || exportFormat === "raw-json") && (
                <p className="text-xs leading-snug text-muted-foreground">
                  {t("load.rawSamplesHint")}
                </p>
              )}
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
