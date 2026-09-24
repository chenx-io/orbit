// Scenario step node: card rendering for a single step (type-field editing + insert/delete/enable-disable),
// recursively rendering THEN children and the ELSE branch.
import {
  ChevronDown,
  ChevronRight,
  CornerDownRight,
  Plus,
  Trash2,
} from "lucide-react";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { RequestTreeSelect } from "@/components/common/RequestTreeSelect";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ScenarioStep, StepType } from "@/data/types";
import { getStepMeta, newStep } from "./stepMeta";

export function StepNode({
  step,
  depth,
  patch,
  remove,
  t,
  dragId,
  dropTarget,
  onRowPointerDown,
  collapsed,
  toggleCollapse,
  insertAfterFn,
  insertBeforeFn,
  insertChildFn,
}: {
  step: ScenarioStep;
  depth: number;
  patch: (id: string, p: Partial<ScenarioStep>) => void;
  remove: (id: string) => void;
  t: (k: string) => string;
  dragId: string | null;
  dropTarget: { id: string; pos: "before" | "after" | "inside" } | null;
  onRowPointerDown: (
    id: string,
    e: React.PointerEvent,
    activate?: () => void,
  ) => void;
  collapsed: Set<string>;
  toggleCollapse: (id: string) => void;
  insertAfterFn: (targetId: string, type: StepType) => void;
  insertBeforeFn: (targetId: string, type: StepType) => void;
  insertChildFn: (targetId: string, type: StepType) => void;
}) {
  const meta = getStepMeta()[step.type];
  const Icon = meta.icon;
  const children = step.children ?? [];
  const isCollapsed = collapsed.has(step.id);
  const hasThen = children.length > 0;
  const canNest =
    step.type === "loop" || step.type === "condition" || step.type === "group";
  const stepMeta = getStepMeta();

  // Bound to the current step.id; child nodes receive the raw functions via props and bind themselves
  const insertAfter = (type: StepType) => insertAfterFn(step.id, type);
  const insertBefore = (type: StepType) => insertBeforeFn(step.id, type);
  const insertChild = (type: StepType) => insertChildFn(step.id, type);

  const setChild = (id: string, p: Partial<ScenarioStep>) =>
    patch(step.id, {
      children: children.map((c) => (c.id === id ? { ...c, ...p } : c)),
    });
  const removeChild = (id: string) =>
    patch(step.id, { children: children.filter((c) => c.id !== id) });
  const addElseBranch = () => {
    const existing = step.elseChildren ?? [];
    patch(step.id, {
      elseChildren: [
        ...existing,
        {
          ...newStep("request", t),
          name: `${t("scenario.elseBranch")} ${existing.length + 1}`,
        },
      ],
    });
  };
  const removeElseBranch = () => patch(step.id, { elseChildren: [] });
  const addElseChild = (type: StepType) =>
    patch(step.id, {
      elseChildren: [
        ...(step.elseChildren ?? []),
        { ...newStep(type, t), name: `${t("scenario.elseBranch")} ${type}` },
      ],
    });

  // Drop indicator: during drag only mark the position (before/after blue line, inside highlight); actual reorder happens on release
  const isBefore = dropTarget?.id === step.id && dropTarget.pos === "before";
  const isAfter = dropTarget?.id === step.id && dropTarget.pos === "after";
  const isInside = dropTarget?.id === step.id && dropTarget.pos === "inside";

  return (
    <div style={{ marginLeft: depth * 22 }}>
      {/* Conditional type: a background container wraps both the IF and ELSE branches; non-conditional types get no extra styling */}
      <div
        className={
          step.type === "condition"
            ? "rounded-md border border-rose-500/15 bg-rose-500/3 p-2"
            : ""
        }
      >
        {/* ── Step card ── */}
        <div
          data-step-id={step.id}
          data-can-nest={canNest ? "1" : "0"}
          onPointerDown={(e) => onRowPointerDown(step.id, e)}
          className={`relative mb-1.5 rounded-md border border-border bg-card/60 p-2 ${dragId === step.id ? "opacity-40" : ""} ${isInside ? "ring-2 ring-ring bg-ring/10" : ""}`}
          style={{ touchAction: "none" }}
        >
          {isBefore && (
            <div className="pointer-events-none absolute inset-x-0 -top-0.75 h-0.5 rounded bg-primary" />
          )}
          {isAfter && (
            <div className="pointer-events-none absolute inset-x-0 -bottom-0.75 h-0.5 rounded bg-primary" />
          )}
          <div className="flex items-center gap-2">
            {/* Expand / collapse */}
            {hasThen ? (
              <button
                type="button"
                onClick={() => toggleCollapse(step.id)}
                className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent"
                title={
                  isCollapsed ? t("scenario.expand") : t("scenario.collapse")
                }
              >
                {isCollapsed ? (
                  <ChevronRight className="h-4 w-4" />
                ) : (
                  <ChevronDown className="h-4 w-4" />
                )}
              </button>
            ) : (
              <span className="h-4 w-4 shrink-0" />
            )}
            <Icon className={`h-4 w-4 shrink-0 ${meta.color}`} />
            {step.type === "condition" && (
              <span className="shrink-0 text-xs font-semibold uppercase text-amber-400">
                IF
              </span>
            )}
            {step.type === "group" && (
              <Input
                value={step.name}
                onChange={(e) => patch(step.id, { name: e.target.value })}
                className="h-7 w-44 text-xs"
                placeholder={t("scenario.groupName")}
              />
            )}
            <Badge variant="outline" className="shrink-0">
              {t(meta.labelKey)}
            </Badge>

            {step.type === "request" && (
              <RequestTreeSelect
                value={step.requestId ?? ""}
                onChange={(v) => patch(step.id, { requestId: v })}
                placeholder={t("scenario.selectRequest")}
                className="h-7 w-44"
              />
            )}
            {step.type === "loop" && (
              <div className="flex items-center gap-1 text-xs text-muted-foreground">
                {t("scenario.loopCount")}
                <Input
                  type="number"
                  value={step.count ?? 1}
                  onChange={(e) =>
                    patch(step.id, { count: parseInt(e.target.value) || 1 })
                  }
                  className="h-7 w-16 text-xs"
                />
              </div>
            )}
            {step.type === "wait" && (
              <div className="flex items-center gap-1 text-xs text-muted-foreground">
                <Input
                  type="number"
                  value={step.ms ?? 0}
                  onChange={(e) =>
                    patch(step.id, { ms: parseInt(e.target.value) || 0 })
                  }
                  className="h-7 w-20 text-xs"
                />
                ms
              </div>
            )}
            {step.type === "setvar" && (
              <div className="flex items-center gap-1">
                <Input
                  value={step.varKey ?? ""}
                  onChange={(e) => patch(step.id, { varKey: e.target.value })}
                  className="h-7 w-24 text-xs"
                  placeholder={t("scenario.varName")}
                />
                <span className="text-xs text-muted-foreground">=</span>
                <Input
                  value={step.varValue ?? ""}
                  onChange={(e) => patch(step.id, { varValue: e.target.value })}
                  className="h-7 w-28 font-mono text-xs"
                  placeholder={t("scenario.varValue")}
                />
              </div>
            )}
            {step.type === "condition" && (
              <div className="flex items-center gap-1">
                <Input
                  value={step.expr ?? ""}
                  onChange={(e) => patch(step.id, { expr: e.target.value })}
                  className="h-7 w-44 font-mono text-xs"
                  placeholder={t("scenario.conditionExpr")}
                />
              </div>
            )}
            {/* Right-side action area */}
            <div className="ml-auto flex shrink-0 items-center gap-0.5">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="h-6 w-6"
                    onClick={(e) => e.stopPropagation()}
                    title={t("scenario.addStep")}
                  >
                    <Plus className="h-3.5 w-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="min-w-35 z-50">
                  <div className="px-2 py-1 text-xs font-medium text-muted-foreground">
                    {t("scenario.insertAfter")}
                  </div>
                  {(Object.keys(stepMeta) as StepType[]).map((type) => {
                    const M = stepMeta[type];
                    const Ico = M.icon;
                    return (
                      <DropdownMenuItem
                        key={type}
                        onClick={() => insertAfter(type)}
                      >
                        <Ico className={`h-3.5 w-3.5 ${M.color}`} />{" "}
                        {t(M.labelKey)}
                      </DropdownMenuItem>
                    );
                  })}
                  <DropdownMenuSeparator />
                  <DropdownMenuSub>
                    <DropdownMenuSubTrigger>
                      {t("scenario.insertBefore")}
                    </DropdownMenuSubTrigger>
                    <DropdownMenuSubContent>
                      {(Object.keys(stepMeta) as StepType[]).map((type) => {
                        const M = stepMeta[type];
                        const Ico = M.icon;
                        return (
                          <DropdownMenuItem
                            key={type}
                            onClick={() => insertBefore(type)}
                          >
                            <Ico className={`h-3.5 w-3.5 ${M.color}`} />{" "}
                            {t(M.labelKey)}
                          </DropdownMenuItem>
                        );
                      })}
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                  {canNest && (
                    <DropdownMenuSub>
                      <DropdownMenuSubTrigger>
                        {t("scenario.insertChild")}
                      </DropdownMenuSubTrigger>
                      <DropdownMenuSubContent>
                        {(Object.keys(stepMeta) as StepType[]).map((type) => {
                          const M = stepMeta[type];
                          const Ico = M.icon;
                          return (
                            <DropdownMenuItem
                              key={type}
                              onClick={() => insertChild(type)}
                            >
                              <Ico className={`h-3.5 w-3.5 ${M.color}`} />{" "}
                              {t(M.labelKey)}
                            </DropdownMenuItem>
                          );
                        })}
                      </DropdownMenuSubContent>
                    </DropdownMenuSub>
                  )}
                  {step.type === "condition" && (
                    <>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onClick={() => {
                          addElseBranch();
                        }}
                        disabled={(step.elseChildren ?? []).length > 0}
                        className={
                          (step.elseChildren ?? []).length > 0
                            ? "opacity-40"
                            : ""
                        }
                      >
                        <CornerDownRight className="h-3.5 w-3.5 text-rose-400" />{" "}
                        {t("scenario.addElseStep")}
                      </DropdownMenuItem>
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
              <Switch
                checked={!step.disabled}
                onCheckedChange={(v) => patch(step.id, { disabled: !v })}
                className="scale-75"
              />
              <Button
                variant="ghost"
                size="icon-sm"
                className=""
                onClick={() => remove(step.id)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* The case already shares the full interface content (pre/post scripts, assertions, etc.); no need to "extract" it separately on the step */}
        </div>

        {/* THEN children */}
        {!isCollapsed &&
          children.map((c) => (
            <StepNode
              key={c.id}
              step={c}
              depth={depth + 1}
              patch={setChild}
              remove={removeChild}
              t={t}
              dragId={dragId}
              dropTarget={dropTarget}
              onRowPointerDown={onRowPointerDown}
              collapsed={collapsed}
              toggleCollapse={toggleCollapse}
              insertAfterFn={insertAfterFn}
              insertBeforeFn={insertBeforeFn}
              insertChildFn={insertChildFn}
            />
          ))}

        {/* ELSE branch card + children */}
        {(step.elseChildren ?? []).length > 0 && (
          <div className="mt-2">
            <div className="relative flex items-center gap-2 rounded-md border border-border bg-card/60 p-2 mb-1.5">
              {/* ELSE expand / collapse */}
              {collapsed.has("else:" + step.id) ? (
                <button
                  type="button"
                  onClick={() => toggleCollapse("else:" + step.id)}
                  className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent"
                  title={t("scenario.expand")}
                >
                  <ChevronRight className="h-4 w-4" />
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => toggleCollapse("else:" + step.id)}
                  className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent"
                  title={t("scenario.collapse")}
                >
                  <ChevronDown className="h-4 w-4" />
                </button>
              )}
              <CornerDownRight className="h-4 w-4 shrink-0 text-rose-400" />
              <span className="shrink-0 text-xs font-semibold uppercase text-rose-400">
                ELSE
              </span>
              <div className="ml-auto flex shrink-0 items-center gap-0.5">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="h-6 w-6"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Plus className="h-3.5 w-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-35 z-50">
                    <div className="px-2 py-1 text-xs font-medium text-muted-foreground">
                      {t("scenario.insertChild")}
                    </div>
                    {(Object.keys(stepMeta) as StepType[]).map((type) => {
                      const M = stepMeta[type];
                      const Ico = M.icon;
                      return (
                        <DropdownMenuItem
                          key={type}
                          onClick={() => addElseChild(type)}
                        >
                          <Ico className={`h-3.5 w-3.5 ${M.color}`} />{" "}
                          {t(M.labelKey)}
                        </DropdownMenuItem>
                      );
                    })}
                  </DropdownMenuContent>
                </DropdownMenu>
                <Switch
                  checked={!step.disabled}
                  onCheckedChange={(v) => patch(step.id, { disabled: !v })}
                  className="scale-75"
                />
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => removeElseBranch()}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
            {!collapsed.has("else:" + step.id) &&
              (step.elseChildren ?? []).map((c) => (
                <StepNode
                  key={c.id}
                  step={c}
                  depth={depth + 1}
                  patch={(id, p) =>
                    patch(step.id, {
                      elseChildren: (step.elseChildren ?? []).map((sc) =>
                        sc.id === id ? { ...sc, ...p } : sc,
                      ),
                    })
                  }
                  remove={(id) =>
                    patch(step.id, {
                      elseChildren: (step.elseChildren ?? []).filter(
                        (sc) => sc.id !== id,
                      ),
                    })
                  }
                  t={t}
                  dragId={dragId}
                  dropTarget={dropTarget}
                  onRowPointerDown={onRowPointerDown}
                  collapsed={collapsed}
                  toggleCollapse={toggleCollapse}
                  insertAfterFn={insertAfterFn}
                  insertBeforeFn={insertBeforeFn}
                  insertChildFn={insertChildFn}
                />
              ))}
          </div>
        )}
      </div>
    </div>
  );
}
