// Local snapshot persistence (full-database JSON, same structure as the browser side).
// The Web channel always goes through the server's Rust data layer (/api/data/*); localStorage is only a fallback and migration source.
import { apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/** localStorage key; the synchronous save on exit in persistence.ts references it directly (single point of definition) */
export const SNAPSHOT_LS_KEY = "orbit.snapshot";

/** Save the whole snapshot JSON:
 * - Dual write: synchronously write localStorage first (localStorage persistence in the Tauri webview, a safety net when the window closes);
 * - then Tauri asynchronously writes app_data_dir/orbit_data.json (the authoritative copy for export/backup/migration). */
export async function saveSnapshotJson(json: string): Promise<void> {
  localStorage.setItem(SNAPSHOT_LS_KEY, json);
  if (isTauri()) {
    await tauriInvoke<void>("save_snapshot", { json });
  }
}

/** Read the snapshot JSON: prefer a synchronous localStorage read (freshest and IPC-free), falling back to the Tauri file */
export async function loadSnapshotJson(): Promise<string | null> {
  const ls = localStorage.getItem(SNAPSHOT_LS_KEY);
  if (ls) return ls;
  if (isTauri()) {
    return tauriInvoke<string | null>("load_snapshot");
  }
  return null;
}

/** Delete the local snapshot (localStorage + Tauri file); used by "clear data / restore seed" */
export async function clearSnapshotJson(): Promise<void> {
  localStorage.removeItem(SNAPSHOT_LS_KEY);
  if (isTauri()) {
    await tauriInvoke<void>("clear_snapshot");
  }
}

// ─── Data channel (Tauri uses the backend DataService as the authoritative store; the Web uses localStorage) ───

/** Fetch the current snapshot JSON; Tauri returns null on first launch (nothing stored), and the frontend seeds and pushes the first one.
 *  Web: prefer the server data layer (authoritative Rust store), falling back to localStorage when the server is unreachable or empty. */
export async function dataLoadSnapshot(): Promise<string | null> {
  if (isTauri()) {
    return tauriInvoke<string | null>("data_load_snapshot");
  }
  try {
    const res = await apiGet<{ json: string | null }>("/api/data/load");
    if (res.json) return res.json;
  } catch (e) {
    console.warn(
      "[persist] failed to read server data; falling back to localStorage",
      e,
    );
  }
  return localStorage.getItem(SNAPSHOT_LS_KEY);
}

/** Push snapshot JSON (debounced save): the Tauri backend persists it (authoritative) plus a dual-write localStorage safety net;
 *  the Web writes to the server (Rust data layer) plus the same dual-write localStorage safety net.
 * @param baseSavedAt optimistic-lock baseline: the latest save time seen by the pusher; null = force overwrite (first launch / clearing data). */
export async function dataSaveSnapshot(
  json: string,
  baseSavedAt?: number | null,
): Promise<void> {
  localStorage.setItem(SNAPSHOT_LS_KEY, json);
  if (isTauri()) {
    await tauriInvoke<void>("data_save_snapshot", {
      request: { json, baseSavedAt: baseSavedAt ?? null },
    });
    return;
  }
  try {
    await apiPost<{ ok: boolean }>("/api/data/save", {
      json,
      baseSavedAt: baseSavedAt ?? null,
    });
  } catch (e) {
    // The server is unreachable / conflicted: the data already sits in localStorage (the safety net) and can be migrated on the next launch
    console.warn("[persist] server save failed; kept in localStorage", e);
    throw e;
  }
}

/** Clear data ("clear / restore seed"): Tauri deletes the backend storage file; the Web clears localStorage */
export async function dataClear(): Promise<void> {
  localStorage.removeItem(SNAPSHOT_LS_KEY);
  if (isTauri()) {
    await tauriInvoke<void>("data_clear");
  }
}

// ─── History (module-level commands, persisted immediately after sending) ───

/** Append a history entry (Tauri; the Web has no matching backend and relies on snapshot sync) */
export async function dataAddHistory(
  entry: import("@/data/types").HistoryEntry,
): Promise<void> {
  if (!isTauri()) return;
  await tauriInvoke<void>("data_add_history", { entry });
}

/** Clear history (Tauri) */
export async function dataClearHistory(): Promise<void> {
  if (!isTauri()) return;
  await tauriInvoke<void>("data_clear_history");
}

/** Tauri: export the snapshot to a user-specified path (paired with dialog save for picking the location) */
export async function exportSnapshotToFile(
  path: string,
  json: string,
): Promise<void> {
  if (!isTauri()) return;
  await tauriInvoke<void>("export_snapshot", { path, json });
}

/** Browser: trigger a file download via Blob + <a download>. */
export function downloadTextFile(content: string, filename: string): void {
  const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

/** Write arbitrary text to a user-chosen path (the Tauri desktop reuses the export_snapshot command;
 *  the browser degrades to a browser download). Used by the "export request" feature. */
export async function writeExportFile(
  path: string,
  content: string,
): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("export_snapshot", { path, json: content });
    return;
  }
  downloadTextFile(content, path.split(/[\\/]/).pop() || "export.txt");
}
