// Load-test domain: starting/stopping runs, the live metrics stream, reports and baseline management.
import type { StateCreator } from "zustand";
import type {
  LoadTestCompleteResult,
  LoadTestConfig,
  LoadTestMetrics,
  LoadTestSummary,
  SavedReport,
} from "@/data/types";
import { t, tFormat } from "@/lib/localeDict";
import { buildRequestUrl, resolveTemplate } from "@/lib/resolve";
import { getActiveBody, stripBodyComments } from "@/lib/requestBody";
import {
  connectLoadTestStream,
  isTauri,
  runLoadTest,
  stopLoadTest as stopLoadTestBackend,
  type LoadTestResult,
} from "@/lib/bridge";
import {
  connectDistributedEvents,
  connectDistributedTaskStream,
  dispatchRun,
  distributedResult,
  distributedStop,
  type AgentMetricsSnapshot,
  type DistributedResult,
} from "@/lib/bridge/distributed";
import { buildLoadYaml, parseDuration } from "../utils/yaml";
import type { AppState } from "../types";
import { isHttpRequest } from "@/data/types";

/** Load-test timer (legacy; now driven by SSE/events, kept in case polling mode returns) */
let loadTimer: ReturnType<typeof setInterval> | null = null;
/** Cleanup handle of the currently active load-test SSE stream */
let loadStreamCleanup: (() => void) | null = null;
/** Previous live snapshot (used to compute the instantaneous RPS) */
let lastLive = { requests: 0, time: 0 };

/** Estimate the planned total duration of a load test in milliseconds per executor; used for the final timeline and as a distributed timeout fallback */
function planDurationMs(cfg: LoadTestConfig): number {
  if (cfg.executor === "ramping-vus") {
    return (cfg.stages ?? []).reduce(
      (acc, s) => acc + parseDuration(s.duration),
      0,
    );
  }
  // constant-vus / constant-arrival-rate：duration + ramp_up
  return parseDuration(cfg.duration) + parseDuration(cfg.rampUp);
}

/** Number of VUs actually active at t=0 for this executor (so the chart starts at 0) */
function initialActiveVus(cfg: LoadTestConfig): number {
  switch (cfg.executor) {
    case "ramping-vus":
      return cfg.startVus || 0;
    case "constant-arrival-rate":
      return cfg.preAllocatedVus || cfg.vus;
    default:
      // constant-vus: without a ramp all VUs start immediately; with a ramp the first VU waits one interval
      return cfg.rampUp === "0s" || cfg.rampUp === "" ? cfg.vus : 0;
  }
}

/** Localized label of a ramp mode, used inline by the report config description. */
function rampLabel(ramp: string): string {
  return t(`perf.ramp.${ramp}`, ramp);
}

/** Ramp-up config for saved reports (following k6/JMeter conventions: show the executor and the full stages rather than a single VU/duration) */
function reportConfig(cfg: LoadTestConfig): {
  vus: number;
  duration: string;
  config: string;
} {
  switch (cfg.executor) {
    case "constant-vus":
      return {
        vus: cfg.vus,
        duration: cfg.duration,
        config: tFormat(
          "perf.config.constantVus",
          cfg.vus,
          cfg.duration,
          cfg.rampUp && cfg.rampUp !== "0s"
            ? tFormat("perf.config.rampUp", cfg.rampUp)
            : "",
        ),
      };
    case "ramping-vus": {
      const stages = (cfg.stages ?? [])
        .map((s) => {
          const ramp =
            s.ramp && s.ramp !== "gradual" ? `,${rampLabel(s.ramp)}` : "";
          return `${s.target}(${s.duration}${ramp})`;
        })
        .join(" → ");
      const maxVus = cfg.maxVus ?? 0;
      const totalMs = (cfg.stages ?? []).reduce(
        (acc, s) => acc + parseDuration(s.duration),
        0,
      );
      const rawPeak = Math.max(
        cfg.startVus ?? 0,
        ...(cfg.stages ?? []).map((s) => s.target),
      );
      const peak = maxVus > 0 ? Math.min(rawPeak, maxVus) : rawPeak;
      return {
        vus: peak,
        duration: `${Math.max(1, Math.round(totalMs / 1000))}s`,
        config: tFormat(
          "perf.config.rampingVus",
          cfg.startVus ?? 0,
          stages,
          maxVus > 0 ? tFormat("perf.config.maxVus", maxVus) : "",
        ),
      };
    }
    case "constant-arrival-rate":
      return {
        vus: cfg.preAllocatedVus || cfg.vus,
        duration: cfg.duration,
        config: tFormat(
          "perf.config.constantArrival",
          cfg.rate,
          cfg.duration,
          cfg.rampUp && cfg.rampUp !== "0s"
            ? tFormat("perf.config.rampUp", cfg.rampUp)
            : "",
        ),
      };
    default:
      return {
        vus: cfg.vus,
        duration: cfg.duration,
        config: tFormat("perf.config.default", cfg.vus, cfg.duration),
      };
  }
}

/** Distributed load-test runtime state (module-level, shared across event callbacks) */
interface DistRunState {
  taskId: string;
  expectedAgents: number;
  startedAt: number;
  /** Agents that already reported finished/failed */
  done: Set<string>;
}
let distRun: DistRunState | null = null;
let distStreamCleanup: (() => void) | null = null;
/** Latest metrics snapshot per agent (used for live aggregation) */
let distSnapshots = new Map<string, AgentMetricsSnapshot>();

/** Aggregate per-agent snapshots into one live metrics point */
function pushDistMetric(
  metrics: AgentMetricsSnapshot,
  start: number,
  set: (
    partial: Partial<AppState> | ((s: AppState) => Partial<AppState>),
  ) => void,
): void {
  // A snapshot from the same agent with the same timestamp is charted only once (polling re-reads the latest snapshot)
  const prev = distSnapshots.get(metrics.agent_id);
  if (prev && prev.timestamp_ms === metrics.timestamp_ms) return;
  distSnapshots.set(metrics.agent_id, metrics);
  let requests = 0;
  let errors = 0;
  let vus = 0;
  let rps = 0;
  let p95 = 0;
  let p99 = 0;
  let p95W = 0;
  let p99W = 0;
  for (const s of distSnapshots.values()) {
    requests += s.total_requests;
    errors += s.total_errors;
    vus += s.active_vus;
    if (s.summary) {
      rps += s.summary.rps;
      p95W += s.summary.p95_ms * s.total_requests;
      p99W += s.summary.p99_ms * s.total_requests;
      p95 += s.total_requests;
      p99 += s.total_requests;
    }
  }
  const elapsed = (Date.now() - start) / 1000;
  const point = {
    time: +elapsed.toFixed(1),
    vus,
    rps: Math.round(rps || (elapsed > 0 ? requests / elapsed : 0)),
    p95: p95 > 0 ? Math.round(p95W / p95) : 0,
    p99: p99 > 0 ? Math.round(p99W / p99) : 0,
    errorRate: requests > 0 ? +((errors / requests) * 100).toFixed(2) : 0,
  };
  // Keep the full history; the chart samples per viewport (so any range can be zoomed into)
  set((s) => ({ loadTestMetrics: [...s.loadTestMetrics, point] }));
}

/** Distributed result → the local LoadTestResult shape */
function distResultToLoadResult(
  r: DistributedResult,
  start: number,
): LoadTestResult {
  return {
    status: "completed",
    summary: {
      total_requests: r.total_requests,
      total_failures: r.total_errors,
      total_duration_ms: Math.max(1, Math.round((Date.now() - start) / 1000)),
      rps: Math.round(r.rps),
      p50_ms: Math.round(r.p50_ms),
      p90_ms: Math.round(r.p90_ms),
      p95_ms: Math.round(r.p95_ms),
      p99_ms: Math.round(r.p99_ms),
      p999_ms: Math.round(r.p999_ms),
      min_ms: 0,
      max_ms: 0,
      mean_ms: Math.round(r.mean_ms),
      error_rate: r.error_rate,
      total_bytes: 0,
      error_breakdown: [],
    },
    thresholds: [],
    all_thresholds_passed: true,
  };
}

/** Finalize a distributed load test: fetch the backend's aggregated result and finalize */
async function finishDistributed(
  taskId: string,
  start: number,
  finalize: (result: LoadTestResult | null) => void,
): Promise<void> {
  // Already stopped or replaced by a new run: do not finalize
  if (!distRun || distRun.taskId !== taskId) return;
  if (distStreamCleanup) {
    distStreamCleanup();
    distStreamCleanup = null;
  }
  distRun = null;
  try {
    const result = await distributedResult();
    if (result.total_requests === 0 && result.agent_count === 0) {
      finalize({ status: "error", error: t("load.noAgentMetrics") });
      return;
    }
    finalize(distResultToLoadResult(result, start));
  } catch (e) {
    console.error("[startLoadTest] distributed result error:", e);
    finalize({ status: "error", error: String(e) });
  }
}

export interface LoadTestSlice {
  loadTestConfig: LoadTestConfig;
  loadTestMetrics: LoadTestMetrics[];
  loadTestSummary: LoadTestSummary | null;
  loadTestRunning: boolean;
  loadTestPanelOpen: boolean;
  /** Most recent complete load-test result (used to save a report) */
  _lastLoadTestResult: LoadTestCompleteResult | null;

  savedReports: SavedReport[];
  baselines: SavedReport[];

  updateLoadConfig: (updates: Partial<LoadTestConfig>) => void;
  setLoadPanelOpen: (open: boolean) => void;
  startLoadTest: () => Promise<void>;
  stopLoadTest: () => void;
  /** Save the most recent load-test result as a report */
  saveLoadReport: (
    name: string,
    endpoint: string,
    method: string,
  ) => Promise<string | null>;
  /** Fetch the report list from the backend */
  fetchReports: () => Promise<void>;
  /** Fetch the baseline list from the backend */
  fetchBaselines: () => Promise<void>;
  /** Delete a report */
  deleteReport: (id: string) => Promise<void>;
  /** Rename a report */
  renameReport: (id: string, name: string) => Promise<void>;
  /** Set as baseline */
  markBaseline: (id: string, name: string) => Promise<void>;
  /** Unset baseline */
  unmarkBaseline: (id: string) => Promise<void>;
}

export const createLoadTestSlice: StateCreator<
  AppState,
  [],
  [],
  LoadTestSlice
> = (set, get) => ({
  loadTestConfig: {
    executor: "constant-vus",
    vus: 1,
    duration: "5s",
    rampUp: "0s",
    rate: 200,
    startVus: 0,
    maxVus: 0,
    stages: [{ id: "st-1", target: 10, duration: "30s", ramp: "gradual" }],
    preAllocatedVus: 50,
    requestId: "req-health",
    ignoreBody: true,
    thresholds: [],
  },
  loadTestMetrics: [],
  loadTestSummary: null,
  loadTestRunning: false,
  loadTestPanelOpen: false,
  _lastLoadTestResult: null,

  savedReports: [],
  baselines: [],

  updateLoadConfig: (updates) =>
    set((s) => ({ loadTestConfig: { ...s.loadTestConfig, ...updates } })),
  setLoadPanelOpen: (open) => set({ loadTestPanelOpen: open }),
  startLoadTest: async () => {
    if (get().loadTestRunning) return;
    const cfg = get().loadTestConfig;
    const durMs = planDurationMs(cfg);
    const reqRaw = cfg.requestId ? get().requests[cfg.requestId] : undefined;
    const req = reqRaw && isHttpRequest(reqRaw) ? reqRaw : undefined;

    // Seed the t=0 starting point: the backend SSE pushes its first frame after ~0.5s, so without this the chart would only start at 0.5s
    set({
      loadTestRunning: true,
      loadTestMetrics: [
        {
          time: 0,
          vus: initialActiveVus(cfg),
          rps: 0,
          p95: 0,
          p99: 0,
          errorRate: 0,
          error_breakdown: [],
        },
      ],
      loadTestSummary: null,
      loadTestPanelOpen: true,
    });
    lastLive = { requests: 0, time: 0 };
    get().track("load_start");

    // URL building shares one code path with one-off sends/scenarios (lib/resolve.buildRequestUrl):
    // keepDynamic=true keeps {{$...}} dynamic values verbatim in the YAML so the backend engine regenerates
    // them on every request — every load-test request gets fresh data
    const env = get().environments.find((e) => e.id === get().activeEnvId);
    const vars: Record<string, string> = {
      ...get().globalVariables,
      ...(env?.variables ?? {}),
    };
    const resolvedUrl = req
      ? await buildRequestUrl(req, vars, get().locale, true)
      : "";
    const headersRecord: Record<string, string> = {};
    for (const h of req?.headers ?? []) {
      if (h.enabled && h.key)
        headersRecord[h.key] = await resolveTemplate(
          h.value,
          vars,
          get().locale,
          true,
        );
    }
    // Use the body matching the current bodyMode (the bodyByMode slot) to match what the request manager saved;
    // avoid falling back to the stale imported `req.body`. Comments (JSON // and /* */, XML <!-- -->) are stripped before sending.
    const activeBody = req ? getActiveBody(req) : "";
    const strippedBody = activeBody
      ? stripBodyComments(activeBody, req?.bodyMode ?? "json")
      : "";
    const resolvedBody = strippedBody
      ? await resolveTemplate(strippedBody, vars, get().locale, true)
      : "";

    // Build the YAML config
    const yaml = buildLoadYaml(
      cfg,
      req,
      resolvedUrl,
      headersRecord,
      resolvedBody,
      // Script library table: references in the load-test YAML are expanded into concrete actions (self-contained file)
      get().actionTemplates,
    );

    // Debug: print the YAML sent to the backend (visible in the browser/Tauri console)
    console.log("[startLoadTest] YAML to send:", yaml);
    console.log("[startLoadTest] vus=", cfg.vus, "duration=", cfg.duration);
    console.log("[startLoadTest] resolvedUrl=", resolvedUrl);
    console.log("[startLoadTest] isTauri=", isTauri());

    const start = Date.now();
    let finalized = false;

    // Finalize: write the final summary / error and reset the running state. Idempotent, to avoid double finalization.
    const finalize = (result: LoadTestResult | null) => {
      if (finalized) return;
      finalized = true;

      // First append the last metrics point from the HTTP response (unaffected by the SSE closing)
      if (result && result.summary) {
        const s = result.summary;
        // The final point's time is the larger of the actual elapsed time and the planned total duration, so a staged strategy does not
        // drag the timeline back with cfg.duration (e.g. 5s) and collapse the chart's x-axis
        const finalTime = Math.max((Date.now() - start) / 1000, durMs / 1000);
        set((prev) => ({
          loadTestMetrics: [
            ...prev.loadTestMetrics,
            {
              time: +Math.max(
                finalTime,
                prev.loadTestMetrics.length > 0
                  ? prev.loadTestMetrics[prev.loadTestMetrics.length - 1].time
                  : 0,
              ).toFixed(1),
              vus:
                prev.loadTestMetrics.length > 0
                  ? prev.loadTestMetrics[prev.loadTestMetrics.length - 1].vus
                  : cfg.vus,
              // Instantaneous curve consistency: the final point reuses the last instantaneous RPS so it is not mixed with the
              // overall average, which would make the curve drop abruptly at the end (the average is shown in the summary card and reports).
              rps:
                prev.loadTestMetrics.length > 0
                  ? prev.loadTestMetrics[prev.loadTestMetrics.length - 1].rps
                  : Math.round(s.rps),
              p95: Math.round(s.p95_ms),
              p99: Math.round(s.p99_ms),
              errorRate:
                s.total_requests > 0
                  ? +((s.total_failures / s.total_requests) * 100).toFixed(2)
                  : 0,
              error_breakdown:
                (s.error_breakdown as unknown as
                  import("@/data/types").ErrorGroup[] | undefined) ?? [],
            },
          ],
        }));
      }

      if (loadStreamCleanup) {
        loadStreamCleanup();
        loadStreamCleanup = null;
      }
      if (result && result.status === "completed" && result.summary) {
        const s = result.summary;
        // Store the complete result (including thresholds) for "Save report"
        set({
          loadTestRunning: false,
          loadTestPanelOpen: false,
          _lastLoadTestResult: {
            status: "completed",
            summary: {
              ...s,
              p90_ms: s.p90_ms ?? 0,
              p999_ms: s.p999_ms ?? 0,
              mean_ms: s.mean_ms ?? 0,
              error_rate: s.error_rate ?? 0,
              total_bytes: s.total_bytes ?? 0,
            },
            thresholds: result.thresholds || [],
            all_thresholds_passed: result.all_thresholds_passed ?? true,
          },
          loadTestSummary: {
            total_requests: s.total_requests,
            total_failures: s.total_failures,
            total_duration_ms: s.total_duration_ms,
            rps: Math.round(s.rps),
            p50_ms: Math.round(s.p50_ms),
            p90_ms: Math.round(s.p90_ms ?? 0),
            p95_ms: Math.round(s.p95_ms),
            p99_ms: Math.round(s.p99_ms),
            p999_ms: Math.round(s.p999_ms ?? 0),
            min_ms: s.min_ms,
            max_ms: s.max_ms,
            mean_ms: Math.round(s.mean_ms ?? 0),
            error_rate:
              s.error_rate ??
              (s.total_requests > 0
                ? +((s.total_failures / s.total_requests) * 100).toFixed(2)
                : 0),
            total_bytes: s.total_bytes ?? 0,
            timing: s.timing,
            error_breakdown:
              (s.error_breakdown as unknown as
                import("@/data/types").ErrorGroup[] | undefined) ?? [],
          },
        });
      } else {
        set({
          loadTestRunning: false,
          loadTestPanelOpen: false,
          _lastLoadTestResult: null,
          loadTestSummary: {
            total_requests: 0,
            total_failures: 0,
            total_duration_ms: durMs,
            rps: 0,
            p50_ms: 0,
            p90_ms: 0,
            p95_ms: 0,
            p99_ms: 0,
            p999_ms: 0,
            min_ms: 0,
            max_ms: 0,
            mean_ms: 0,
            error_rate: 0,
            total_bytes: 0,
          },
        });
      }
      get().track("load_stop");
    };

    // Distributed execution: reuse the global executionTarget; in agent mode the load-test plan is dispatched to agents
    const execTarget = get().executionTarget;
    if (execTarget.mode === "agent") {
      const taskId = `task-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
      try {
        const res = await dispatchRun(yaml, taskId, execTarget.agentIds);
        if (!res.ok) throw new Error(t("load.dispatchFailed"));
        distRun = {
          taskId: res.task_id,
          expectedAgents: res.agents,
          startedAt: Date.now(),
          done: new Set(),
        };
        const runTaskId = distRun.taskId;
        const runExpectedAgents = distRun.expectedAgents;
        distSnapshots = new Map();
        if (distStreamCleanup) distStreamCleanup();
        if (isTauri()) {
          // Desktop: poll task snapshots/completion (a background emit contends with the window message loop for the lock and can hang)
          const handle = connectDistributedTaskStream(
            runTaskId,
            runExpectedAgents,
            (metrics) => pushDistMetric(metrics, start, set),
            () => {
              void finishDistributed(runTaskId, start, finalize);
            },
          );
          distStreamCleanup = handle.unlisten;
        } else {
          // Browser: SSE event stream
          const handle = connectDistributedEvents((ev) => {
            if (
              ev.type !== "task_event" ||
              !distRun ||
              ev.task_id !== distRun.taskId
            )
              return;
            if (ev.kind === "progress" && ev.metrics) {
              pushDistMetric(ev.metrics, start, set);
            } else if (ev.kind === "finished" || ev.kind === "failed") {
              if (distRun.done.has(ev.agent_id)) return;
              distRun.done.add(ev.agent_id);
              if (distRun.done.size >= distRun.expectedAgents) {
                void finishDistributed(distRun.taskId, start, finalize);
              }
            }
          });
          distStreamCleanup = handle.unlisten;
        }
      } catch (e) {
        console.error("[startLoadTest] distributed dispatch error:", e);
        finalize({ status: "error", error: String(e) });
        return;
      }
      // Fallback timeout finalization (planned duration + 30s margin, at least 60s)
      const timeoutMs = Math.max(durMs + 30000, 60000);
      setTimeout(() => {
        if (distRun?.taskId === taskId && !finalized) {
          void finishDistributed(taskId, start, finalize);
        }
      }, timeoutMs);
      return;
    }

    const onMessage = (data: string) => {
      try {
        const parsed = JSON.parse(data);
        if (parsed && typeof parsed === "object") {
          // Note: the load-test event stream and scenario runs share one broadcast channel, but step progress events
          // are written to scenarioResults only by the scenario runner's handler (the load-test UI does not display them),
          // so writing them here again would duplicate rows in scenario results. Only live metrics snapshots/final summary are handled.
          // Parse the live metrics snapshot (possibly the final summary)
          if (parsed.total_requests !== undefined || parsed.rps !== undefined) {
            const elapsed = (Date.now() - start) / 1000;
            // Instantaneous RPS: computed from the total-request delta between two ticks, avoiding the cumulative average being badly underestimated early on
            const now = Date.now();
            const dt = lastLive.time > 0 ? (now - lastLive.time) / 1000 : 0;
            const delta =
              typeof parsed.total_requests === "number"
                ? parsed.total_requests - lastLive.requests
                : 0;
            const instRps = dt > 0.2 && delta >= 0 ? Math.round(delta / dt) : 0;
            lastLive = {
              requests:
                typeof parsed.total_requests === "number"
                  ? parsed.total_requests
                  : lastLive.requests,
              time: now,
            };
            const metrics = {
              time: +elapsed.toFixed(1),
              vus: typeof parsed.vus === "number" ? parsed.vus : cfg.vus,
              rps:
                instRps > 0
                  ? instRps
                  : typeof parsed.rps === "number"
                    ? Math.round(parsed.rps)
                    : 0,
              p95:
                typeof parsed.p95_ms === "number"
                  ? Math.round(parsed.p95_ms)
                  : 0,
              p99:
                typeof parsed.p99_ms === "number"
                  ? Math.round(parsed.p99_ms)
                  : 0,
              errorRate:
                parsed.total_requests > 0
                  ? +(
                      ((parsed.total_failures || 0) / parsed.total_requests) *
                      100
                    ).toFixed(2)
                  : 0,
              timing: parsed.timing
                ? {
                    dns_ms: Math.round(parsed.timing.dns_ms * 10) / 10,
                    tcp_ms: Math.round(parsed.timing.tcp_ms * 10) / 10,
                    tls_ms: Math.round(parsed.timing.tls_ms * 10) / 10,
                    send_ms: Math.round(parsed.timing.send_ms * 10) / 10,
                    ttfb_ms: Math.round(parsed.timing.ttfb_ms * 10) / 10,
                    download_ms:
                      Math.round(parsed.timing.download_ms * 10) / 10,
                    total_ms: Math.round(parsed.timing.total_ms * 10) / 10,
                  }
                : undefined,
            };
            set((s) => ({
              loadTestMetrics: [...s.loadTestMetrics, metrics],
            }));
          }
        }
      } catch {
        /* Non-JSON data (e.g. text progress messages) is ignored */
      }
    };

    // Tauri: poll the final summary returned by load_progress; browser: finalize from runLoadTest's return value.
    const onDone = (data: string) => {
      console.log("[startLoadTest] final result received:", data);
      try {
        const parsed = JSON.parse(data);
        if (parsed && parsed.summary) {
          console.log("[startLoadTest] Summary:", parsed.summary);
          finalize(parsed);
        } else if (parsed && parsed.status === "error") {
          console.error("[startLoadTest] load-done error:", parsed.error);
          finalize({ status: "error", error: parsed.error });
        }
      } catch {
        /* ignore malformed payload */
      }
    };

    if (isTauri()) {
      // Desktop: start the load test first (the command resets backend progress when it starts), then connect the polling stream,
      // so polling does not read the previous run's leftover final result before the command resets it and finalize early.
      let result: LoadTestResult;
      try {
        result = await runLoadTest({
          yaml,
          vus: cfg.vus,
          duration: cfg.duration,
        });
      } catch (e) {
        console.error("[startLoadTest] Error:", e);
        finalize({ status: "error", error: String(e) });
        return;
      }
      console.log("[startLoadTest] runLoadTest result:", result);
      if (loadStreamCleanup) loadStreamCleanup();
      const handle = connectLoadTestStream(onMessage, onDone, () => {
        /* polling mode has no SSE errors */
      });
      loadStreamCleanup = handle.unlisten;
      // Finalize here only as a fallback when the command itself reports an error (e.g. an internal exception); normally the polled final result drives it.
      if (result.status === "error") {
        console.error("[startLoadTest] Tauri command returned error:", result);
        finalize(result);
      }
    } else {
      // Browser: connect the SSE stream before blocking on the run, so live metrics are not missed.
      if (loadStreamCleanup) loadStreamCleanup();
      const handle = connectLoadTestStream(onMessage, onDone, () => {
        /* SSE errors are handled silently */
      });
      loadStreamCleanup = handle.unlisten;
      try {
        await handle.ready;
      } catch {
        /* ignore */
      }
      try {
        const result = await runLoadTest({
          yaml,
          vus: cfg.vus,
          duration: cfg.duration,
        });
        console.log("[startLoadTest] runLoadTest result:", result);
        finalize(result);
      } catch (e) {
        console.error("[startLoadTest] Error:", e);
        finalize({ status: "error", error: String(e) });
      }
    }
  },
  stopLoadTest: () => {
    if (loadTimer) {
      clearInterval(loadTimer);
      loadTimer = null;
    }
    if (distRun) {
      void distributedStop(distRun.taskId).catch(() => {});
    }
    if (distStreamCleanup) {
      distStreamCleanup();
      distStreamCleanup = null;
    }
    distRun = null;
    void stopLoadTestBackend().catch(() => {});
    set({ loadTestRunning: false, loadTestPanelOpen: false });
  },

  fetchReports: async () => {
    try {
      const { listReports } = await import("@/lib/bridge");
      // Reports are isolated per workspace; fetch only the current workspace's reports
      const reports = await listReports(get().activeWorkspaceId ?? undefined);
      set({ savedReports: reports });
    } catch {
      /* silent when the backend is unavailable */
    }
  },

  fetchBaselines: async () => {
    try {
      const { listBaselines } = await import("@/lib/bridge");
      const baselines = await listBaselines(
        get().activeWorkspaceId ?? undefined,
      );
      set({ baselines });
    } catch {
      /* silent when the backend is unavailable */
    }
  },

  saveLoadReport: async (name, endpoint, method) => {
    const result = get()._lastLoadTestResult;
    if (!result || !result.summary) {
      console.warn("[saveReport] No load test result to save");
      return null;
    }
    try {
      const { saveReport } = await import("@/lib/bridge");
      const cfg = get().loadTestConfig;
      const { vus, duration, config } = reportConfig(cfg);
      const resp = await saveReport({
        name,
        endpoint,
        method,
        vus,
        duration,
        config,
        summary: result.summary,
        thresholds: result.thresholds || [],
        all_thresholds_passed: result.all_thresholds_passed ?? true,
        workspaceId: get().activeWorkspaceId ?? undefined,
      });
      if ("id" in resp) {
        await get().fetchReports();
        return resp.id;
      }
      console.error("[saveReport] Failed:", resp);
      return null;
    } catch (e) {
      console.error("[saveReport]", e);
      return null;
    }
  },

  deleteReport: async (id) => {
    try {
      const { deleteReport } = await import("@/lib/bridge");
      await deleteReport(id);
      await get().fetchReports();
      await get().fetchBaselines();
    } catch (e) {
      console.error("[deleteReport]", e);
    }
  },

  renameReport: async (id, name) => {
    try {
      const { renameReport: renameBridge } = await import("@/lib/bridge");
      await renameBridge(id, name);
      await get().fetchReports();
    } catch (e) {
      console.error("[renameReport]", e);
    }
  },

  markBaseline: async (id, name) => {
    try {
      const { setBaseline } = await import("@/lib/bridge");
      await setBaseline(id, name);
      await get().fetchReports();
      await get().fetchBaselines();
    } catch (e) {
      console.error("[markBaseline]", e);
    }
  },

  unmarkBaseline: async (id) => {
    try {
      const { unsetBaseline } = await import("@/lib/bridge");
      await unsetBaseline(id);
      await get().fetchReports();
      await get().fetchBaselines();
    } catch (e) {
      console.error("[unmarkBaseline]", e);
    }
  },
});
