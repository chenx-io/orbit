// Bridge for persistent-connection sessions (interactive debugging of non-HTTP protocols).
// - Tauri: session_open / session_send / session_close commands plus session-event events;
// - Browser: /api/session/* REST plus an SSE event stream.

import { apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

export interface SessionFraming {
  mode: string;
  delimiter?: string;
  fixedLen?: number;
  bigEndian?: boolean;
}

export interface OpenSessionOptions {
  protocol: string;
  url: string;
  service?: string;
  messageFormat?: string;
  streaming?: string;
  framing?: SessionFraming | null;
  messageType?: string;
  closeAfter?: number;
  payload?: string;
  payloadType?: string;
  preScript?: string;
  postScript?: string;
  maxEvents?: number;
  query?: string;
  variables?: string;
  operationName?: string;
  headers?: Record<string, string>;
  envVars?: Record<string, string>;
  /** Plugin protocol connection config (JSON; from the collection connection, passed through to the plugin's execute) */
  connection?: Record<string, unknown> | null;
}

export interface SessionOpenResult {
  session_id: string;
  protocol: string;
  can_send: boolean;
}

export interface ScriptLogEntry {
  level: string;
  message: string;
}

export type SessionEvent =
  | {
      type: "opened";
      session_id: string;
      protocol: string;
      can_send: boolean;
      time: number;
    }
  | {
      type: "sent";
      session_id: string;
      seq: number;
      data: string;
      text?: string | null;
      pre_logs?: ScriptLogEntry[] | null;
      time: number;
    }
  | {
      type: "received";
      session_id: string;
      seq: number;
      data: string;
      text?: string | null;
      decoded?: string | null;
      post_logs?: ScriptLogEntry[] | null;
      sse?: SseFields | null;
      time: number;
    }
  | { type: "error"; session_id: string; message: string; time: number }
  | { type: "closed"; session_id: string; reason: string; time: number };

export interface SessionMessage {
  seq: number;
  direction: "send" | "recv";
  data: string;
  text?: string | null;
  decoded?: string | null;
  pre_logs?: ScriptLogEntry[] | null;
  post_logs?: ScriptLogEntry[] | null;
  sse?: SseFields | null;
  error?: string | null;
  time: number;
}

/** Standard SSE message fields: id / event / data / retry */
export interface SseFields {
  id?: string | null;
  event?: string | null;
  data?: string | null;
  retry?: number | null;
}

function toSnake(o: OpenSessionOptions): Record<string, unknown> {
  return {
    protocol: o.protocol,
    url: o.url,
    service: o.service ?? null,
    message_format: o.messageFormat ?? null,
    streaming: o.streaming ?? null,
    framing: o.framing
      ? {
          mode: o.framing.mode,
          delimiter: o.framing.delimiter ?? null,
          fixed_len: o.framing.fixedLen ?? null,
          big_endian: o.framing.bigEndian ?? null,
        }
      : null,
    message_type: o.messageType ?? null,
    close_after: o.closeAfter ?? null,
    payload: o.payload ?? null,
    payload_type: o.payloadType ?? null,
    pre_script: o.preScript ?? null,
    post_script: o.postScript ?? null,
    max_events: o.maxEvents ?? null,
    query: o.query ?? null,
    variables: o.variables ?? null,
    operation_name: o.operationName ?? null,
    headers: o.headers ?? null,
    env_vars: o.envVars ?? null,
    connection: o.connection ?? null,
  };
}

export async function sessionOpen(
  options: OpenSessionOptions,
): Promise<SessionOpenResult> {
  if (isTauri()) {
    return tauriInvoke<SessionOpenResult>("session_open", {
      request: toSnake(options),
    });
  }
  return apiPost<SessionOpenResult>("/api/session/open", toSnake(options));
}

export async function sessionSend(
  sessionId: string,
  data: string,
  preScript?: string | null,
): Promise<{ ok: boolean; seq: number }> {
  const payload = {
    session_id: sessionId,
    data,
    pre_script: preScript ?? null,
  };
  if (isTauri()) {
    return tauriInvoke<{ ok: boolean; seq: number }>("session_send", {
      request: payload,
    });
  }
  return apiPost<{ ok: boolean; seq: number }>("/api/session/send", payload);
}

export async function sessionClose(
  sessionId: string,
): Promise<{ ok: boolean }> {
  const payload = { session_id: sessionId };
  if (isTauri()) {
    return tauriInvoke<{ ok: boolean }>("session_close", { request: payload });
  }
  return apiPost<{ ok: boolean }>("/api/session/close", payload);
}

export async function sessionMessages(
  sessionId: string,
): Promise<SessionMessage[]> {
  if (isTauri()) {
    // Tauri v2 converts camelCase argument names to snake_case automatically (same as validate_response_against_model)
    return tauriInvoke<SessionMessage[]>("session_messages", { sessionId });
  }
  return apiGet<SessionMessage[]>(
    `/api/session/messages?session_id=${encodeURIComponent(sessionId)}`,
  );
}

// ─── gRPC Server Reflection (service discovery)──────────────────────────

export interface GrpcReflectMethod {
  name: string;
  inputType: string;
  outputType: string;
  clientStreaming: boolean;
  serverStreaming: boolean;
}

export interface GrpcReflectService {
  name: string;
  methods: GrpcReflectMethod[];
}

/** Discover services and methods via gRPC Server Reflection (the browser uses the HTTP API, Tauri uses a command) */
export async function grpcReflect(url: string): Promise<GrpcReflectService[]> {
  if (isTauri()) {
    return tauriInvoke<GrpcReflectService[]>("grpc_reflect", { url });
  }
  const res = await apiPost<{ services: GrpcReflectService[]; error?: string }>(
    "/api/session/grpc/reflect",
    { url },
  );
  if (res.error) throw new Error(res.error);
  return res.services ?? [];
}

export interface SessionStreamHandle {
  unlisten: () => void;
  ready: Promise<void>;
}

/** Subscribe to live session events */
export function connectSessionEvents(
  sessionId: string,
  onEvent: (ev: SessionEvent) => void,
): SessionStreamHandle {
  const API_BASE: string =
    (import.meta.env.VITE_API_BASE as string | undefined) ||
    "http://localhost:8788";
  let unlisten: (() => void) | null = null;
  let resolveReady!: () => void;
  const ready = new Promise<void>((res) => {
    resolveReady = res;
  });

  if (isTauri()) {
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const un = await listen("session-event", (e) => {
        const ev = e.payload as SessionEvent;
        if (ev && ev.session_id === sessionId) onEvent(ev);
      });
      unlisten = un;
      resolveReady();
    })();
  } else {
    const es = new EventSource(
      `${API_BASE}/api/session/events?session_id=${encodeURIComponent(sessionId)}`,
    );
    es.onmessage = (e) => {
      try {
        const ev = JSON.parse(e.data) as SessionEvent;
        if (ev && ev.session_id === sessionId) onEvent(ev);
      } catch {
        /* ignore malformed */
      }
    };
    unlisten = () => es.close();
    resolveReady();
  }

  return {
    unlisten: () => unlisten?.(),
    ready,
  };
}
