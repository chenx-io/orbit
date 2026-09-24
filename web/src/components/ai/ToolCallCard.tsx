// Tool call card: one "record" row plus expandable arguments/results.
//
// It is meant to be a **lightweight record** (matching the size of the "thinking" collapsible block): one row by default,
// showing the tool name and status while collapsed, with arguments and results (possibly large JSON) expanding on demand.
import { useState } from "react";
import {
  Ban,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  ListChecks,
  Loader2,
  PenLine,
  Play,
  Search,
  ShieldQuestion,
  XCircle,
} from "lucide-react";
import type { AiToolCard as AiToolCardData } from "@/data/aiTypes";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { toolLabelKey, toolSubject } from "@/lib/ai/proposal";

/** Tool name → icon (falling back to a magnifier when not listed). */
function iconFor(name: string) {
  if (name === "present_plan") return ListChecks;
  if (name.startsWith("create_") || name.startsWith("update_")) return PenLine;
  if (name.startsWith("run_")) return Play;
  return Search;
}

/** Status → icon and colors (the status dot's accent color). */
function statusMeta(status: AiToolCardData["status"]) {
  switch (status) {
    case "running":
      return { icon: Loader2, className: "text-primary", spin: true };
    case "completed":
      return { icon: CheckCircle2, className: "text-success", spin: false };
    case "failed":
      return { icon: XCircle, className: "text-destructive", spin: false };
    case "denied":
      return { icon: Ban, className: "text-muted-foreground", spin: false };
    default:
      return { icon: ShieldQuestion, className: "text-warning", spin: false };
  }
}

export interface ToolCallCardProps {
  card: AiToolCardData;
  /**
   * Authorization decision callback (**required while awaiting confirmation**, otherwise the flow stalls on the wait).
   *
   * Today only execution-type tools reach the pending state: writes persist directly in Agent mode and
   * Ask / Plan modes have no write tools at all.
   */
  onDecide?: (allow: boolean) => void;
}

export function ToolCallCard({ card, onDecide }: ToolCallCardProps) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const Icon = iconFor(card.name);
  const meta = statusMeta(card.status);
  const StatusIcon = meta.icon;
  const pending = card.status === "pendingApproval";
  const hasDetail =
    Object.keys(card.arguments ?? {}).length > 0 || card.summary !== undefined;
  // The collapsed row must convey "what this call did": showing only "Create request" makes consecutive calls indistinguishable
  const subject = toolSubject(card.arguments);

  return (
    <div className="rounded-md bg-muted/30">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        disabled={!hasDetail}
        className={cn(
          "flex w-full items-center gap-1.5 px-2 py-1 text-left",
          hasDetail && "cursor-pointer hover:bg-accent/40",
        )}
      >
        {hasDetail ? (
          open ? (
            <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
          )
        ) : (
          <span className="w-3 shrink-0" />
        )}
        <Icon className="h-3 w-3 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate text-xs">
          {t(toolLabelKey(card.name), card.name)}
          {subject && (
            <span className="text-muted-foreground"> · {subject}</span>
          )}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {t(`ai.status.${card.status}`)}
        </span>
        <StatusIcon
          className={cn(
            "h-3 w-3 shrink-0",
            meta.className,
            meta.spin && "animate-spin",
          )}
        />
      </button>

      {pending && onDecide && (
        <div className="flex flex-wrap items-center gap-1.5 border-t border-border px-2 py-1">
          <span className="text-xs text-warning">{t("ai.approval.hint")}</span>
          <div className="ml-auto flex items-center gap-1">
            <Button size="xs" variant="ghost" onClick={() => onDecide(false)}>
              {t("ai.approval.reject")}
            </Button>
            <Button size="xs" onClick={() => onDecide(true)}>
              <Check className="h-3 w-3" />
              {t("ai.approval.allow")}
            </Button>
          </div>
        </div>
      )}

      {open && hasDetail && (
        <div className="space-y-1.5 border-t border-border px-2 py-1.5">
          {card.summary && (
            <p className="text-xs text-muted-foreground">{card.summary}</p>
          )}
          {Object.keys(card.arguments ?? {}).length > 0 && (
            <pre className="max-h-40 overflow-auto rounded bg-background/70 p-1.5 font-mono text-xs">
              {JSON.stringify(card.arguments, null, 2)}
            </pre>
          )}
          {card.payload !== undefined && card.payload !== null && (
            <pre className="max-h-52 overflow-auto rounded bg-background/70 p-1.5 font-mono text-xs">
              {JSON.stringify(card.payload, null, 2)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
