// "Execution timing" hint entry: tucked into a tooltip beside the "syntax reference"; the panel keeps only the syntax reference + action list,
// no longer spreading large descriptive text (shared by pre / post; content is chosen by kind).
//
// The pre list contains a built-in "interpolation" node: before the node = pre-interpolation (can produce variables for this interpolation to consume, can rewrite the template),
// after the node = post-interpolation (gets the final message, good for signing / encryption); the post list executes in order after the response is received.
import { Info } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useT } from "@/lib/i18n";

export function ScriptTimingHint({ kind }: { kind: "pre" | "post" }) {
  const { t } = useT();
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0 text-muted-foreground"
            aria-label={t("script.timingHint")}
          >
            <Info className="h-3.5 w-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align="end"
          className="max-w-sm text-left text-xs leading-relaxed"
        >
          {kind === "pre" ? (
            <div className="space-y-1.5">
              <p>{t("script.preHint")}</p>
              <p className="text-background/70">
                {t("script.preResolveTemplateHint")}
              </p>
            </div>
          ) : (
            <p>{t("script.postHint")}</p>
          )}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
