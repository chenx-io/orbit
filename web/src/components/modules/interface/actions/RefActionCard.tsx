// Script-library reference card: read-only display of the library item's current content + a source badge; shows a broken state and repair entry when the library item is missing.
//
// Clear division of labor with ActionCard (editable script / DB actions): a reference's **content is not editable in place**
// — to change content, edit it in the script library (all references sync), or "convert to a copy" to detach from syncing.
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  BookOpen,
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  Copy,
  GripVertical,
  Trash2,
  Unlink,
  XCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { summarizeAction } from "@/lib/requestActions";
import { LibraryPickerPanel } from "./LibraryPicker";
import type {
  ActionLog as ActionLogType,
  ActionTemplate,
  RefActionItem,
} from "@/data/types";

export function RefActionCard({
  action,
  index,
  expanded,
  template,
  result,
  stage,
  draggable: canDrag,
  canMoveUp,
  canMoveDown,
  onToggle,
  onToggleEnabled,
  onDelete,
  onDuplicate,
  onMoveUp,
  onMoveDown,
  onHandleDown,
  onHandleMove,
  onHandleUp,
  onDetach,
  templates,
  onPickTemplate,
  onOpenLibrary,
}: {
  action: RefActionItem;
  index: number;
  expanded: boolean;
  /** The matched library item; `undefined` = dangling reference (library item deleted) */
  template?: ActionTemplate;
  result?: ActionLogType;
  /** Position relative to the built-in interpolation node (passed only for the pre list) */
  stage?: "before" | "after";
  draggable: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onToggle: () => void;
  onToggleEnabled: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onHandleDown: (e: React.PointerEvent) => void;
  onHandleMove: (e: React.PointerEvent) => void;
  onHandleUp: () => void;
  /** Convert to a copy: copy the library item's current content into a standalone action (list position unchanged) */
  onDetach: () => void;
  /** Script library table (used by the broken reference's "re-select" panel) */
  templates: ActionTemplate[];
  /** Re-select a library item for a broken reference */
  onPickTemplate: (tpl: ActionTemplate) => void;
  /** Jump to the script library management page */
  onOpenLibrary: () => void;
}) {
  const { t } = useT();
  const broken = !template;
  // Display name: reference alias first, then library item name, falling back to the library id (still identifiable in the broken state)
  const displayName =
    action.name?.trim() ||
    template?.name ||
    `${t("action.libBroken")} · ${action.libraryId}`;
  const canDragAvailable = canDrag || canMoveUp || canMoveDown;

  return (
    <div
      data-action-card
      className={cn(
        "group overflow-hidden rounded-lg border bg-card transition-all duration-150",
        broken
          ? "border-destructive/60 bg-destructive/5"
          : expanded
            ? "border-violet-500/40 shadow-lg shadow-violet-500/5"
            : "border-border",
        !broken &&
          "hover:border-border/80 hover:shadow-md hover:shadow-black/20",
        !action.enabled && "opacity-60",
      )}
    >
      <div className="flex items-center gap-1.5 px-2 py-1.5">
        <button
          type="button"
          onPointerDown={canDrag ? onHandleDown : undefined}
          onPointerMove={canDrag ? onHandleMove : undefined}
          onPointerUp={canDrag ? onHandleUp : undefined}
          onPointerCancel={canDrag ? onHandleUp : undefined}
          title={canDrag ? t("action.dragHint") : undefined}
          aria-label={t("action.dragHint")}
          className={cn(
            "touch-none rounded p-0.5 text-muted-foreground/60 transition-colors hover:bg-accent/40 hover:text-foreground",
            canDrag
              ? "cursor-grab active:cursor-grabbing"
              : "cursor-default opacity-40",
          )}
        >
          <GripVertical className="h-4 w-4" />
        </button>

        <button
          type="button"
          onClick={onToggle}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          )}
          <Badge
            variant="outline"
            className={cn(
              "shrink-0 gap-1 text-xs",
              broken
                ? "border-destructive/50 text-destructive"
                : "border-violet-500/40 text-violet-300",
            )}
          >
            {broken ? (
              <AlertTriangle className="h-3 w-3" />
            ) : (
              <BookOpen className="h-3 w-3" />
            )}
            {t("action.libBadge")}
          </Badge>
          {stage && (
            <span
              className={cn(
                "shrink-0 rounded-full px-1.5 py-0.5 text-xs",
                stage === "before"
                  ? "bg-amber-500/10 text-amber-400"
                  : "bg-emerald-500/10 text-emerald-400",
              )}
              title={
                stage === "before"
                  ? t("action.stageBeforeHint")
                  : t("action.stageAfterHint")
              }
            >
              {stage === "before"
                ? t("action.stageBefore")
                : t("action.stageAfter")}
            </span>
          )}
          <span className="shrink-0 font-mono text-xs text-muted-foreground">
            #{index + 1}
          </span>
          <span className="truncate text-xs font-medium">{displayName}</span>
        </button>

        <ResultBadge result={result} />

        {canDragAvailable && (
          <div className="flex shrink-0 items-center">
            <Button
              variant="ghost"
              size="icon"
              className="h-5 w-5 text-muted-foreground/50 hover:text-foreground"
              title={t("action.moveUp")}
              disabled={!canMoveUp}
              onClick={onMoveUp}
            >
              <ArrowUp className="h-3 w-3" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-5 w-5 text-muted-foreground/50 hover:text-foreground"
              title={t("action.moveDown")}
              disabled={!canMoveDown}
              onClick={onMoveDown}
            >
              <ArrowDown className="h-3 w-3" />
            </Button>
          </div>
        )}

        <Switch
          checked={action.enabled}
          onCheckedChange={onToggleEnabled}
          title={t("action.enable")}
        />

        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-muted-foreground hover:text-foreground"
          title={t("action.detach")}
          disabled={broken}
          onClick={onDetach}
        >
          <Unlink className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-muted-foreground hover:text-foreground"
          title={t("action.duplicate")}
          onClick={onDuplicate}
        >
          <Copy className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 text-muted-foreground hover:text-rose-400"
          title={t("action.delete")}
          onClick={onDelete}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>

      {broken ? (
        <div className="flex flex-wrap items-center gap-2 border-t border-destructive/30 px-2 py-1.5">
          <span className="flex-1 text-xs text-destructive">
            {t("action.libBrokenHint").replace("{0}", action.libraryId)}
          </span>
          <Popover>
            <PopoverTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                className="h-6 border-destructive/50 px-2 text-xs text-destructive hover:text-destructive"
              >
                {t("action.repick")}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-72 p-1">
              <LibraryPickerPanel
                templates={templates}
                onPick={onPickTemplate}
                onOpenLibrary={onOpenLibrary}
              />
            </PopoverContent>
          </Popover>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={onOpenLibrary}
          >
            {t("action.openLibrary")}
          </Button>
        </div>
      ) : (
        !expanded && (
          <button
            type="button"
            onClick={onToggle}
            className="block w-full truncate px-9 pb-1.5 text-left font-mono text-xs text-muted-foreground hover:text-foreground/80"
          >
            {summarizeAction(template.action)}
          </button>
        )
      )}

      {expanded && !broken && (
        <div className="space-y-2 border-t border-border/60 bg-muted/10 px-2.5 py-2.5">
          <div className="text-xs text-muted-foreground">
            {t("action.libLiveHint")}
          </div>
          {template.action.kind === "script" ? (
            <pre className="max-h-48 overflow-auto rounded-md border border-border bg-muted/20 p-2 font-mono text-xs">
              {template.action.code || t("action.libEmptyCode")}
            </pre>
          ) : (
            <div className="rounded-md border border-border bg-muted/20 p-2 font-mono text-xs">
              {summarizeAction(template.action)}
            </div>
          )}
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={onDetach}
            >
              <Unlink className="h-3 w-3" /> {t("action.detach")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={onOpenLibrary}
            >
              <BookOpen className="h-3 w-3" /> {t("action.openLibrary")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Execution-result badge (success / failure / not run) */
function ResultBadge({ result }: { result?: ActionLogType }) {
  const { t } = useT();
  if (!result) return null;
  return (
    <span
      className={cn(
        "flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-xs",
        result.ok
          ? "bg-emerald-500/15 text-emerald-400"
          : "bg-rose-500/15 text-rose-400",
      )}
      title={result.detail}
    >
      {result.ok ? (
        <CheckCircle2 className="h-3 w-3" />
      ) : (
        <XCircle className="h-3 w-3" />
      )}
      {result.ok ? t("action.resultOk") : t("action.resultFail")}
      {result.elapsedMs > 0 && (
        <span className="opacity-70">{result.elapsedMs}ms</span>
      )}
    </span>
  );
}
