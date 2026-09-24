// Single action card: drag handle + type badge + name + summary + enable toggle + more actions + expandable editor.
import {
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Code2,
  Copy,
  Database,
  GripVertical,
  Trash2,
  XCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { summarizeAction } from "@/lib/requestActions";
import type { DataSourceOption } from "@/lib/dbQuery";
import type {
  ActionLog as ActionLogType,
  DbActionItem,
  RequestAction,
  ScriptActionItem,
} from "@/data/types";
import { ScriptEditor } from "../ScriptEditor";
import { ScriptExamplesPopover } from "../ScriptExamplesPopover";
import { DbActionForm } from "./DbActionForm";

export function ActionCard({
  action,
  index,
  expanded,
  result,
  dataSources,
  variables,
  scriptKind,
  stage,
  draggable: canDrag,
  canMoveUp,
  canMoveDown,
  onReplace,
  onToggle,
  onToggleEnabled,
  onDelete,
  onDuplicate,
  onMoveUp,
  onMoveDown,
  onHandleDown,
  onHandleMove,
  onHandleUp,
}: {
  /** Script / DB action card; the built-in interpolation node is rendered by InterpolateNode, not here */
  action: ScriptActionItem | DbActionItem;
  index: number;
  expanded: boolean;
  result?: ActionLogType;
  dataSources: DataSourceOption[];
  variables?: string[];
  scriptKind: "pre" | "post";
  /** Position relative to the built-in interpolation node (passed only for the pre list): `before` = pre-interpolation, `after` = post-interpolation */
  stage?: "before" | "after";
  /** Whether drag-reordering is allowed (enabled when the list has more than one item) */
  draggable: boolean;
  /** Move up / down (an accessible alternative to drag; the corresponding button is disabled at the ends) */
  canMoveUp: boolean;
  canMoveDown: boolean;
  onReplace: (next: RequestAction) => void;
  /** Expand / collapse the editor */
  onToggle: () => void;
  /** Enable / disable this action (independent of expansion) */
  onToggleEnabled: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  /** Pointer events for the drag handle (reordering is handled centrally by the list) */
  onHandleDown: (e: React.PointerEvent) => void;
  onHandleMove: (e: React.PointerEvent) => void;
  onHandleUp: () => void;
}) {
  const { t } = useT();
  const isScript = action.kind === "script";
  const dsName = (id: string) =>
    dataSources.find((d) => d.id === id)?.name || id;
  const displayName = action.name?.trim() || t("action.unnamed");
  const canDragAvailable = canDrag || canMoveUp || canMoveDown;

  return (
    <div
      data-action-card
      className={cn(
        "group overflow-hidden rounded-lg border bg-card transition-all duration-150",
        expanded
          ? "border-fuchsia-500/40 shadow-lg shadow-fuchsia-500/5"
          : "border-border",
        "hover:border-border/80 hover:shadow-md hover:shadow-black/20",
        !action.enabled && "opacity-60",
      )}
    >
      {/* Header */}
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
              "shrink-0 gap-1 text-[10px]",
              isScript
                ? "border-fuchsia-500/40 text-fuchsia-400"
                : "border-sky-500/40 text-sky-400",
            )}
          >
            {isScript ? (
              <Code2 className="h-3 w-3" />
            ) : (
              <Database className="h-3 w-3" />
            )}
            {isScript ? t("action.kindScript") : t("action.kindDb")}
          </Badge>
          {stage && (
            <span
              className={cn(
                "shrink-0 rounded px-1 py-0.5 text-[10px]",
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
          <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
            #{index + 1}
          </span>
          <span className="truncate text-xs font-medium">{displayName}</span>
        </button>

        <ResultBadge result={result} />

        {/* Accessible alternative to dragging (a second path besides drag) */}
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

      {/* Collapsed summary */}
      {!expanded && (
        <button
          type="button"
          onClick={onToggle}
          className="block w-full truncate px-9 pb-1.5 text-left font-mono text-[11px] text-muted-foreground hover:text-foreground/80"
        >
          {summarizeAction(action, dsName)}
        </button>
      )}

      {/* Expanded editor */}
      {expanded && (
        <div className="space-y-2.5 border-t border-border/60 bg-muted/10 px-2.5 py-2.5">
          <Input
            className="h-7 text-xs"
            placeholder={t("action.namePlaceholder")}
            value={action.name ?? ""}
            onChange={(e) => onReplace({ ...action, name: e.target.value })}
          />
          {isScript ? (
            <div className="space-y-1.5">
              <div className="flex justify-end">
                <ScriptExamplesPopover
                  kind={scriptKind}
                  onAppend={(code) =>
                    onReplace({
                      ...action,
                      code: action.code.trim()
                        ? `${action.code.replace(/\s*$/, "")}\n\n${code}`
                        : code,
                    })
                  }
                />
              </div>
              <ScriptEditor
                value={action.code}
                onChange={(code) => onReplace({ ...action, code })}
                kind={scriptKind}
                variables={variables}
                placeholder={
                  scriptKind === "pre"
                    ? (t("script.prePlaceholder") as string)
                    : (t("script.postPlaceholder") as string)
                }
              />
            </div>
          ) : (
            <DbActionForm
              value={action}
              onChange={(patch) => onReplace({ ...action, ...patch })}
              dataSources={dataSources}
            />
          )}
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
        "flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px]",
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

/** Reusable for the list: an alias for the action's result-log type (avoids repeating `from "@/data/types"` everywhere) */
export type { ActionLogType };
