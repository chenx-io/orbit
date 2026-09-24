// Automation scenario run reports: Tauri → app_data_dir/scenario_reports; browser → localStorage (keeping the latest 30).
import type { ScenarioReportSummary, ScenarioRunRecord } from "@/data/types";
import { isTauri, tauriInvoke } from "./client";

const LS_KEY = "orbit.scenarioReports";
/** Browser channel capacity cap (localStorage is limited to 5MB and reports keep growing) */
const MAX_WEB_REPORTS = 30;

function readWebReports(): ScenarioRunRecord[] {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as ScenarioRunRecord[]) : [];
  } catch (e) {
    console.error("[scenarioReports] failed to read local reports", e);
    return [];
  }
}

function writeWebReports(list: ScenarioRunRecord[]): void {
  try {
    localStorage.setItem(
      LS_KEY,
      JSON.stringify(list.slice(0, MAX_WEB_REPORTS)),
    );
  } catch (e) {
    console.error("[scenarioReports] failed to write local reports", e);
  }
}

export function toReportSummary(r: ScenarioRunRecord): ScenarioReportSummary {
  return {
    id: r.id,
    workspaceId: r.workspaceId,
    targetType: r.targetType,
    targetId: r.targetId,
    targetName: r.targetName,
    runMode: r.runMode,
    startedAt: r.startedAt,
    durationMs: r.durationMs,
    envName: r.envName,
    status: r.status,
    totalPass: r.totalPass,
    totalFail: r.totalFail,
    totalSkip: r.totalSkip,
    caseCount: r.cases.length,
  };
}

/** Save a run report */
export async function saveScenarioReport(
  record: ScenarioRunRecord,
): Promise<void> {
  if (isTauri()) {
    await tauriInvoke("save_scenario_report", {
      req: {
        report: record as unknown as Record<string, unknown>,
        workspace_id: record.workspaceId ?? null,
      },
    });
    return;
  }
  const list = readWebReports().filter((r) => r.id !== record.id);
  list.unshift(record);
  writeWebReports(list);
}

/** List report summaries (newest startedAt first) */
export async function listScenarioReports(
  workspaceId?: string | null,
): Promise<ScenarioReportSummary[]> {
  if (isTauri()) {
    const rows = await tauriInvoke<ScenarioReportSummary[]>(
      "list_scenario_reports",
      { workspaceId: workspaceId ?? null },
    );
    return rows ?? [];
  }
  return readWebReports()
    .filter((r) => !workspaceId || (r.workspaceId ?? null) === workspaceId)
    .map(toReportSummary)
    .sort((a, b) => b.startedAt - a.startedAt);
}

/** Load report details */
export async function loadScenarioReport(
  id: string,
): Promise<ScenarioRunRecord | null> {
  if (isTauri()) {
    return tauriInvoke<ScenarioRunRecord>("load_scenario_report", { id });
  }
  return readWebReports().find((r) => r.id === id) ?? null;
}

/** Delete a report */
export async function deleteScenarioReport(id: string): Promise<void> {
  if (isTauri()) {
    await tauriInvoke("delete_scenario_report", { id });
    return;
  }
  writeWebReports(readWebReports().filter((r) => r.id !== id));
}

/** Clear all reports */
export async function clearScenarioReports(): Promise<void> {
  if (isTauri()) {
    await tauriInvoke("clear_scenario_reports");
    return;
  }
  writeWebReports([]);
}
