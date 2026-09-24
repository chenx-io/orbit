// Performance reports & baseline management.
import type { SavedReport } from "@/data/types";
import { t } from "@/lib/localeDict";
import { apiDelete, apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

export async function saveReport(report: {
  name: string;
  endpoint: string;
  method: string;
  vus: number;
  duration: string;
  config?: string | null;
  summary: unknown;
  thresholds: unknown;
  all_thresholds_passed: boolean;
  workspaceId?: string;
}): Promise<{ status: string; id: string } | { error: string }> {
  if (isTauri()) {
    return tauriInvoke("save_report", {
      req: report as unknown as Record<string, unknown>,
    });
  }
  return apiPost("/api/report/save", report);
}

export async function listReports(
  workspaceId?: string,
): Promise<SavedReport[]> {
  if (isTauri()) {
    return tauriInvoke<SavedReport[]>("list_reports", {
      workspaceId: workspaceId ?? null,
    });
  }
  return apiGet<SavedReport[]>(
    `/api/report/list?workspaceId=${encodeURIComponent(workspaceId ?? "ws-default")}`,
  );
}

export async function loadReport(id: string): Promise<SavedReport> {
  if (isTauri()) {
    return tauriInvoke<SavedReport>("load_report", { id });
  }
  return apiGet<SavedReport>(`/api/report/${id}`);
}

export async function deleteReport(id: string): Promise<{ status: string }> {
  if (isTauri()) {
    return tauriInvoke<{ status: string }>("delete_report", { id });
  }
  return apiDelete<{ status: string }>(`/api/report/${id}`);
}

/** Export a saved performance report (desktop): format supports html/json/csv/junit (saved reports hold no raw samples) */
export async function exportSavedReport(
  id: string,
  format: string,
): Promise<{ content: string; filename: string; truncated: boolean }> {
  if (!isTauri()) {
    throw new Error(t("perf.exportDesktopOnly"));
  }
  return tauriInvoke("export_saved_report", { id, format });
}

export async function setBaseline(
  id: string,
  baselineName: string,
): Promise<{ status: string }> {
  if (isTauri()) {
    return tauriInvoke<{ status: string }>("set_baseline", {
      id,
      req: { baseline_name: baselineName },
    });
  }
  return apiPost(`/api/baseline/set/${id}`, { baseline_name: baselineName });
}

export async function unsetBaseline(id: string): Promise<{ status: string }> {
  if (isTauri()) {
    return tauriInvoke<{ status: string }>("unset_baseline", { id });
  }
  return apiPost(`/api/baseline/unset/${id}`, {});
}

export async function listBaselines(
  workspaceId?: string,
): Promise<SavedReport[]> {
  if (isTauri()) {
    return tauriInvoke<SavedReport[]>("list_baselines", {
      workspaceId: workspaceId ?? null,
    });
  }
  return apiGet<SavedReport[]>(
    `/api/baseline/list?workspaceId=${encodeURIComponent(workspaceId ?? "ws-default")}`,
  );
}

export async function renameReport(
  id: string,
  name: string,
): Promise<{ status: string }> {
  if (isTauri()) {
    return tauriInvoke<{ status: string }>("rename_report", { id, name });
  }
  return apiPost(`/api/report/rename/${id}`, { name });
}
