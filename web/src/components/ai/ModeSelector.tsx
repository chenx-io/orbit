// Work mode selector (the first cell of the input's bottom bar): Ask / Agent / Plan.
//
// The difference between the modes is not just copy — it decides **which tools the Rust side sends to the model** and **whether writes persist**:
// - Ask: read-only conversation (write and execution tools are invisible)
// - Agent: full capability (writes persist directly; execution tools confirm each time)
// - Plan: read-only planning (produces a plan; clicking "Start implementing" switches to Agent)
// So this component only handles "selection"; the real admission control is enforced in the engine.
import {
  Bot,
  Check,
  ChevronDown,
  ListChecks,
  MessageCircle,
} from "lucide-react";
import type { AiMode } from "@/data/aiTypes";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { AI_MODE_ORDER } from "@/lib/ai/constants";

const MODE_ICONS: Record<AiMode, typeof Bot> = {
  ask: MessageCircle,
  agent: Bot,
  plan: ListChecks,
};

/**
 * Mode → accent color.
 *
 * Agent uses the primary color (writable, the most "heavyweight"); Plan keeps the AI brand purple (matching the drawer's empty-state icon, meaning "draft");
 * Ask stays neutral gray. success/warning/destructive are not used — they are status colors, not mode colors.
 */
function modeTone(mode: AiMode): string {
  if (mode === "agent") return "text-primary";
  if (mode === "plan") return "text-violet-400";
  return "text-muted-foreground";
}

export interface ModeSelectorProps {
  mode: AiMode;
  onChange: (mode: AiMode) => void;
}

export function ModeSelector({ mode, onChange }: ModeSelectorProps) {
  const { t } = useT();
  const Icon = MODE_ICONS[mode];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={t("ai.mode.hint")}
          // shrink-0: when the bottom bar runs out of room the model name truncates while the mode pill stays intact (it is very short anyway)
          className="flex h-7 shrink-0 cursor-pointer items-center gap-1 rounded-full border border-border px-2 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <Icon className={cn("h-3.5 w-3.5", modeTone(mode))} />
          <span className="shrink-0 font-medium text-foreground">
            {t(`ai.mode.${mode}`)}
          </span>
          <span className="shrink-0 opacity-60">·</span>
          <span className="max-w-20 truncate">{t(`ai.mode.${mode}Short`)}</span>
          <ChevronDown className="h-3 w-3 shrink-0" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-80">
        <DropdownMenuLabel>{t("ai.mode.label")}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {AI_MODE_ORDER.map((key) => {
          const ItemIcon = MODE_ICONS[key];
          return (
            <DropdownMenuItem
              key={key}
              onSelect={() => onChange(key)}
              className="items-start gap-2"
            >
              <ItemIcon
                className={cn("mt-0.5 h-3.5 w-3.5 shrink-0", modeTone(key))}
              />
              <span className="min-w-0 flex-1">
                <span className="block text-xs font-medium">
                  {t(`ai.mode.${key}`)}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t(`ai.mode.${key}Hint`)}
                </span>
              </span>
              {key === mode && (
                <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              )}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
