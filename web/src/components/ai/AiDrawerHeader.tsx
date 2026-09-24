// AI drawer header: session switch/rename/delete, new, settings, close, plus workspace/environment chips.
//
// Note: entity-level context is not here — the `ReferencePicker` above the input carries it (addable, removable and more complete).
// The header keeps only the information that "these two values are environment-level and users do not delete them one by one".
import { useState } from "react";
import {
  ChevronDown,
  MessageSquarePlus,
  Pencil,
  Settings2,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { AiSessionSummary } from "@/data/aiTypes";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useT } from "@/lib/i18n";

export interface AiDrawerHeaderProps {
  title: string;
  sessions: AiSessionSummary[];
  activeSessionId: string | null;
  /** Data source for the context chips (names only, never a secret value) */
  workspaceName: string | null;
  environmentName: string | null;
  onOpenSession: (id: string) => void;
  onNewSession: () => void;
  onRenameSession: (title: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenSettings: () => void;
}

export function AiDrawerHeader({
  title,
  sessions,
  activeSessionId,
  workspaceName,
  environmentName,
  onOpenSession,
  onNewSession,
  onRenameSession,
  onDeleteSession,
  onOpenSettings,
}: AiDrawerHeaderProps) {
  const { t } = useT();
  const [renaming, setRenaming] = useState(false);

  // Environment-level chips: workspace / active environment
  const chips: { key: string; label: string }[] = [];
  if (workspaceName) chips.push({ key: "ws", label: workspaceName });
  if (environmentName) chips.push({ key: "env", label: environmentName });

  return (
    <div className="shrink-0 border-b border-border">
      <div className="flex items-center gap-1.5 px-3 py-2">
        <Sparkles className="h-4 w-4 shrink-0 text-violet-400" />

        {renaming ? (
          <input
            autoFocus
            defaultValue={title}
            onBlur={(e) => {
              const next = e.target.value.trim();
              if (next && next !== title) onRenameSession(next);
              setRenaming(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") setRenaming(false);
            }}
            className="w-40 rounded-md border border-input bg-transparent px-1.5 py-0.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
          />
        ) : (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="flex min-w-0 cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-sm font-semibold hover:bg-accent"
              >
                <span className="max-w-52 truncate">{title}</span>
                <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-64">
              <DropdownMenuLabel>{t("ai.sessions")}</DropdownMenuLabel>
              <DropdownMenuSeparator />
              {sessions.length === 0 && (
                <DropdownMenuItem disabled>
                  {t("ai.session.empty")}
                </DropdownMenuItem>
              )}
              {sessions.map((s) => (
                <DropdownMenuItem
                  key={s.id}
                  onSelect={() => onOpenSession(s.id)}
                >
                  <span className="min-w-0 flex-1 truncate">{s.title}</span>
                  {s.id === activeSessionId && (
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {t("ai.session.current")}
                    </span>
                  )}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => setRenaming(true)}>
                <Pencil className="h-3.5 w-3.5" />
                {t("ai.session.rename")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onNewSession}>
                <MessageSquarePlus className="h-3.5 w-3.5" />
                {t("ai.session.new")}
              </DropdownMenuItem>
              {activeSessionId && (
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() => onDeleteSession(activeSessionId)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  {t("ai.session.delete")}
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}

        {/* Only the settings entry is kept: close with the AI button in the top toolbar (or Ctrl/Cmd+L) —
            a second close button inside the drawer would duplicate the entry point and eat into the title area */}
        <div className="ml-auto flex shrink-0 items-center gap-0.5">
          <Button
            size="icon-sm"
            variant="ghost"
            title={t("ai.settings.title")}
            onClick={onOpenSettings}
          >
            <Settings2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {chips.length > 0 && (
        <div className="flex flex-wrap items-center gap-1 px-3 pb-2">
          {chips.map((chip) => (
            <span
              key={chip.key}
              className="flex max-w-full items-center gap-1 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground"
            >
              <span className="truncate">{chip.label}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
