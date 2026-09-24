// Built-in "interpolation" node: turns the request template into the final message (variable interpolation + request body assembly).
//
// Maintained by the system; the only positional marker in the pre-action list:
// - not editable / copyable / deletable / disableable / draggable;
// - actions *before* it = pre-interpolation (can write variables for this interpolation to consume, rewrite the template);
// - actions *after* it = post-interpolation (rewriting yields the final bytes, good for signing / encryption).
import { ArrowLeftRight } from "lucide-react";
import { useT } from "@/lib/i18n";
import type { ActionLog } from "@/data/types";

export function InterpolateNode({ result }: { result?: ActionLog }) {
  const { t } = useT();
  return (
    <div
      className="flex items-center gap-2 rounded-lg border border-dashed border-border bg-muted/30 px-2.5 py-2"
      title={t("action.interpolateHint")}
    >
      <ArrowLeftRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <span className="shrink-0 text-xs font-medium">
        {t("action.interpolateNode")}
      </span>
      <span className="shrink-0 rounded bg-muted px-1 py-0.5 text-[10px] text-muted-foreground">
        {t("action.interpolateBuiltin")}
      </span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
        {t("action.interpolateHint")}
      </span>
      {result && result.elapsedMs > 0 && (
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {result.elapsedMs}ms
        </span>
      )}
    </div>
  );
}
