// Scenario edit view: run-config bar + step orchestrator (drag / nest / branch) + bottom "current run / history report".
// The step orchestration logic was ported from the old AutomationModule; behavior is unchanged.
import { useCallback, useEffect, useRef, useState } from "react";
import { Play, Plus, Save, Settings2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import type { ScenarioStep, StepType } from "@/data/types";
import { StepNode } from "./StepNode";
import { getStepMeta, isDescendantOf, newStep } from "./stepMeta";
import { CaseConfigPanel } from "./CaseConfigPanel";
import { DataSetDialog } from "./DataSetDialog";
import { RunPanel } from "./RunPanel";
import { ReportPanel } from "./ReportPanel";

type DropTarget = { id: string; pos: "before" | "after" | "inside" } | null;

export function ScenarioEditor({ scenarioId }: { scenarioId: string }) {
  const { t } = useT();
  const scenario = useAppStore((s) =>
    s.scenarios.find((x) => x.id === scenarioId),
  );
  const dirty = useAppStore((s) => s.dirtyScenarios.has(scenarioId));
  const updateScenario = useAppStore((s) => s.updateScenario);
  const saveScenario = useAppStore((s) => s.saveScenario);
  const restoreScenario = useAppStore((s) => s.restoreScenario);
  const runTarget = useAppStore((s) => s.runTarget);
  const running = useAppStore((s) => s.scenarioRunning);

  const [dataSetOpen, setDataSetOpen] = useState(false);
  const [configOpen, setConfigOpen] = useState(false);
  const [tab, setTab] = useState("run");
  const isRunning = running === `case:${scenarioId}`;

  // When a run starts, auto-switch to the "current run" tab to show run status and results
  useEffect(() => {
    if (isRunning) setTab("run");
  }, [isRunning]);

  // ── Step orchestration (ported from the old implementation) ──
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const patchStep = useCallback(
    (id: string, p: Partial<ScenarioStep>) => {
      if (!scenario) return;
      const walk = (steps: ScenarioStep[]): ScenarioStep[] =>
        steps.map((st) => {
          if (st.id === id) return { ...st, ...p };
          if (st.children) return { ...st, children: walk(st.children) };
          return st;
        });
      updateScenario(scenario.id, { steps: walk(scenario.steps) });
    },
    [scenario, updateScenario],
  );

  const removeStep = useCallback(
    (id: string) => {
      if (!scenario) return;
      const walk = (steps: ScenarioStep[]): ScenarioStep[] =>
        steps
          .filter((st) => st.id !== id)
          .map((st) =>
            st.children ? { ...st, children: walk(st.children) } : st,
          );
      updateScenario(scenario.id, { steps: walk(scenario.steps) });
    },
    [scenario, updateScenario],
  );

  const addRootStep = useCallback(
    (type: StepType) => {
      if (!scenario) return;
      updateScenario(scenario.id, {
        steps: [...scenario.steps, newStep(type, t)],
      });
    },
    [scenario, updateScenario, t],
  );

  const walkInsert = useCallback(
    (targetId: string, type: StepType, mode: "after" | "before" | "child") => {
      if (!scenario) return;
      const nu = newStep(type, t);
      const walk = (steps: ScenarioStep[]): ScenarioStep[] => {
        const idx = steps.findIndex((s) => s.id === targetId);
        if (idx !== -1) {
          const copy = [...steps];
          if (mode === "child") {
            const tgt = copy[idx];
            copy[idx] = { ...tgt, children: [...(tgt.children ?? []), nu] };
          } else {
            copy.splice(mode === "after" ? idx + 1 : idx, 0, nu);
          }
          return copy;
        }
        return steps.map((s) => {
          let ns = s;
          if (s.children) ns = { ...ns, children: walk(s.children) };
          if (s.type === "condition" && s.elseChildren)
            ns = { ...ns, elseChildren: walk(s.elseChildren) };
          return ns;
        });
      };
      updateScenario(scenario.id, { steps: walk(scenario.steps) });
    },
    [scenario, updateScenario, t],
  );

  const insertAfter = useCallback(
    (targetId: string, type: StepType) => walkInsert(targetId, type, "after"),
    [walkInsert],
  );
  const insertBefore = useCallback(
    (targetId: string, type: StepType) => walkInsert(targetId, type, "before"),
    [walkInsert],
  );
  const insertChild = useCallback(
    (targetId: string, type: StepType) => walkInsert(targetId, type, "child"),
    [walkInsert],
  );

  const moveStep = useCallback(
    (srcId: string, targetId: string, pos: "before" | "after" | "inside") => {
      if (!scenario || srcId === targetId) return;
      if (isDescendantOf(scenario.steps, srcId, targetId)) return;
      let dragged: ScenarioStep | null = null;
      const removeRec = (steps: ScenarioStep[]): ScenarioStep[] =>
        steps.filter((s) => {
          if (s.id === srcId) {
            dragged = s;
            return false;
          }
          return true;
        });
      const insertRec = (steps: ScenarioStep[]): ScenarioStep[] => {
        const idx = steps.findIndex((s) => s.id === targetId);
        if (idx !== -1) {
          const copy = [...steps];
          if (pos === "inside") {
            const tgt = copy[idx];
            const canNest =
              tgt.type === "loop" ||
              tgt.type === "condition" ||
              tgt.type === "group";
            if (canNest && dragged) {
              copy[idx] = {
                ...tgt,
                children: [...(tgt.children ?? []), dragged],
              };
            } else if (dragged) {
              copy.splice(idx + 1, 0, dragged);
            }
            return copy;
          }
          if (dragged)
            copy.splice(pos === "before" ? idx : idx + 1, 0, dragged);
          return copy;
        }
        return steps.map((s) => {
          let ns = s;
          if (s.children) ns = { ...ns, children: insertRec(s.children) };
          if (s.elseChildren)
            ns = { ...ns, elseChildren: insertRec(s.elseChildren) };
          return ns;
        });
      };
      const removed = removeRec(scenario.steps);
      if (!dragged) return;
      updateScenario(scenario.id, { steps: insertRec(removed) });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [scenario, updateScenario],
  );

  const scenarioRef = useRef(scenario);
  scenarioRef.current = scenario;
  const dragRef = useRef<{
    id: string;
    x: number;
    y: number;
    active: boolean;
    activate?: () => void;
  } | null>(null);
  const dropTargetRef = useRef<DropTarget>(null);
  const moveStepRef = useRef(moveStep);
  moveStepRef.current = moveStep;

  const onRowPointerDown = useCallback(
    (id: string, e: React.PointerEvent, activate?: () => void) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (
        target.closest("button, input, select, textarea, a, [role='menuitem']")
      )
        return;
      e.preventDefault();
      dragRef.current = {
        id,
        x: e.clientX,
        y: e.clientY,
        active: false,
        activate,
      };
      document.body.style.userSelect = "none";

      const onMove = (ev: PointerEvent): void => {
        const d = dragRef.current;
        if (!d) return;
        if (!d.active) {
          if (Math.hypot(ev.clientX - d.x, ev.clientY - d.y) < 5) return;
          d.active = true;
          setDragId(d.id);
        }
        const rows = Array.from(
          document.querySelectorAll<HTMLElement>("[data-step-id]"),
        );
        let targetEl: HTMLElement | null = null;
        for (const el of rows) {
          const r = el.getBoundingClientRect();
          if (ev.clientY >= r.top && ev.clientY <= r.bottom) {
            targetEl = el;
            break;
          }
        }
        if (!targetEl) {
          dropTargetRef.current = null;
          setDropTarget(null);
          return;
        }
        const tid = targetEl.getAttribute("data-step-id")!;
        if (tid === d.id) {
          dropTargetRef.current = null;
          setDropTarget(null);
          return;
        }
        if (isDescendantOf(scenarioRef.current?.steps ?? [], d.id, tid)) {
          dropTargetRef.current = null;
          setDropTarget(null);
          return;
        }
        const canNest = targetEl.getAttribute("data-can-nest") === "1";
        const r = targetEl.getBoundingClientRect();
        let pos: "before" | "after" | "inside";
        if (
          canNest &&
          ev.clientY > r.top + r.height * 0.3 &&
          ev.clientY < r.top + r.height * 0.7
        ) {
          pos = "inside";
        } else {
          pos = ev.clientY < r.top + r.height / 2 ? "before" : "after";
        }
        dropTargetRef.current = { id: tid, pos };
        setDropTarget({ id: tid, pos });
      };

      const onUp = (): void => {
        const d = dragRef.current;
        if (d) {
          if (d.active && dropTargetRef.current) {
            moveStepRef.current(
              d.id,
              dropTargetRef.current.id,
              dropTargetRef.current.pos,
            );
          } else if (!d.active) {
            d.activate?.();
          }
        }
        dragRef.current = null;
        dropTargetRef.current = null;
        setDragId(null);
        setDropTarget(null);
        document.body.style.userSelect = "";
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
      };

      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [],
  );

  const toggleCollapse = useCallback((id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  if (!scenario) return null;
  const stepMeta = getStepMeta();

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header: name + dirty + step actions + run */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <span className="text-sm font-semibold">{scenario.name}</span>
        <span className="text-xs text-muted-foreground">
          {scenario.steps.length}
          {t("scenario.stepsCount")}
        </span>
        {dirty && (
          <span className="text-xs text-warning" title={t("scenario.dirty")}>
            ● {t("scenario.dirty")}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" className="h-7">
                <Plus className="h-3.5 w-3.5" /> {t("scenario.addStep")}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-40">
              {(Object.keys(stepMeta) as StepType[]).map((type) => {
                const Meta = stepMeta[type];
                const Icon = Meta.icon;
                return (
                  <DropdownMenuItem
                    key={type}
                    onClick={() => addRootStep(type)}
                  >
                    <Icon className={`h-3.5 w-3.5 ${Meta.color}`} />{" "}
                    {t(Meta.labelKey)}
                  </DropdownMenuItem>
                );
              })}
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            size="sm"
            className="h-7"
            disabled={!!running}
            onClick={() => void runTarget({ type: "case", id: scenario.id })}
          >
            <Play className="h-3.5 w-3.5" /> {t("scenario.run")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-7"
            disabled={!dirty}
            onClick={() => saveScenario(scenario.id)}
          >
            <Save className="h-3.5 w-3.5" /> {t("scenario.save")}
          </Button>
          {dirty && (
            <Button
              size="sm"
              variant="ghost"
              className="h-7"
              onClick={() => restoreScenario(scenario.id)}
            >
              {t("scenario.restore")}
            </Button>
          )}
          <Button
            size="sm"
            variant={configOpen ? "secondary" : "outline"}
            className="h-7"
            onClick={() => setConfigOpen((v) => !v)}
            title={t("scenario.runConfig")}
          >
            <Settings2 className="h-3.5 w-3.5" /> {t("scenario.runConfig")}
          </Button>
        </div>
      </div>

      {/* Main area (left) and run-config panel (right, collapsible) share a horizontal split: expanded it shrinks the main area, collapsed the main area takes full width */}
      <PanelGroup
        direction="horizontal"
        autoSaveId="automation-editor-layout"
        className="min-h-0 flex-1"
      >
        <Panel id="main" order={1} minSize={30} className="min-w-0">
          <PanelGroup
            direction="vertical"
            autoSaveId="automation-run-layout"
            className="h-full min-h-0"
          >
            {/* Step orchestration area */}
            <Panel minSize={25}>
              <ScrollArea className="h-full">
                <div className="p-3">
                  {scenario.steps.length > 0 ? (
                    scenario.steps.map((st) => (
                      <StepNode
                        key={st.id}
                        step={st}
                        depth={0}
                        patch={patchStep}
                        remove={removeStep}
                        t={t}
                        dragId={dragId}
                        dropTarget={dropTarget}
                        onRowPointerDown={onRowPointerDown}
                        collapsed={collapsed}
                        toggleCollapse={toggleCollapse}
                        insertAfterFn={insertAfter}
                        insertBeforeFn={insertBefore}
                        insertChildFn={insertChild}
                      />
                    ))
                  ) : (
                    <div className="py-10 text-center text-sm text-muted-foreground">
                      {t("scenario.emptyHint")}
                    </div>
                  )}
                </div>
              </ScrollArea>
            </Panel>

            <PanelResizeHandle className="h-1 bg-border transition-colors hover:bg-primary" />

            {/* Run results / history report */}
            <Panel defaultSize={35} minSize={15}>
              <Tabs
                value={tab}
                onValueChange={setTab}
                className="flex h-full min-h-0 flex-col gap-0"
              >
                <div className="flex shrink-0 items-center border-b border-border px-3 py-1">
                  <TabsList className="h-7 bg-transparent p-0">
                    <TabsTrigger value="run" className="text-xs">
                      {t("run.tab.current")}
                    </TabsTrigger>
                    <TabsTrigger value="reports" className="text-xs">
                      {t("run.tab.reports")}
                    </TabsTrigger>
                  </TabsList>
                  {isRunning && (
                    <Badge
                      variant="outline"
                      className="ml-2 animate-pulse text-warning"
                    >
                      {t("scenario.running")}
                    </Badge>
                  )}
                </div>
                <TabsContent value="run" className="mt-0 min-h-0 flex-1">
                  <RunPanel />
                </TabsContent>

                <TabsContent value="reports" className="mt-0 min-h-0 flex-1">
                  <ReportPanel />
                </TabsContent>
              </Tabs>
            </Panel>
          </PanelGroup>
        </Panel>

        {configOpen && (
          <>
            <PanelResizeHandle className="w-1 bg-border transition-colors hover:bg-primary" />
            <Panel
              id="settings"
              order={2}
              defaultSize={26}
              minSize={18}
              maxSize={42}
              className="min-w-0"
            >
              <CaseConfigPanel
                scenario={scenario}
                onManageDataSets={() => setDataSetOpen(true)}
                onClose={() => setConfigOpen(false)}
              />
            </Panel>
          </>
        )}
      </PanelGroup>

      <DataSetDialog open={dataSetOpen} onOpenChange={setDataSetOpen} />
    </div>
  );
}
