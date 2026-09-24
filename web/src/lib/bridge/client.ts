// Tauri bridge layer: low-level call primitives.
// On the Tauri desktop it calls the real Rust backend; in browser preview mode it calls a standalone orbit-server
// HTTP service (default http://localhost:8788). Every operation really executes; there is no mock
// or fake data left: if the backend is unavailable a clear error is thrown instead of quietly returning a fabricated response.

/* eslint-disable @typescript-eslint/no-explicit-any */

import { tFormat } from "@/lib/localeDict";

const API_BASE: string =
  (import.meta.env.VITE_API_BASE as string | undefined) ||
  "http://localhost:8788";

export function isTauri(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(
    (window as any).__TAURI__ || (window as any).__TAURI_INTERNALS__,
  );
}

/** Tauri desktop: invoke a Rust command. */
export async function tauriInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Browser preview mode: POST JSON to orbit-server. */
export async function apiPost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  if (!res.ok) {
    let msg = tFormat("bridge.requestFailed", res.status, res.statusText);
    try {
      const d: any = await res.json();
      if (
        d &&
        typeof d === "object" &&
        "error" in d &&
        typeof d.error === "string"
      ) {
        msg = d.error;
      }
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }
  const data: any = await res.json();
  // The backend always returns { error: "non-empty string" } on failure; successful responses may carry an error: null field
  if (
    data &&
    typeof data === "object" &&
    typeof data.error === "string" &&
    data.error.length > 0
  ) {
    throw new Error(data.error);
  }
  return data as T;
}

/** Browser preview mode: GET to orbit-server. */
export async function apiGet<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, { method: "GET" });
  if (!res.ok) {
    let msg = tFormat("bridge.requestFailed", res.status, res.statusText);
    try {
      const d: any = await res.json();
      if (
        d &&
        typeof d === "object" &&
        "error" in d &&
        typeof d.error === "string"
      ) {
        msg = d.error;
      }
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }
  const data: any = await res.json();
  if (
    data &&
    typeof data === "object" &&
    typeof data.error === "string" &&
    data.error.length > 0
  ) {
    throw new Error(data.error);
  }
  return data as T;
}

/** Browser preview mode: DELETE to orbit-server. */
export async function apiDelete<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, { method: "DELETE" });
  if (!res.ok) {
    let msg = tFormat("bridge.requestFailed", res.status, res.statusText);
    try {
      const d: any = await res.json();
      if (
        d &&
        typeof d === "object" &&
        "error" in d &&
        typeof d.error === "string"
      ) {
        msg = d.error;
      }
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }
  const data: any = await res.json();
  if (
    data &&
    typeof data === "object" &&
    typeof data.error === "string" &&
    data.error.length > 0
  ) {
    throw new Error(data.error);
  }
  return data as T;
}
