import type { ScenarioRunStep } from "@/data/types";

/** Step-result filter: all steps, only passed, only failed */
export type ResultFilterValue = "all" | "pass" | "fail";

/** Filter run steps by result status (shared by the live run panel and the history report detail) */
export function filterSteps(
  steps: ScenarioRunStep[],
  f: ResultFilterValue,
): ScenarioRunStep[] {
  if (f === "pass") return steps.filter((s) => s.status === "pass");
  if (f === "fail") return steps.filter((s) => s.status === "fail");
  return steps;
}
