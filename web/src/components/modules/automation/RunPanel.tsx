// Current run panel: progress + run metrics (shared with the historical report detail) + all/pass/fail detail filter.
import { useEffect, useMemo, useState } from "react";
import { CheckCircle2, Loader2, Square, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import {
  ResultFilterBar,
  RunMetricsBar,
  StepList,
  type RunMetrics,
} from "./RunResultView";
import { filterSteps, type ResultFilterValue } from "./stepFilter";

export function RunPanel() {
  const { t, format } = useT();
  const run = useAppStore((s) => s.scenarioRun);
  const running = useAppStore((s) => s.scenarioRunning);
  const abortRun = useAppStore((s) => s.abortRun);
  const [filter, setFilter] = useState<ResultFilterValue>("all");
  const [now, setNow] = useState(Date.now());

  // Refresh elapsed time every 500ms while running
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(timer);
  }, [running]);

  const elapsed = useMemo(() => {
    if (!run) return 0;
    return running
      ? (now - run.startedAt) / 1000
      : (Date.now() - run.startedAt) / 1000;
  }, [run, running, now]);

  if (!run) {
    return (
      <div className="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground">
        {t("run.idle")}
      </div>
    );
  }

  const totalRounds = Math.max(
    1,
    run.rowTotal * Math.max(1, run.iterationTotal),
  );
  const doneRounds = Math.min(
    totalRounds,
    Math.max(0, run.caseIndex) * totalRounds +
      Math.max(0, run.rowIndex - 1) * Math.max(1, run.iterationTotal) +
      Math.max(0, run.iteration - 1),
  );
  const progress = run.finished
    ? 100
    : Math.min(99, (doneRounds / totalRounds) * 100);
  const steps = filterSteps(run.steps, filter);

  const metrics: RunMetrics = {
    envName: run.envName ?? run.cases[0]?.envName ?? null,
    iterations: run.iterations,
    pass: run.pass,
    fail: run.fail,
    durationMs: elapsed * 1000,
    requestMs: run.requestMs,
    requestCount: run.requestCount,
    assertCount: run.assertCount,
  };

  return (
    <div className="flex h-full flex-col">
      {/* Progress header */}
      <div className="shrink-0 space-y-1.5 border-b border-border px-3 py-2">
        <div className="flex items-center gap-2 text-xs">
          {running && (
            <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
          )}
          <span className="font-medium">{run.targetName}</span>
          {run.runMode === "parallel" && (
            <span className="rounded-full border border-border px-1.5 py-px text-xs text-muted-foreground">
              {t("report.mode.parallel")}
            </span>
          )}
          {running ? (
            <>
              <span className="text-muted-foreground">
                {format(
                  "run.progressCase",
                  Math.min(run.caseIndex + 1, run.caseTotal),
                  run.caseTotal,
                )}
              </span>
              {run.rowTotal > 1 && (
                <span className="text-muted-foreground">
                  {format(
                    "run.progressRow",
                    Math.max(1, run.rowIndex),
                    run.rowTotal,
                  )}
                </span>
              )}
              {run.iterationTotal > 1 && (
                <span className="text-muted-foreground">
                  {format(
                    "run.progressIter",
                    Math.max(1, run.iteration),
                    run.iterationTotal,
                  )}
                </span>
              )}
            </>
          ) : (
            <span className="text-muted-foreground">
              {run.aborted
                ? t("run.stopped")
                : run.halted
                  ? t("run.halted")
                  : t("run.done")}
            </span>
          )}
          <span className="ml-auto flex items-center gap-1.5">
            <span className="flex items-center gap-0.5 text-emerald-500">
              <CheckCircle2 className="h-3.5 w-3.5" /> {run.pass}
            </span>
            <span className="flex items-center gap-0.5 text-rose-500">
              <XCircle className="h-3.5 w-3.5" /> {run.fail}
            </span>
            <span className="text-muted-foreground">
              {format("run.elapsed", elapsed.toFixed(1))}
            </span>
            {running && (
              <Button
                size="sm"
                variant="outline"
                className="h-6 text-xs"
                onClick={abortRun}
              >
                <Square className="h-3 w-3" /> {t("scenario.stop")}
              </Button>
            )}
          </span>
        </div>
        <Progress value={progress} className="h-1.5" />
        {run.runMode === "parallel" && (
          <div className="text-xs text-muted-foreground">
            {t("run.parallelHint")}
          </div>
        )}
      </div>

      {/* Run metrics: identical to the historical report detail */}
      <RunMetricsBar m={metrics} />

      {/* Detail filter: all / pass / fail */}
      <div className="flex shrink-0 items-center gap-1 px-3 py-1">
        <ResultFilterBar value={filter} onChange={setFilter} />
        <span className="ml-auto text-xs text-muted-foreground">
          {steps.length}
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-2">
        <StepList steps={steps} />
      </div>
    </div>
  );
}
