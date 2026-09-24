// Distributed load-test management bridge (agent list / add / pause / resume / remove / task dispatch)

import { t, tFormat } from "@/lib/localeDict";
import { apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

export type AgentState = "idle" | "running" | "paused" | "offline";

export interface AgentResource {
  cpu_percent: number;
  mem_used_mb: number;
  mem_total_mb: number;
  mem_percent: number;
  load_avg_1m: number;
  timestamp_ms: number;
}

export interface AgentInfo {
  id: string;
  mode: string;
  addr: string;
  version: string;
  cpu_cores: number;
  memory_mb: number;
  labels: [string, string][];
  state: AgentState;
  degraded: boolean;
  taken_over?: boolean;
  last_heartbeat_ms: number;
  registered_at_ms: number;
  task_id: string | null;
  resource: AgentResource | null;
  resource_history: AgentResource[];
}

/** Load-test metrics snapshot reported by an agent */
export interface AgentMetricsSnapshot {
  agent_id: string;
  timestamp_ms: number;
  active_vus: number;
  total_requests: number;
  total_errors: number;
  hdr_histogram_b64: string;
  summary: {
    p50_ms: number;
    p90_ms: number;
    p95_ms: number;
    p99_ms: number;
    mean_ms: number;
    rps: number;
    error_rate: number;
  } | null;
}

/** Aggregated distributed load-test result */
export interface DistributedResult {
  total_requests: number;
  total_errors: number;
  error_rate: number;
  rps: number;
  p50_ms: number;
  p90_ms: number;
  p95_ms: number;
  p99_ms: number;
  p999_ms: number;
  mean_ms: number;
  agent_count: number;
  agent_stats: AgentMetricsSnapshot[];
}

export type RegistryEvent =
  | { type: "agent_updated"; agent: AgentInfo }
  | { type: "agent_removed"; agent_id: string }
  | {
      type: "task_event";
      agent_id: string;
      task_id: string;
      kind: "started" | "progress" | "finished" | "failed";
      state: AgentState;
      metrics: AgentMetricsSnapshot | null;
    };

export async function listAgents(): Promise<AgentInfo[]> {
  if (isTauri()) return tauriInvoke<AgentInfo[]>("distributed_agents");
  return apiGet<AgentInfo[]>("/api/distributed/agents");
}

export async function addAgent(
  addr: string,
  agentId?: string,
  labels?: [string, string][],
  force?: boolean,
): Promise<
  | { ok: true; agent: AgentInfo }
  | { ok: false; claimed: true; agentId: string; message: string }
> {
  const payload = {
    addr,
    agentId: agentId ?? null,
    labels: labels ?? null,
    force: force ?? false,
  };
  if (isTauri()) {
    const r = await tauriInvoke<any>("distributed_add_agent", {
      request: payload,
    });
    if (r && r.claimed) {
      return {
        ok: false,
        claimed: true,
        agentId: r.agent_id,
        message: r.error,
      };
    }
    return { ok: true, agent: r as AgentInfo };
  }
  const API_BASE: string =
    (import.meta.env.VITE_API_BASE as string | undefined) ||
    "http://localhost:8788";
  const res = await fetch(`${API_BASE}/api/distributed/agent`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const data: any = await res.json().catch(() => ({}));
  if (data && data.claimed) {
    return {
      ok: false,
      claimed: true,
      agentId: data.agent_id,
      message: data.error ?? t("distributed.agentInUse"),
    };
  }
  if (!res.ok) {
    throw new Error(
      data?.error ??
        tFormat("bridge.requestFailed", res.status, res.statusText),
    );
  }
  return { ok: true, agent: data as AgentInfo };
}

export async function agentAction(
  id: string,
  action: "pause" | "resume" | "remove" | "ping",
): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("distributed_agent_action", { id, action });
    return;
  }
  await apiPost<{ ok: boolean }>(
    `/api/distributed/agent/${encodeURIComponent(id)}/${action}`,
    {},
  );
}

export async function dispatchRun(
  yaml: string,
  taskId?: string,
  agentIds?: string[] | null,
): Promise<{ ok: boolean; task_id: string; agents: number }> {
  const payload = { yaml, taskId: taskId ?? null, agentIds: agentIds ?? null };
  if (isTauri()) {
    return tauriInvoke<{ ok: boolean; task_id: string; agents: number }>(
      "distributed_run",
      { request: payload },
    );
  }
  return apiPost<{ ok: boolean; task_id: string; agents: number }>(
    "/api/distributed/run",
    payload,
  );
}

/** Stop a distributed load-test task (sends stop to the agent running it) */
export async function distributedStop(
  taskId: string,
): Promise<{ ok: boolean; stopped: number }> {
  const payload = { taskId };
  if (isTauri()) {
    return tauriInvoke<{ ok: boolean; stopped: number }>("distributed_stop", {
      request: payload,
    });
  }
  return apiPost<{ ok: boolean; stopped: number }>(
    "/api/distributed/stop",
    payload,
  );
}

/** Take the current aggregated distributed load-test result */
export async function distributedResult(): Promise<DistributedResult> {
  if (isTauri()) {
    return tauriInvoke<DistributedResult>("distributed_result");
  }
  return apiGet<DistributedResult>("/api/distributed/result");
}

/** Live task progress (Tauri polling: the latest metrics snapshot per agent plus the list of finished agents). */
export interface DistributedTaskProgress {
  snapshots: AgentMetricsSnapshot[];
  done_agents: string[];
}

export async function distributedTaskProgress(
  taskId: string,
): Promise<DistributedTaskProgress> {
  if (isTauri()) {
    return tauriInvoke<DistributedTaskProgress>("distributed_task_progress", {
      taskId,
    });
  }
  // Browser mode keeps using SSE (/api/distributed/events); this interface exists for Tauri polling only.
  return { snapshots: [], done_agents: [] };
}

/** Tauri: poll live task metrics and completion state (replacing the `distributed-event` push, avoiding the emit deadlock). */
export function connectDistributedTaskStream(
  taskId: string,
  expectedAgents: number,
  onMetrics: (m: AgentMetricsSnapshot) => void,
  onFinish: () => void,
): { unlisten: () => void } {
  if (!isTauri()) {
    return { unlisten: () => {} };
  }
  let stopped = false;
  let timer: ReturnType<typeof setInterval> | null = null;
  const poll = async () => {
    if (stopped) return;
    try {
      const p = await distributedTaskProgress(taskId);
      for (const m of p.snapshots) onMetrics(m);
      if (p.done_agents.length >= expectedAgents) {
        stopped = true;
        if (timer) {
          clearInterval(timer);
          timer = null;
        }
        onFinish();
      }
    } catch {
      // The backend is not ready yet; retry on the next tick
    }
  };
  void poll();
  timer = setInterval(() => void poll(), 800);
  return {
    unlisten: () => {
      stopped = true;
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    },
  };
}

export interface ExecuteRequestInput {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
  protocol?: string;
  requestFormat?: string;
  responseFormat?: string;
  prereqScript?: string | null;
  postreqScript?: string | null;
  envVars?: Record<string, string>;
}

export interface ExecuteResult {
  status: number;
  headers: Record<string, string>;
  body: string;
  duration_ms: number;
  error?: string | null;
  timing?: {
    dns_ms: number;
    tcp_ms: number;
    tls_ms: number;
    ttfb_ms: number;
    download_ms: number;
    total_ms: number;
  } | null;
  pre_logs?: { level: string; message: string }[];
  post_logs?: { level: string; message: string }[];
  decoded?: string | null;
  post_tests?: { name: string; passed: boolean; message: string }[];
}

/** Execute a single request on the given agent (any request can go through an agent) */
export async function executeOnAgent(
  agentId: string,
  input: ExecuteRequestInput,
): Promise<ExecuteResult> {
  const payload = {
    method: input.method,
    url: input.url,
    headers: input.headers,
    body: input.body,
    protocol: input.protocol ?? "http",
    requestFormat: input.requestFormat ?? null,
    responseFormat: input.responseFormat ?? null,
    prereqScript: input.prereqScript ?? null,
    postreqScript: input.postreqScript ?? null,
    envVars: input.envVars ?? {},
  };
  if (isTauri()) {
    return tauriInvoke<ExecuteResult>("distributed_execute", {
      id: agentId,
      request: payload,
    });
  }
  return apiPost<ExecuteResult>(
    `/api/distributed/agent/${encodeURIComponent(agentId)}/execute`,
    payload,
  );
}

export async function controllerStatus(): Promise<{
  running: boolean;
  addr: string | null;
}> {
  if (isTauri())
    return tauriInvoke<{ running: boolean; addr: string | null }>(
      "distributed_controller_status",
    );
  return apiGet<{ running: boolean; addr: string | null }>(
    "/api/distributed/controller",
  );
}

export async function controllerStart(
  port: number,
): Promise<{ running: boolean; addr: string }> {
  const payload = { port };
  if (isTauri()) {
    return tauriInvoke<{ running: boolean; addr: string }>(
      "distributed_controller_start",
      {
        request: payload,
      },
    );
  }
  return apiPost<{ running: boolean; addr: string }>(
    "/api/distributed/controller/start",
    payload,
  );
}

export async function controllerStop(): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("distributed_controller_stop");
    return;
  }
  await apiPost<{ ok: boolean }>("/api/distributed/controller/stop", {});
}

export function connectDistributedEvents(
  onEvent: (ev: RegistryEvent) => void,
): { unlisten: () => void } {
  const API_BASE: string =
    (import.meta.env.VITE_API_BASE as string | undefined) ||
    "http://localhost:8788";
  if (isTauri()) {
    // Tauri: no longer listens for `distributed-event` (a background emit contending with the window message loop for the lock would hang),
    // instead polling the agent list and synthesizing add/remove/update events from the diff.
    let stopped = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    let last = new Map<string, AgentInfo>();
    const poll = async () => {
      if (stopped) return;
      try {
        const agents = await listAgents();
        const next = new Map(agents.map((a) => [a.id, a]));
        const seen = new Set(next.keys());
        for (const id of last.keys()) {
          if (!seen.has(id)) {
            onEvent({ type: "agent_removed", agent_id: id });
          }
        }
        for (const [id, agent] of next) {
          const prev = last.get(id);
          if (!prev) {
            onEvent({ type: "agent_updated", agent });
          } else if (JSON.stringify(prev) !== JSON.stringify(agent)) {
            onEvent({ type: "agent_updated", agent });
          }
        }
        last = next;
      } catch {
        // The backend is not ready yet; retry on the next tick
      }
    };
    void poll();
    timer = setInterval(() => void poll(), 2000);
    return {
      unlisten: () => {
        stopped = true;
        if (timer) {
          clearInterval(timer);
          timer = null;
        }
      },
    };
  }
  const es = new EventSource(`${API_BASE}/api/distributed/events`);
  es.onmessage = (e) => {
    try {
      onEvent(JSON.parse(e.data) as RegistryEvent);
    } catch {
      /* ignore */
    }
  };
  return { unlisten: () => es.close() };
}
