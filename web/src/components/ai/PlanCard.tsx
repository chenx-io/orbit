// Plan card: the output of Plan mode, pinned above the input (always visible, never scrolled away with messages).
//
// Why pinned here rather than in the message stream: the plan is a "proposal awaiting confirmation" the user may revise over several turns,
// scrolling through history meanwhile; only by staying above the input does the card keep offering the "Start implementing" entry point.
import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  ListChecks,
  Play,
  TriangleAlert,
} from "lucide-react";
import type { AiMode, AiPlanArtifact } from "@/data/aiTypes";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";

export interface PlanCardProps {
  plan: AiPlanArtifact;
  /** Current mode: expanded with "Start implementing" under Plan; collapsed to one row after switching to Agent (still queryable) */
  mode: AiMode;
  /** Disabled while generating (to avoid clashing with the turn in progress) */
  busy: boolean;
  onImplement: () => void;
}

export function PlanCard({ plan, mode, busy, onImplement }: PlanCardProps) {
  const { t, format } = useT();
  const [manuallyOpen, setManuallyOpen] = useState(false);
  const planning = mode === "plan";
  const expanded = planning || manuallyOpen;

  return (
    <div className="mx-3 mt-2 shrink-0 overflow-hidden rounded-md border border-violet-500/40 bg-violet-500/5">
      <div className="flex items-center gap-2 px-2.5 py-1.5">
        <button
          type="button"
          onClick={() => setManuallyOpen((v) => !v)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
        >
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          )}
          <ListChecks className="h-3.5 w-3.5 shrink-0 text-violet-400" />
          <span className="min-w-0 flex-1 truncate text-xs font-semibold">
            {plan.title}
          </span>
        </button>
        <span className="shrink-0 text-xs text-muted-foreground">
          {format("ai.plan.revision", plan.revision, plan.steps.length)}
        </span>
        {planning && (
          <Button
            size="xs"
            className="shrink-0"
            disabled={busy}
            onClick={onImplement}
            title={t("ai.plan.implementHint")}
          >
            <Play className="h-3 w-3" />
            {t("ai.plan.implement")}
          </Button>
        )}
      </div>

      {expanded && (
        <div className="space-y-2 border-t border-violet-500/30 px-2.5 py-2">
          {plan.summary && (
            <p className="text-xs text-muted-foreground">{plan.summary}</p>
          )}
          <ol className="space-y-1">
            {plan.steps.map((step, idx) => (
              <li key={idx} className="flex gap-2">
                <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-violet-500/15 font-mono text-xs text-violet-400">
                  {idx + 1}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-xs font-medium">
                    {step.title}
                  </span>
                  {step.detail && (
                    <span className="block text-xs text-muted-foreground">
                      {step.detail}
                    </span>
                  )}
                </span>
              </li>
            ))}
          </ol>
          {plan.notes.length > 0 && (
            <ul className="space-y-0.5 border-t border-violet-500/20 pt-1.5">
              {plan.notes.map((note, idx) => (
                <li
                  key={idx}
                  className="flex items-start gap-1.5 text-xs text-muted-foreground"
                >
                  <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0 text-warning" />
                  <span className="min-w-0 flex-1">{note}</span>
                </li>
              ))}
            </ul>
          )}
          {planning && (
            <p className="text-xs text-muted-foreground">{t("ai.plan.hint")}</p>
          )}
        </div>
      )}
    </div>
  );
}
