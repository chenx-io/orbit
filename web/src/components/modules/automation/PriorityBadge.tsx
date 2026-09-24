// Scenario priority badge (P0–P3): semantic-token coloring, rounded-full text-xs.
import type { ScenarioPriority } from "@/data/types";
import { cn } from "@/lib/utils";

const META: Record<ScenarioPriority, { label: string; cls: string }> = {
  p0: {
    label: "P0",
    cls: "border-destructive/40 bg-destructive/10 text-destructive",
  },
  p1: { label: "P1", cls: "border-warning/40 bg-warning/10 text-warning" },
  p2: {
    label: "P2",
    cls: "border-border bg-secondary text-secondary-foreground",
  },
  p3: { label: "P3", cls: "border-border bg-muted text-muted-foreground" },
};

export function PriorityBadge({
  priority,
  className,
}: {
  priority?: ScenarioPriority;
  className?: string;
}) {
  if (!priority) return null;
  const meta = META[priority];
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-full border px-1.5 py-px text-xs font-medium leading-none",
        meta.cls,
        className,
      )}
    >
      {meta.label}
    </span>
  );
}
