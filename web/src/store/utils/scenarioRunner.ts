// Automation run orchestration (pure functions): scenario collection, environment resolution, data row ordering, round planning, result aggregation.
// Independent of the store and of IPC, so it can be reasoned about and reused on its own.
import type {
  DataSetMode,
  Environment,
  FailurePolicy,
  Scenario,
  ScenarioDataSet,
  ScenarioFolder,
  ScenarioReportSummary,
  ScenarioRunCaseResult,
  ScenarioRunRecord,
  ScenarioRunStep,
  ScenarioStep,
  TestSuite,
} from "@/data/types";
import { csvRowToVars, orderRowIndexes, parseCsvText } from "@/lib/csv";
import { tFormat } from "@/lib/localeDict";

export type RunTargetType = "case" | "folder" | "suite";

export interface RunTarget {
  type: RunTargetType;
  id: string;
}

const PRIORITY_RANK = { p0: 0, p1: 1, p2: 2, p3: 3 } as const;

/** Collect every scenario under a folder (including all subfolders) */
export function collectFolderCases(
  scenarios: Scenario[],
  folders: ScenarioFolder[],
  folderId: string,
): Scenario[] {
  const folderIds = new Set<string>([folderId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const f of folders) {
      if (f.parentId && folderIds.has(f.parentId) && !folderIds.has(f.id)) {
        folderIds.add(f.id);
        changed = true;
      }
    }
  }
  return scenarios.filter((s) => !!s.folderId && folderIds.has(s.folderId));
}

/** Suite members (in memberIds order, ignoring deleted scenarios) */
export function collectSuiteCases(
  scenarios: Scenario[],
  suite: TestSuite,
): Scenario[] {
  const map = new Map(scenarios.map((s) => [s.id, s]));
  return suite.memberIds
    .map((id) => map.get(id))
    .filter((s): s is Scenario => !!s);
}

/** Order: P0→P3, then by name within the same priority */
export function sortCases(cases: Scenario[]): Scenario[] {
  return [...cases].sort((a, b) => {
    const pa = PRIORITY_RANK[a.priority ?? "p2"];
    const pb = PRIORITY_RANK[b.priority ?? "p2"];
    if (pa !== pb) return pa - pb;
    return a.name.localeCompare(b.name, "zh-CN");
  });
}

/** Environment resolution: only explicitly set values count; unspecified / deleted environment means no environment variables are injected (globals only) */
export function resolveEnv(
  environments: Environment[],
  envId: string | null | undefined,
  globalVariables: Record<string, string>,
): { name: string | null; vars: Record<string, string> } {
  const base = { ...(globalVariables ?? {}) };
  if (!envId) return { name: null, vars: base };
  const env = environments.find((e) => e.id === envId);
  if (!env) return { name: null, vars: base };
  return { name: env.name, vars: { ...base, ...(env.variables ?? {}) } };
}

/** Execution plan of a single scenario: environment / data rows / rounds / failure policy */
export interface CasePlan {
  scenario: Scenario;
  envName: string | null;
  envVars: Record<string, string>;
  dataSetName: string | null;
  /** Data rows (variable dictionaries); empty = not data-driven */
  rows: Record<string, string>[];
  /** Row execution order (indices into `rows`) */
  rowOrder: number[];
  iterations: number;
  onError: FailurePolicy;
  /** Warnings such as a data set parse failure (recorded without blocking the run) */
  warning?: string;
}

export interface PlanContext {
  environments: Environment[];
  globalVariables: Record<string, string>;
  dataSets: ScenarioDataSet[];
  /** Forced environment (a suite run overrides the scenario environment with the suite environment) */
  forcedEnvId?: string | null;
}

export function buildCasePlan(sc: Scenario, ctx: PlanContext): CasePlan {
  const envId =
    ctx.forcedEnvId !== undefined ? ctx.forcedEnvId : (sc.envId ?? null);
  const env = resolveEnv(ctx.environments, envId, ctx.globalVariables);

  const useData = sc.useDataSet === true && !!sc.dataSetId;
  const ds = useData
    ? ctx.dataSets.find((d) => d.id === sc.dataSetId)
    : undefined;

  let rows: Record<string, string>[] = [];
  let rowOrder: number[] = [];
  let warning: string | undefined;
  if (ds) {
    try {
      const parsed = parseCsvText(ds.csv);
      rows = parsed.rows.map((r) => csvRowToVars(parsed.columns, r));
      rowOrder = orderRowIndexes(rows.length, ds.mode ?? "sequential");
    } catch (e) {
      warning = tFormat(
        "scenario.dataset.parseFailed",
        ds.name,
        e instanceof Error ? e.message : String(e),
      );
    }
  }

  return {
    scenario: sc,
    envName: env.name,
    envVars: env.vars,
    dataSetName: ds?.name ?? null,
    rows,
    rowOrder,
    iterations: Math.max(1, Math.floor(sc.iterations ?? 1)),
    onError: sc.onError ?? "stop",
    warning,
  };
}

/** Total executions of one "rows × rounds" pass (used as the progress denominator) */
export function planRuns(plan: CasePlan): number {
  const rows = plan.rowOrder.length > 0 ? plan.rowOrder.length : 1;
  return rows * plan.iterations;
}

export function summarizeSteps(steps: ScenarioRunStep[]): {
  pass: number;
  fail: number;
  skip: number;
} {
  let pass = 0;
  let fail = 0;
  let skip = 0;
  for (const s of steps) {
    if (s.status === "pass") pass += 1;
    else if (s.status === "fail") fail += 1;
    else skip += 1;
  }
  return { pass, fail, skip };
}

export function buildRecord(p: {
  workspaceId?: string;
  targetType: RunTargetType;
  targetId: string;
  targetName: string;
  runMode: "serial" | "parallel";
  startedAt: number;
  durationMs: number;
  envName: string | null;
  cases: ScenarioRunCaseResult[];
  steps: ScenarioRunStep[];
  aborted?: boolean;
}): ScenarioRunRecord {
  // When scenario results exist they are authoritative (in parallel mode steps are accounted at "round" granularity and must not be treated as request counts)
  const pass =
    p.cases.length > 0
      ? p.cases.reduce((n, c) => n + c.pass, 0)
      : summarizeSteps(p.steps).pass;
  const fail =
    p.cases.length > 0
      ? p.cases.reduce((n, c) => n + c.fail, 0)
      : summarizeSteps(p.steps).fail;
  const skip =
    p.cases.length > 0
      ? p.cases.reduce((n, c) => n + c.skip, 0)
      : summarizeSteps(p.steps).skip;
  const iterations = p.cases.reduce((n, c) => n + (c.iterations ?? 1), 0);
  const totalRequestMs = p.cases.reduce((n, c) => n + (c.requestMs ?? 0), 0);
  const totalRequestCount = p.cases.reduce(
    (n, c) => n + (c.requestCount ?? 0),
    0,
  );
  const totalAssertions = p.cases.reduce((n, c) => n + (c.assertCount ?? 0), 0);
  const status = fail > 0 || p.cases.some((c) => !!c.error) ? "fail" : "pass";
  return {
    id: `scr-${p.startedAt}-${Math.random().toString(36).slice(2, 8)}`,
    workspaceId: p.workspaceId,
    targetType: p.targetType,
    targetId: p.targetId,
    targetName: p.targetName,
    runMode: p.runMode,
    startedAt: p.startedAt,
    durationMs: p.durationMs,
    envName: p.envName,
    status,
    totalPass: pass,
    totalFail: fail,
    totalSkip: skip,
    cases: p.cases,
    steps: p.steps,
    aborted: p.aborted ?? false,
    iterations,
    totalRequestMs,
    totalRequestCount,
    totalAssertions,
  };
}

/** Count the assertions executed by one full scenario run (enabled assertions of referenced requests; loops multiply by count) */
export function countScenarioAssertions(
  steps: ScenarioStep[],
  requests: { id: string; assertions?: { meta?: { enabled?: boolean } }[] }[],
): number {
  const byId = new Map(requests.map((r) => [r.id, r]));
  const walk = (list?: ScenarioStep[]): number => {
    if (!list || list.length === 0) return 0;
    return list.reduce((n, s) => {
      if (s.disabled) return n;
      const req = s.requestId ? byId.get(s.requestId) : undefined;
      let c = (req?.assertions ?? []).filter(
        (a) => a.meta?.enabled !== false,
      ).length;
      c += walk(s.children);
      c += walk(s.elseChildren);
      if (s.type === "loop") c *= Math.max(1, s.count ?? 1);
      return n + c;
    }, 0);
  };
  return walk(steps);
}

/** Report detail → list summary (the list renders summary fields only) */
export function toSummary(r: ScenarioRunRecord): ScenarioReportSummary {
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

/** Run concurrently (bounded by the concurrency limit), returning results in the same order as the input */
export async function runWithConcurrency<T, R>(
  items: T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const concurrency = Math.max(1, Math.min(10, Math.floor(limit || 1)));
  const results = new Array<R>(items.length);
  let cursor = 0;
  const runners = Array.from(
    { length: Math.min(concurrency, items.length) },
    async () => {
      for (;;) {
        const i = cursor;
        cursor += 1;
        if (i >= items.length) return;
        results[i] = await worker(items[i], i);
      }
    },
  );
  await Promise.all(runners);
  return results;
}

/** Data row read mode labels (i18n key suffixes) */
export const DATA_SET_MODES: DataSetMode[] = [
  "sequential",
  "random",
  "shuffle",
];
export const FAILURE_POLICIES: FailurePolicy[] = [
  "stop",
  "continue",
  "next-loop",
];
