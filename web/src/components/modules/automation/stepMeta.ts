// Helper functions for scenario steps (ScenarioStep): type metadata, step creation, cycle detection.
import { Clock, Folder, GitBranch, Repeat, Send, Variable } from "lucide-react";
import type { ScenarioStep, StepType } from "@/data/types";
import { uid } from "@/data/seed";

export type StepMeta = { icon: typeof Send; labelKey: string; color: string };

export function getStepMeta(): Record<StepType, StepMeta> {
  return {
    request: {
      icon: Send,
      labelKey: "scenario.step.request",
      color: "text-primary",
    },
    loop: {
      icon: Repeat,
      labelKey: "scenario.step.loop",
      color: "text-violet-400",
    },
    condition: {
      icon: GitBranch,
      labelKey: "scenario.step.condition",
      color: "text-amber-400",
    },
    wait: {
      icon: Clock,
      labelKey: "scenario.step.wait",
      color: "text-cyan-400",
    },
    setvar: {
      icon: Variable,
      labelKey: "scenario.step.setvar",
      color: "text-emerald-400",
    },
    group: {
      icon: Folder,
      labelKey: "scenario.step.group",
      color: "text-rose-400",
    },
  };
}

export function newStep(
  type: StepType,
  t: (k: string) => string,
): ScenarioStep {
  const meta = getStepMeta();
  return {
    id: uid("st"),
    type,
    name: t(meta[type].labelKey),
    ...(type === "loop" ? { count: 3, children: [] } : {}),
    ...(type === "wait" ? { ms: 500 } : {}),
    ...(type === "setvar" ? { varKey: "var", varValue: "" } : {}),
    ...(type === "condition" ? { expr: "{{status}} == 200" } : {}),
  };
}

// Check whether nodeId lies within ancestorId's subtree (used to forbid dragging a node into its own subtree, which would create a cycle)
export function isDescendantOf(
  steps: ScenarioStep[],
  ancestorId: string,
  nodeId: string,
): boolean {
  for (const s of steps) {
    if (s.id === ancestorId) {
      const walk = (n: ScenarioStep): boolean =>
        n.id === nodeId ||
        (n.children ?? []).some(walk) ||
        (n.elseChildren ?? []).some(walk);
      return walk(s);
    }
    if (
      (s.children && isDescendantOf(s.children, ancestorId, nodeId)) ||
      (s.elseChildren && isDescendantOf(s.elseChildren, ancestorId, nodeId))
    ) {
      return true;
    }
  }
  return false;
}
