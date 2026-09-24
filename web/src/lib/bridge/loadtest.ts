// Load tests and automation scenario runs: the real backend orbit-engine, with a live metrics stream.
import { t } from "@/lib/localeDict";
import { apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

export interface LoadTestConfig {
  yaml: string;
  vus: number;
  duration: string;
}

export interface LoadTestResult {
  status: "completed" | "error";
  summary?: {
    total_requests: number;
    total_failures: number;
    total_duration_ms: number;
    rps: number;
    p50_ms: number;
    p90_ms?: number;
    p95_ms: number;
    p99_ms: number;
    p999_ms?: number;
    min_ms: number;
    max_ms: number;
    mean_ms?: number;
    error_rate?: number;
    total_bytes?: number;
    timing?: {
      dns_ms: number;
      tcp_ms: number;
      tls_ms: number;
      send_ms: number;
      ttfb_ms: number;
      download_ms: number;
      total_ms: number;
    };
    error_breakdown?: Array<{ type: string; count: number; sample: string }>;
  };
  thresholds?: Array<{
    label: string;
    actual: number;
    target: number;
    passed: boolean;
    abort_on_fail: boolean;
  }>;
  all_thresholds_passed?: boolean;
  error?: string;
}

export async function runLoadTest(
  config: LoadTestConfig,
): Promise<LoadTestResult> {
  if (isTauri()) {
    console.log(
      "[bridge] Tauri run_load_test called with vus=",
      config.vus,
      "duration=",
      config.duration,
    );
    // Tauri v2 matches command arguments by name: the backend signature is run_load_test(app, state, request: RunLoadTestRequest),
    // so the config must be wrapped in the request key
    return tauriInvoke<LoadTestResult>("run_load_test", { request: config });
  }
  return apiPost<LoadTestResult>("/api/run", config);
}

/** Stop the running load test (effective in Tauri desktop mode only; the browser mode has no matching endpoint). */
export async function stopLoadTest(): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("stop_load_test");
  } else {
    // Browser mode: POST /api/stop to tell the backend to cancel
    await apiPost<unknown>("/api/stop", {});
  }
}

export interface LoadStreamHandle {
  /** Close the stream (cancel the SSE subscription / event listener). */
  unlisten: () => void;
  /** Resolves once the listener is ready (the Tauri event is registered), ensuring the stream is connected before the run starts. */
  ready: Promise<void>;
}

/** Connect to the live metrics stream, returning a handle and a readiness promise.
 *
 * - Browser preview mode: connect to the `/api/stream` SSE endpoint of `orbit-server`; `onMessage` receives live snapshots.
 * - Tauri desktop mode: poll the `load_progress` command to read shared state (live metrics snapshot + final summary).
 *   `onDone` fires once the final result is polled, used for teardown (the desktop `run_load_test` is non-blocking and
 *   does not carry the final result in its return value).
 *   Note: Tauri events (`load-progress`/`load-done`) are not used — a background-thread emit would contend
 *   with the window message loop for the same webview lock, freezing the app on drag/resize (tauri#9453).
 */
export function connectLoadTestStream(
  onMessage: (data: string) => void,
  onDone?: (data: string) => void,
  onError?: (err: Event) => void,
): LoadStreamHandle {
  const API_BASE: string =
    (import.meta.env.VITE_API_BASE as string | undefined) ||
    "http://localhost:8788";
  let cleanup: (() => void) | null = null;
  let resolveReady!: () => void;
  const ready = new Promise<void>((res) => {
    resolveReady = res;
  });

  if (isTauri()) {
    let stopped = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const poll = async () => {
      if (stopped) return;
      try {
        const st = await tauriInvoke<{
          snapshot: string | null;
          done: string | null;
          running: boolean;
        }>("load_progress");
        if (st.done) {
          onDone?.(st.done);
          stopped = true;
          if (timer) {
            clearInterval(timer);
            timer = null;
          }
          return;
        }
        if (st.snapshot) onMessage(st.snapshot);
      } catch {
        // The backend is not ready yet; retry on the next tick
      }
    };
    void poll();
    timer = setInterval(() => void poll(), 500);
    resolveReady();
    cleanup = () => {
      stopped = true;
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    };
  } else {
    const url = `${API_BASE}/api/stream`;
    const es = new EventSource(url);
    es.onmessage = (e) => onMessage(e.data);
    if (onError) es.onerror = onError;
    cleanup = () => es.close();
    resolveReady();
  }

  return {
    unlisten: () => cleanup?.(),
    ready,
  };
}

/** Pull automation scenario step progress (Tauri polling drain; the browser mode has no such endpoint and returns an empty array). */
export async function drainScenarioProgress(): Promise<string[]> {
  if (!isTauri()) return [];
  return tauriInvoke<string[]>("scenario_progress");
}

export interface LoadReportExport {
  content: string;
  filename: string;
  /** Whether a raw-sample export (jtl/raw-json) was truncated by the memory cap */
  truncated: boolean;
}

/** Export the most recent load-test report (desktop): format supports html/json/csv/junit/jtl/raw-json */
export async function exportLoadReport(
  format: string,
  name?: string,
): Promise<LoadReportExport> {
  if (!isTauri()) {
    throw new Error(t("perf.exportDesktopOnly"));
  }
  return tauriInvoke<LoadReportExport>("export_load_report", {
    format,
    name: name ?? null,
  });
}

/** Run an automation scenario (reusing the run_load_test pipeline; the Tauri side uses the run_scenario command) */
export async function runScenario(config: {
  yaml: string;
  vus: number;
  duration: string;
  /** Whether request/response details are captured (the record-request-details switch) */
  recordDetails?: boolean;
}): Promise<LoadTestResult> {
  if (isTauri()) {
    console.log("[bridge] Tauri run_scenario called");
    return tauriInvoke<LoadTestResult>("run_scenario", {
      request: config as unknown as Record<string, unknown>,
    });
  }
  return apiPost<LoadTestResult>("/api/run", config);
}
