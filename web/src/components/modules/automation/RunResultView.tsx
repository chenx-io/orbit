// Unified run-result view: the "current run" panel and the "history report detail" share the same metrics and detail filter,
// ensuring the two UIs are perfectly consistent (environment / iterations / pass / fail / total time / request time / avg time / assertion count).
import {
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  CircleDashed,
  XCircle,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { ScenarioRunStep } from "@/data/types";
import type { ResultFilterValue } from "./stepFilter";

/** Run metrics (shared shape between live progress and history report) */
export interface RunMetrics {
  envName: string | null;
  iterations: number;
  pass: number;
  fail: number;
  durationMs: number;
  /** Total API request time (ms) */
  requestMs: number;
  /** Number of API requests (used for averaging) */
  requestCount: number;
  /** Total assertion count */
  assertCount: number;
}

function fmtMs(ms: number): string {
  if (!ms || ms < 0) return "0ms";
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  return `${Math.round(ms)}ms`;
}

/** Metrics bar: two rows —— row 1 (run overview) environment / iterations / pass / fail; row 2 (timing) total time / request time / avg time / assertions */
export function RunMetricsBar({ m }: { m: RunMetrics }) {
  const { t } = useT();
  const avg = m.requestCount > 0 ? Math.round(m.requestMs / m.requestCount) : 0;
  const rows: {
    label: string;
    value: string;
    tone?: "pass" | "fail";
  }[][] = [
    [
      { label: t("run.metric.env"), value: m.envName ?? "—" },
      { label: t("run.metric.iterations"), value: String(m.iterations) },
      { label: t("run.metric.pass"), value: String(m.pass), tone: "pass" },
      { label: t("run.metric.fail"), value: String(m.fail), tone: "fail" },
    ],
    [
      { label: t("run.metric.totalDuration"), value: fmtMs(m.durationMs) },
      { label: t("run.metric.requestDuration"), value: fmtMs(m.requestMs) },
      { label: t("run.metric.avgRequest"), value: fmtMs(avg) },
      { label: t("run.metric.assertions"), value: String(m.assertCount) },
    ],
  ];
  return (
    <div className="space-y-0.5 border-b border-border px-3 py-1.5 text-xs">
      {rows.map((row, i) => (
        <div key={i} className="flex flex-wrap items-center gap-x-4 gap-y-1">
          {row.map((it) => (
            <span key={it.label} className="flex items-center gap-1">
              <span className="text-muted-foreground">{it.label}</span>
              <span
                className={cn(
                  "font-medium",
                  it.tone === "pass" && "text-emerald-500",
                  it.tone === "fail" && "text-rose-500",
                )}
              >
                {it.value}
              </span>
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}

/** All / pass / fail toggle (shared TabsList/TabsTrigger pill style) */
export function ResultFilterBar({
  value,
  onChange,
}: {
  value: ResultFilterValue;
  onChange: (v: ResultFilterValue) => void;
}) {
  const { t } = useT();
  const opts: { key: ResultFilterValue; label: string }[] = [
    { key: "all", label: t("run.filter.all") },
    { key: "pass", label: t("run.filter.pass") },
    { key: "fail", label: t("run.filter.fail") },
  ];
  return (
    <Tabs value={value} onValueChange={(v) => onChange(v as ResultFilterValue)}>
      <TabsList className="h-7">
        {opts.map((o) => (
          <TabsTrigger key={o.key} value={o.key} className="px-2.5 text-xs">
            {o.label}
          </TabsTrigger>
        ))}
      </TabsList>
    </Tabs>
  );
}

/** Request/response detail card (expands when request details are recorded) */
function RequestDetailCard({ req }: { req: ScenarioRunStep["request"] }) {
  if (!req) return null;
  return (
    <div className="mt-1 space-y-2 rounded border border-border bg-muted/40 p-2 text-[11px]">
      {/* Request line */}
      <div className="flex items-center gap-1.5">
        {req.method && (
          <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 font-mono font-medium text-emerald-500">
            {req.method}
          </span>
        )}
        <span className="truncate font-mono text-foreground/90">
          {req.target}
        </span>
      </div>

      {req.error && <div className="text-rose-500">{req.error}</div>}

      {/* Request headers */}
      {req.requestHeaders.length > 0 && (
        <DetailSection title="request.headers">
          {req.requestHeaders.map(([k, v], i) => (
            <Row key={i} k={k} v={v} />
          ))}
        </DetailSection>
      )}

      {/* Request body */}
      {req.requestBody && (
        <DetailSection title="request.body" truncated={req.requestTruncated}>
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono text-foreground/90">
            {req.requestBody}
          </pre>
        </DetailSection>
      )}

      {/* Response status */}
      <div className="flex items-center gap-1.5">
        <span className="text-muted-foreground">response.status</span>
        <span
          className={cn(
            "font-mono font-medium",
            req.status >= 200 && req.status < 400
              ? "text-emerald-500"
              : req.status > 0
                ? "text-rose-500"
                : "text-muted-foreground",
          )}
        >
          {req.status || "—"}
        </span>
        <span className="text-muted-foreground">
          ({req.responseSize} bytes)
        </span>
      </div>

      {/* Response headers */}
      {req.responseHeaders.length > 0 && (
        <DetailSection title="response.headers">
          {req.responseHeaders.map(([k, v], i) => (
            <Row key={i} k={k} v={v} />
          ))}
        </DetailSection>
      )}

      {/* Response body */}
      {req.responseBody && (
        <DetailSection title="response.body" truncated={req.responseTruncated}>
          <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all font-mono text-foreground/90">
            {req.responseBody}
          </pre>
        </DetailSection>
      )}
    </div>
  );
}

function DetailSection({
  title,
  truncated,
  children,
}: {
  title: string;
  truncated?: boolean;
  children: ReactNode;
}) {
  const { t } = useT();
  return (
    <div>
      <div className="mb-0.5 flex items-center gap-1 text-muted-foreground">
        <span className="font-mono">{title}</span>
        {truncated && (
          <span className="rounded bg-amber-500/15 px-1 text-amber-500">
            {t("run.detail.truncated")}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex gap-1.5 font-mono leading-relaxed">
      <span className="shrink-0 text-sky-500/90">{k}:</span>
      <span className="min-w-0 break-all text-foreground/85">{v}</span>
    </div>
  );
}

/** Detail step list (shared with history report; steps with request details recorded can expand to view request/response) */
export function StepList({ steps }: { steps: ScenarioRunStep[] }) {
  const { t, format } = useT();
  const [open, setOpen] = useState<Set<number>>(new Set());
  if (steps.length === 0) {
    return (
      <div className="py-6 text-center text-xs text-muted-foreground">
        {t("run.noSteps")}
      </div>
    );
  }
  const toggle = (seq: number) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(seq)) next.delete(seq);
      else next.add(seq);
      return next;
    });
  return (
    <div className="space-y-0.5">
      {steps.map((s) => (
        <div key={s.seq}>
          <div
            className={cn(
              "flex items-center gap-1.5 rounded px-1.5 py-1 text-xs",
              s.request
                ? "cursor-pointer hover:bg-accent/10"
                : "hover:bg-accent/10",
            )}
            onClick={s.request ? () => toggle(s.seq) : undefined}
            title={s.request ? t("run.detail.expandHint") : undefined}
          >
            {s.request ? (
              open.has(s.seq) ? (
                <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              )
            ) : s.status === "pass" ? (
              <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
            ) : s.status === "fail" ? (
              <XCircle className="h-3.5 w-3.5 shrink-0 text-rose-500" />
            ) : (
              <CircleDashed className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            )}
            <span className="min-w-0 flex-1 truncate">{s.name}</span>
            {s.rowIndex != null && (
              <span className="shrink-0 text-muted-foreground">
                {format("run.rowLabel", s.rowIndex + 1)}
              </span>
            )}
            {s.iteration != null && s.iteration > 0 && (
              <span className="shrink-0 text-muted-foreground">
                {format("run.iterLabel", s.iteration + 1)}
              </span>
            )}
            {s.durationMs > 0 && (
              <span className="shrink-0 text-muted-foreground">
                {s.durationMs}ms
              </span>
            )}
          </div>
          {open.has(s.seq) && s.request && (
            <div className="pl-4">
              <RequestDetailCard req={s.request} />
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
