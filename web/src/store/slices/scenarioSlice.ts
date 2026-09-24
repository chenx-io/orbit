// Automation scenario domain: case / folder / data set / suite CRUD, run orchestration and run reports.
//
// Run semantics:
// - Environment: only explicitly set values count. A case run alone uses its "case environment"; a suite run uses the "suite environment" throughout (overriding case environments);
//   a batch folder run uses each case's own environment; unspecified = no environment variables injected (no fallback to the global activeEnvId).
// - Iteration: loop count and CSV row iteration are driven by the frontend calling run_scenario per round (rows × loops), with YAML iterations fixed at 1.
// - Failure policy: the engine's OnError only has stop / continue; next-loop is implemented by the frontend at loop boundaries.
import type { StateCreator } from "zustand";
import type {
  DataSetMode,
  FailurePolicy,
  RunMode,
  Scenario,
  ScenarioDataSet,
  ScenarioPriority,
  ScenarioFolder,
  ScenarioReportSummary,
  ScenarioRunCaseResult,
  ScenarioRunRecord,
  ScenarioRunStep,
  TestSuite,
  RecordedRequest,
} from "@/data/types";
import { seedScenarios, uid } from "@/data/seed";
import { t, tFormat } from "@/lib/localeDict";
import {
  connectLoadTestStream,
  deleteScenarioReport,
  drainScenarioProgress,
  isTauri,
  listScenarioReports,
  loadScenarioReport,
  runScenario as runScenarioBackend,
  saveScenarioReport,
} from "@/lib/bridge";
import { saveNow } from "@/lib/persistence";
import { buildScenarioYaml } from "../utils/yaml";
import {
  buildCasePlan,
  buildRecord,
  collectFolderCases,
  collectSuiteCases,
  countScenarioAssertions,
  resolveEnv,
  runWithConcurrency,
  sortCases,
  toSummary,
} from "../utils/scenarioRunner";
import type { RunTarget, RunTargetType } from "../utils/scenarioRunner";
import type { AppState } from "../types";

/** Raw step event pushed by the engine */
interface RawStepEvent {
  step?: string;
  status?: string;
  duration_ms?: number;
  /** Request/response details (sent with the event only when "record request details" is enabled) */
  detail?: RecordedRequest;
}

/** Run progress (the only data source for the bottom "This run" panel) */
export interface ScenarioRunProgress {
  targetType: RunTargetType;
  targetId: string;
  targetName: string;
  runMode: RunMode;
  startedAt: number;
  caseIndex: number;
  caseTotal: number;
  currentCaseName: string;
  rowIndex: number;
  rowTotal: number;
  iteration: number;
  iterationTotal: number;
  pass: number;
  fail: number;
  skip: number;
  /** Name of the environment the current case runs in */
  envName: string | null;
  /** Accumulated loop count */
  iterations: number;
  /** Accumulated total request duration (ms) */
  requestMs: number;
  /** Accumulated request count */
  requestCount: number;
  /** Accumulated assertion count */
  assertCount: number;
  /** Live detail steps (empty in parallel mode, where only case-level summaries are kept) */
  steps: ScenarioRunStep[];
  cases: ScenarioRunCaseResult[];
  finished: boolean;
  aborted: boolean;
  /** Ended early because of the stop failure policy (not a user abort) */
  halted: boolean;
}

export interface ScenarioSlice {
  scenarios: Scenario[];
  activeScenarioId: string | null;
  scenarioFolders: ScenarioFolder[];
  scenarioDataSets: ScenarioDataSet[];
  scenarioSuites: TestSuite[];
  activeSuiteId: string | null;
  /** Key of the running target (${type}:${id}); non-empty means busy */
  scenarioRunning: string | null;
  scenarioRun: ScenarioRunProgress | null;
  scenarioReports: ScenarioReportSummary[];
  scenarioReportDetail: ScenarioRunRecord | null;
  dirtyScenarios: Set<string>;
  _scenarioSnapshots: Record<string, Scenario>;

  // ── Cases ──
  addScenario: (
    name: string,
    folderId?: string | null,
    priority?: ScenarioPriority,
  ) => string;
  updateScenario: (id: string, updates: Partial<Scenario>) => void;
  removeScenario: (id: string) => void;
  setActiveScenario: (id: string | null) => void;
  moveScenario: (id: string, folderId: string | null) => void;

  // ── Folders ──
  addFolder: (name: string, parentId: string | null) => string;
  updateFolder: (id: string, patch: Partial<ScenarioFolder>) => void;
  removeFolder: (id: string) => void;
  setFolderCollapsed: (id: string, collapsed: boolean) => void;

  // ── Data sets ──
  addDataSet: (name: string, csv: string, mode?: DataSetMode) => string;
  updateDataSet: (id: string, patch: Partial<ScenarioDataSet>) => void;
  removeDataSet: (id: string) => void;

  // ── Suites ──
  addSuite: (name: string) => string;
  updateSuite: (id: string, patch: Partial<TestSuite>) => void;
  removeSuite: (id: string) => void;
  setActiveSuite: (id: string | null) => void;

  // ── Runs ──
  runTarget: (target: RunTarget) => Promise<void>;
  abortRun: () => void;
  clearRun: () => void;

  // ── Reports ──
  fetchReports: () => Promise<void>;
  openReport: (id: string) => Promise<void>;
  closeReport: () => void;
  removeReport: (id: string) => Promise<void>;

  // ── Dirty snapshots ──
  markScenarioDirty: (id: string) => void;
  saveScenario: (id: string) => void;
  restoreScenario: (id: string) => void;
}

/** User abort flag (module-level, to avoid high-frequency setState) */
let abortRequested = false;

function countSteps(steps: ScenarioRunStep[]): {
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

/** Start the step event pump: collects engine-pushed events into pending (only meaningful for ownership in serial mode) */
function startStepPump(pending: RawStepEvent[]): () => void {
  const onData = (data: string): void => {
    try {
      const p = JSON.parse(data) as RawStepEvent & { type?: string };
      if (p && p.type === "step") pending.push(p);
    } catch {
      /* ignore non-JSON / malformed events */
    }
  };
  if (!isTauri()) {
    const stream = connectLoadTestStream(
      onData,
      () => {},
      () => {},
    );
    return () => stream.unlisten?.();
  }
  let stopped = false;
  const poll = async (): Promise<void> => {
    if (stopped) return;
    try {
      const events = await drainScenarioProgress();
      for (const d of events) onData(d);
    } catch {
      /* ignore */
    }
  };
  const timer = setInterval(() => void poll(), 300);
  return () => {
    stopped = true;
    clearInterval(timer);
  };
}

/** Wait for this round's events to flow back (browser SSE is delayed; Tauri drains once proactively) */
async function settleEvents(pending: RawStepEvent[]): Promise<void> {
  if (isTauri()) {
    try {
      const events = await drainScenarioProgress();
      for (const d of events) {
        try {
          const p = JSON.parse(d) as RawStepEvent & { type?: string };
          if (p && p.type === "step") pending.push(p);
        } catch {
          /* ignore */
        }
      }
    } catch {
      /* ignore */
    }
    return;
  }
  await new Promise((r) => setTimeout(r, 150));
}

/** Small numeric accumulator helper (run metric aggregation) */
function sumBy<T>(list: T[], pick: (x: T) => number): number {
  return list.reduce((n, x) => n + (pick(x) || 0), 0);
}

/** Resolve a run target → case list + run mode + environment override */
function resolveTarget(
  state: AppState,
  target: RunTarget,
): {
  cases: Scenario[];
  targetName: string;
  runMode: RunMode;
  concurrency: number;
  forcedEnvId: string | null | undefined;
} | null {
  if (target.type === "case") {
    const sc = state.scenarios.find((s) => s.id === target.id);
    return sc
      ? {
          cases: [sc],
          targetName: sc.name,
          runMode: "serial",
          concurrency: 1,
          forcedEnvId: undefined,
        }
      : null;
  }
  if (target.type === "folder") {
    const folder = state.scenarioFolders.find((f) => f.id === target.id);
    if (!folder) return null;
    return {
      cases: sortCases(
        collectFolderCases(state.scenarios, state.scenarioFolders, folder.id),
      ),
      targetName: folder.name,
      runMode: "serial",
      concurrency: 1,
      forcedEnvId: undefined,
    };
  }
  const suite = state.scenarioSuites.find((s) => s.id === target.id);
  if (!suite) return null;
  const cases = collectSuiteCases(state.scenarios, suite);
  return {
    cases: suite.runMode === "serial" ? sortCases(cases) : cases,
    targetName: suite.name,
    runMode: suite.runMode,
    concurrency: suite.concurrency ?? 3,
    forcedEnvId: suite.envId ?? null,
  };
}

export const createScenarioSlice: StateCreator<
  AppState,
  [],
  [],
  ScenarioSlice
> = (set, get) => ({
  scenarios: seedScenarios,
  activeScenarioId: seedScenarios[0]?.id ?? null,
  scenarioFolders: [],
  scenarioDataSets: [],
  scenarioSuites: [],
  activeSuiteId: null,
  scenarioRunning: null,
  scenarioRun: null,
  scenarioReports: [],
  scenarioReportDetail: null,
  dirtyScenarios: new Set(),
  _scenarioSnapshots: {},

  // ── Cases ──
  addScenario: (name, folderId = null, priority) => {
    const id = uid("sc");
    set((s) => ({
      scenarios: [...s.scenarios, { id, name, steps: [], folderId, priority }],
    }));
    void saveNow();
    return id;
  },
  updateScenario: (id, updates) => {
    set((s) => {
      const snap = { ...s._scenarioSnapshots };
      if (!snap[id]) {
        const sc = s.scenarios.find((x) => x.id === id);
        if (sc) snap[id] = JSON.parse(JSON.stringify(sc));
      }
      return {
        scenarios: s.scenarios.map((sc) =>
          sc.id === id ? { ...sc, ...updates } : sc,
        ),
        dirtyScenarios: new Set([...s.dirtyScenarios, id]),
        _scenarioSnapshots: snap,
      };
    });
    void saveNow();
  },
  removeScenario: (id) => {
    set((s) => ({
      scenarios: s.scenarios.filter((sc) => sc.id !== id),
      activeScenarioId: s.activeScenarioId === id ? null : s.activeScenarioId,
      // Clean up suite members too (static references, to avoid leftover dangling ids)
      scenarioSuites: s.scenarioSuites.map((su) =>
        su.memberIds.includes(id)
          ? {
              ...su,
              memberIds: su.memberIds.filter((m) => m !== id),
              updatedAt: Date.now(),
            }
          : su,
      ),
    }));
    void saveNow();
  },
  setActiveScenario: (id) => set({ activeScenarioId: id, activeSuiteId: null }),
  moveScenario: (id, folderId) => {
    get().updateScenario(id, { folderId });
  },

  // ── Folders ──
  addFolder: (name, parentId) => {
    const id = uid("sf");
    set((s) => ({
      scenarioFolders: [...s.scenarioFolders, { id, name, parentId }],
    }));
    void saveNow();
    return id;
  },
  updateFolder: (id, patch) => {
    set((s) => ({
      scenarioFolders: s.scenarioFolders.map((f) =>
        f.id === id ? { ...f, ...patch } : f,
      ),
    }));
    void saveNow();
  },
  removeFolder: (id) => {
    set((s) => {
      // Cascade: collect itself + all subfolders; cases under them are promoted to the deleted folder's parent
      const doomed = new Set<string>([id]);
      let changed = true;
      while (changed) {
        changed = false;
        for (const f of s.scenarioFolders) {
          if (f.parentId && doomed.has(f.parentId) && !doomed.has(f.id)) {
            doomed.add(f.id);
            changed = true;
          }
        }
      }
      const target = s.scenarioFolders.find((f) => f.id === id);
      const parentId = target?.parentId ?? null;
      return {
        scenarioFolders: s.scenarioFolders.filter((f) => !doomed.has(f.id)),
        scenarios: s.scenarios.map((sc) =>
          sc.folderId && doomed.has(sc.folderId)
            ? { ...sc, folderId: parentId }
            : sc,
        ),
      };
    });
    void saveNow();
  },
  setFolderCollapsed: (id, collapsed) => {
    set((s) => ({
      scenarioFolders: s.scenarioFolders.map((f) =>
        f.id === id ? { ...f, collapsed } : f,
      ),
    }));
    void saveNow();
  },

  // ── Data sets ──
  addDataSet: (name, csv, mode = "sequential") => {
    const id = uid("ds");
    set((s) => ({
      scenarioDataSets: [
        ...s.scenarioDataSets,
        {
          id,
          name,
          csv,
          columns: [],
          rowCount: 0,
          mode,
          updatedAt: Date.now(),
        },
      ],
    }));
    void saveNow();
    return id;
  },
  updateDataSet: (id, patch) => {
    set((s) => ({
      scenarioDataSets: s.scenarioDataSets.map((d) =>
        d.id === id ? { ...d, ...patch, updatedAt: Date.now() } : d,
      ),
    }));
    void saveNow();
  },
  removeDataSet: (id) => {
    set((s) => ({
      scenarioDataSets: s.scenarioDataSets.filter((d) => d.id !== id),
      // Unbind cases referencing this data set (avoids dangling references that would fail resolution at run time)
      scenarios: s.scenarios.map((sc) =>
        sc.dataSetId === id
          ? { ...sc, dataSetId: null, useDataSet: false }
          : sc,
      ),
    }));
    void saveNow();
  },

  // ── Suites ──
  addSuite: (name) => {
    const id = uid("su");
    set((s) => ({
      scenarioSuites: [
        ...s.scenarioSuites,
        {
          id,
          name,
          envId: null,
          runMode: "serial",
          concurrency: 3,
          memberIds: [],
          updatedAt: Date.now(),
        },
      ],
    }));
    void saveNow();
    return id;
  },
  updateSuite: (id, patch) => {
    set((s) => ({
      scenarioSuites: s.scenarioSuites.map((su) =>
        su.id === id ? { ...su, ...patch, updatedAt: Date.now() } : su,
      ),
    }));
    void saveNow();
  },
  removeSuite: (id) => {
    set((s) => ({
      scenarioSuites: s.scenarioSuites.filter((su) => su.id !== id),
      activeSuiteId: s.activeSuiteId === id ? null : s.activeSuiteId,
    }));
    void saveNow();
  },
  setActiveSuite: (id) => set({ activeSuiteId: id, activeScenarioId: null }),

  // ── Runs ──
  runTarget: async (target) => {
    const state = get();
    if (state.scenarioRunning) return;
    const resolved = resolveTarget(state, target);
    if (!resolved || resolved.cases.length === 0) return;
    const { cases, targetName, runMode, concurrency, forcedEnvId } = resolved;

    const startedAt = Date.now();
    const collectSteps = runMode === "serial"; // parallel mode crosstalks on event ownership → no detail collection
    abortRequested = false;

    set({
      scenarioRunning: `${target.type}:${target.id}`,
      scenarioReportDetail: null,
      scenarioRun: {
        targetType: target.type,
        targetId: target.id,
        targetName,
        runMode,
        startedAt,
        caseIndex: 0,
        caseTotal: cases.length,
        currentCaseName: "",
        rowIndex: 0,
        rowTotal: 0,
        iteration: 0,
        iterationTotal: 0,
        pass: 0,
        fail: 0,
        skip: 0,
        envName: null,
        iterations: 0,
        requestMs: 0,
        requestCount: 0,
        assertCount: 0,
        steps: [],
        cases: [],
        finished: false,
        aborted: false,
        halted: false,
      },
    });
    get().track("scenario_run");

    const pending: RawStepEvent[] = [];
    const stopPump = collectSteps ? startStepPump(pending) : null;
    let seq = 0;
    const allSteps: ScenarioRunStep[] = [];

    /** Persist buffered events as detail steps (returns the newly added part for per-case counting) */
    const flush = (
      caseId: string,
      rowIndex?: number,
      iteration?: number,
    ): ScenarioRunStep[] => {
      if (!collectSteps || pending.length === 0) return [];
      const added: ScenarioRunStep[] = pending.map((e) => ({
        seq: seq++,
        caseId,
        stepId: e.step || "",
        name: e.step || t("scenario.stepLabel"),
        status: e.status === "fail" ? ("fail" as const) : ("pass" as const),
        message: `${e.step ?? ""} · ${e.status ?? ""}${typeof e.duration_ms === "number" ? ` (${Math.round(e.duration_ms)}ms)` : ""}`,
        durationMs:
          typeof e.duration_ms === "number" ? Math.round(e.duration_ms) : 0,
        rowIndex,
        iteration,
        request: e.detail,
      }));
      pending.length = 0;
      allSteps.push(...added);
      set((s) => {
        if (!s.scenarioRun) return {};
        const steps = [...s.scenarioRun.steps, ...added];
        return {
          scenarioRun: { ...s.scenarioRun, steps, ...countSteps(steps) },
        };
      });
      return added;
    };

    const caseResults: ScenarioRunCaseResult[] = [];
    let halted = false;

    const runOneCase = async (sc: Scenario, idx: number): Promise<void> => {
      const s = get();
      const plan = buildCasePlan(sc, {
        environments: s.environments,
        globalVariables: s.globalVariables,
        dataSets: s.scenarioDataSets,
        forcedEnvId,
      });
      set((st) =>
        st.scenarioRun
          ? {
              scenarioRun: {
                ...st.scenarioRun,
                caseIndex: idx,
                currentCaseName: sc.name,
                envName: plan.envName,
                rowTotal: plan.rowOrder.length,
                iterationTotal: plan.iterations,
              },
            }
          : {},
      );

      const caseSteps: ScenarioRunStep[] = [];
      const failedRows: number[] = [];
      const started = Date.now();
      /** Assertion count of one full case run (enabled assertions of referenced requests) */
      const assertsPerRun = countScenarioAssertions(
        sc.steps,
        Object.values(get().requests),
      );
      let caseRequestMs = 0;
      let caseRequestCount = 0;
      let caseAsserts = 0;
      const rowSeq: (number | null)[] =
        plan.rowOrder.length > 0 ? plan.rowOrder : [null];

      for (let ri = 0; ri < rowSeq.length && !abortRequested; ri += 1) {
        const rowIdx = rowSeq[ri];
        const rowVars = rowIdx === null ? {} : (plan.rows[rowIdx] ?? {});
        for (let it = 0; it < plan.iterations && !abortRequested; it += 1) {
          set((st) =>
            st.scenarioRun
              ? {
                  scenarioRun: {
                    ...st.scenarioRun,
                    rowIndex: ri + 1,
                    iteration: it + 1,
                  },
                }
              : {},
          );
          const yaml = buildScenarioYaml(
            sc,
            get().requests,
            { ...plan.envVars, ...rowVars },
            get().collections,
            // Script library table: references in the scenario YAML are expanded into concrete actions (self-contained file)
            {
              iterations: 1,
              onError: plan.onError,
              library: get().actionTemplates,
            },
          );
          const before = caseSteps.length;
          try {
            const result = await runScenarioBackend({
              yaml,
              vus: 1,
              duration: "60s",
              recordDetails: sc.recordRequestDetails === true,
            });
            await settleEvents(pending);
            if (collectSteps) {
              caseSteps.push(...flush(sc.id, rowIdx ?? undefined, it));
            } else if (result.summary) {
              // No detail events in parallel mode: account from the engine summary
              const total = result.summary.total_requests ?? 0;
              const failures = result.summary.total_failures ?? 0;
              const stub: ScenarioRunStep = {
                seq: seq++,
                caseId: sc.id,
                stepId: `${sc.id}:${ri}:${it}`,
                name:
                  plan.rowOrder.length > 0
                    ? tFormat("run.rowRound", ri + 1, it + 1)
                    : tFormat("run.roundLabel", it + 1),
                status: failures > 0 ? "fail" : "pass",
                message: `${Math.max(0, total - failures)} pass / ${failures} fail`,
                durationMs: result.summary.total_duration_ms ?? 0,
                rowIndex: rowIdx ?? undefined,
                iteration: it,
              };
              caseSteps.push(stub);
              allSteps.push(stub);
              set((st) => {
                if (!st.scenarioRun) return {};
                const steps = [...st.scenarioRun.steps, stub];
                return {
                  scenarioRun: {
                    ...st.scenarioRun,
                    steps,
                    ...countSteps(steps),
                  },
                };
              });
            }
            if (result.status === "error") {
              caseSteps.push({
                seq: seq++,
                caseId: sc.id,
                stepId: "",
                name: sc.name,
                status: "fail",
                message: result.error || t("common.unknownError"),
                durationMs: 0,
                rowIndex: rowIdx ?? undefined,
                iteration: it,
              });
              allSteps.push(caseSteps[caseSteps.length - 1]);
            }
          } catch (e) {
            const stub: ScenarioRunStep = {
              seq: seq++,
              caseId: sc.id,
              stepId: "",
              name: sc.name,
              status: "fail",
              message: e instanceof Error ? e.message : String(e),
              durationMs: 0,
              rowIndex: rowIdx ?? undefined,
              iteration: it,
            };
            caseSteps.push(stub);
            allSteps.push(stub);
          }

          const added = caseSteps.slice(before);
          caseRequestMs += added.reduce((n, x) => n + (x.durationMs || 0), 0);
          caseRequestCount += added.length;
          caseAsserts += assertsPerRun;
          const iterFail = added.some((x) => x.status === "fail");
          if (iterFail && rowIdx !== null && !failedRows.includes(rowIdx)) {
            failedRows.push(rowIdx);
          }
          if (iterFail) {
            if (plan.onError === "stop") {
              halted = true;
              break;
            }
            if (plan.onError === "next-loop") break; // end this row/round and move to the next row
          }
        }
        if (halted || abortRequested) break;
      }

      const c = countSteps(caseSteps);
      const caseResult: ScenarioRunCaseResult = {
        scenarioId: sc.id,
        scenarioName: sc.name,
        priority: sc.priority ?? "p2",
        envName: plan.envName,
        dataSetName: plan.dataSetName,
        iterations: plan.iterations,
        rows: plan.rowOrder.length,
        pass: c.pass,
        fail: c.fail,
        skip: c.skip,
        durationMs: Date.now() - started,
        failedRows,
        requestMs: caseRequestMs,
        requestCount: caseRequestCount,
        assertCount: caseAsserts,
        error: plan.warning,
      };
      caseResults.push(caseResult);
      set((st) =>
        st.scenarioRun
          ? {
              scenarioRun: {
                ...st.scenarioRun,
                cases: [...caseResults],
                iterations: sumBy(caseResults, (x) => x.iterations ?? 1),
                requestMs: sumBy(caseResults, (x) => x.requestMs ?? 0),
                requestCount: sumBy(caseResults, (x) => x.requestCount ?? 0),
                assertCount: sumBy(caseResults, (x) => x.assertCount ?? 0),
              },
            }
          : {},
      );
    };

    try {
      if (runMode === "parallel") {
        await runWithConcurrency(cases, concurrency, (sc, i) =>
          runOneCase(sc, i),
        );
      } else {
        for (let i = 0; i < cases.length; i += 1) {
          if (abortRequested) break;
          await runOneCase(cases[i], i);
          if (halted) break; // stop failure policy: terminate the remaining cases
        }
      }
    } finally {
      stopPump?.();
      const wsId = get().activeWorkspaceId;
      const envName =
        target.type === "suite"
          ? resolveEnv(get().environments, forcedEnvId, {}).name
          : null;
      const record = buildRecord({
        workspaceId: wsId ?? undefined,
        targetType: target.type,
        targetId: target.id,
        targetName,
        runMode,
        startedAt,
        durationMs: Date.now() - startedAt,
        envName,
        cases: caseResults,
        steps: allSteps,
        aborted: abortRequested,
      });
      try {
        await saveScenarioReport(record);
      } catch (e) {
        console.error("[runTarget] failed to save the run report", e);
      }
      set((s) => ({
        scenarioRunning: null,
        scenarioReports: [toSummary(record), ...s.scenarioReports],
        scenarioRun: s.scenarioRun
          ? {
              ...s.scenarioRun,
              finished: true,
              aborted: abortRequested,
              halted,
            }
          : null,
      }));
    }
  },

  abortRun: () => {
    abortRequested = true;
  },
  clearRun: () => set({ scenarioRun: null }),

  // ── Reports ──
  fetchReports: async () => {
    try {
      const rows = await listScenarioReports(get().activeWorkspaceId);
      set({ scenarioReports: rows });
    } catch (e) {
      console.error("[scenario] failed to load run reports", e);
    }
  },
  openReport: async (id) => {
    try {
      const detail = await loadScenarioReport(id);
      set({ scenarioReportDetail: detail });
    } catch (e) {
      console.error("[scenario] failed to load report details", e);
    }
  },
  closeReport: () => set({ scenarioReportDetail: null }),
  removeReport: async (id) => {
    try {
      await deleteScenarioReport(id);
      set((s) => ({
        scenarioReports: s.scenarioReports.filter((r) => r.id !== id),
        scenarioReportDetail:
          s.scenarioReportDetail?.id === id ? null : s.scenarioReportDetail,
      }));
    } catch (e) {
      console.error("[scenario] failed to delete the report", e);
    }
  },

  // ── Dirty snapshots ──
  markScenarioDirty: (id) => {
    set((s) => {
      if (!s._scenarioSnapshots[id]) {
        const sc = s.scenarios.find((x) => x.id === id);
        if (sc) s._scenarioSnapshots[id] = JSON.parse(JSON.stringify(sc));
      }
      return { dirtyScenarios: new Set([...s.dirtyScenarios, id]) };
    });
  },
  saveScenario: (id) =>
    set((s) => {
      const ds = new Set(s.dirtyScenarios);
      ds.delete(id);
      const snap = { ...s._scenarioSnapshots };
      delete snap[id];
      return { dirtyScenarios: ds, _scenarioSnapshots: snap };
    }),
  restoreScenario: (id) =>
    set((s) => {
      const snap = s._scenarioSnapshots[id];
      if (!snap) return {};
      const ds = new Set(s.dirtyScenarios);
      ds.delete(id);
      const ss = { ...s._scenarioSnapshots };
      delete ss[id];
      return {
        scenarios: s.scenarios.map((sc) =>
          sc.id === id ? JSON.parse(JSON.stringify(snap)) : sc,
        ),
        dirtyScenarios: ds,
        _scenarioSnapshots: ss,
      };
    }),
});

/** Default failure policy (used for new cases and as the form default) */
export const DEFAULT_FAILURE_POLICY: FailurePolicy = "stop";
