// Pre/post action list editor: add scripts / DB operations, drag to reorder, toggle per-item enable and expand for editing.
//
// Order is execution order. The pre list (`allowInterpolate`) contains one **built-in, unmodifiable "interpolation" node**:
// before the node = pre-interpolation (can produce variables for this interpolation to consume), after the node = post-interpolation (rewriting yields the final bytes).
// When empty, the parent falls back to the old single-script field (normalization lives in lib/requestActions).
//
// Sorting implementation note: HTML5 Drag & Drop is not used — Tauri's WebView enables file drag-drop by default
// (dragDropEnabled), which intercepts in-page `dragover`/`drop` and prevents drops from landing (on desktop this shows as
// "drag starts but nothing happens on release"). Here we use Pointer events + pointer capture for sorting,
// which behaves consistently across desktop / browser / touch and adds no extra dependency.
import { useRef, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import {
  enabledActions,
  isActionEnabled,
  newActionId,
  newDbAction,
  newRefAction,
  newScriptAction,
  resolveActionRefs,
} from "@/lib/requestActions";
import type { DataSourceOption } from "@/lib/dbQuery";
import type { ActionLog, ActionTemplate, RequestAction } from "@/data/types";
import { ActionCard } from "./ActionCard";
import { ActionAddMenu } from "./ActionAddMenu";
import { RefActionCard } from "./RefActionCard";
import { InterpolateNode } from "./InterpolateNode";

export function RequestActionsEditor({
  value,
  onChange,
  kind,
  variables,
  allowDb = true,
  allowInterpolate = false,
  results,
  dataSources,
  templates,
  onOpenLibrary,
  headerLeading,
  headerTrailing,
}: {
  value: RequestAction[];
  onChange: (next: RequestAction[]) => void;
  kind: "pre" | "post";
  /** Variable names used for script completion */
  variables?: string[];
  /** Whether DB actions are allowed (not yet supported for long-connection session protocols) */
  allowDb?: boolean;
  /** Whether to render the built-in interpolation node (pre list only; node is not editable / deletable / disableable / draggable) */
  allowInterpolate?: boolean;
  /** Results of this run's actions (in execution order; used to show a status badge on each card) */
  results?: ActionLog[];
  dataSources: DataSourceOption[];
  /** Script library table: renders reference cards and the "reference from library" entry (absent = no reuse capability) */
  templates?: ActionTemplate[];
  /** Jump to the script library management page */
  onOpenLibrary?: () => void;
  /** Top toolbar: button placed to the **left** of "add action" (e.g. "syntax reference") */
  headerLeading?: ReactNode;
  /** Top toolbar: button placed to the **right** of "add action" (e.g. execution-timing info ⓘ) */
  headerTrailing?: ReactNode;
}) {
  const { t } = useT();
  const lib = templates ?? [];
  const openLibrary = onOpenLibrary ?? (() => {});
  const listRef = useRef<HTMLDivElement>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  /** Drag-reorder state: `from` = start index, `to` = current drop index (both list subscripts) */
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null);

  // User actions (excluding the built-in interpolation node): counts and the "empty list" hint both derive from this
  const userActions = value.filter((a) => a.kind !== "interpolate");
  const active = enabledActions(userActions).length;
  const canDrag = value.length > 1;
  const anchorIndex = value.findIndex((a) => a.kind === "interpolate");

  /**
   * Position of an action relative to the built-in interpolation node (shown only for the pre list):
   * with no anchor, everything is treated as "post-interpolation" — the engine prepends the anchor to the front of the list, keeping semantics consistent.
   */
  const stageOf = (index: number): "before" | "after" | undefined => {
    if (!allowInterpolate) return undefined;
    return anchorIndex === -1 || index > anchorIndex ? "after" : "before";
  };

  /**
   * Result-log index: the backend only returns logs for **enabled items** (including the built-in interpolation node entry); disabled items are excluded,
   * so we cannot index `results[i]` directly by list position.
   */
  const resultIndex = (index: number): number | undefined => {
    let cursor = 0;
    for (let i = 0; i < value.length; i++) {
      if (!isActionEnabled(value[i])) continue;
      if (i === index) return cursor;
      cursor += 1;
    }
    return undefined;
  };

  const move = (from: number, to: number) => {
    if (from === to || from < 0 || to < 0 || to >= value.length) return;
    // The built-in interpolation node cannot be a drag source (its card also has no drag handle)
    if (value[from]?.kind === "interpolate") return;
    const next = [...value];
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    onChange(next);
  };

  /** Enable / disable a single action (independent of expand state; the built-in interpolation node cannot be disabled) */
  const toggleEnabled = (i: number) => {
    if (value[i]?.kind === "interpolate") return;
    onChange(
      value.map((a, j) =>
        j === i && a.kind !== "interpolate" ? { ...a, enabled: !a.enabled } : a,
      ),
    );
  };

  // ── Pointer-based reordering ──
  /** Pick the closest list item by pointer Y coordinate (nearest vertical center wins) */
  const hitIndex = (clientY: number): number => {
    const items =
      listRef.current?.querySelectorAll<HTMLElement>("[data-action-item]");
    if (!items || items.length === 0) return 0;
    let best = Number.POSITIVE_INFINITY;
    let index = 0;
    items.forEach((el, i) => {
      const r = el.getBoundingClientRect();
      const d = Math.abs(clientY - (r.top + r.bottom) / 2);
      if (d < best) {
        best = d;
        index = i;
      }
    });
    return index;
  };

  const onHandleDown = (e: React.PointerEvent, i: number) => {
    if (!canDrag) return;
    e.preventDefault();
    // Pointer capture: subsequent move/up events keep dispatching to this handle even if the pointer leaves it
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setDrag({ from: i, to: i });
  };

  const onHandleMove = (e: React.PointerEvent) => {
    if (!drag) return;
    const to = hitIndex(e.clientY);
    if (to !== drag.to) setDrag({ ...drag, to });
  };

  const onHandleUp = () => {
    if (drag) {
      if (drag.from !== drag.to) move(drag.from, drag.to);
      setDrag(null);
    }
  };

  const addScript = () => {
    const item = newScriptAction();
    setExpandedId(item.id);
    onChange([...value, item]);
  };

  const addDb = () => {
    const item = newDbAction(dataSources[0]?.id ?? "");
    setExpandedId(item.id);
    onChange([...value, item]);
  };

  /** Insert a reference from the script library: appended to the end; the user reorders via drag afterward */
  const insertTemplate = (tpl: ActionTemplate) => {
    const item = newRefAction(tpl.id);
    setExpandedId(item.id);
    onChange([...value, item]);
  };

  /**
   * Convert to a copy: copy the library item's current content into a standalone action (**list position unchanged**), after which it is detached from the library.
   * If the library item was deleted (dangling reference) the content cannot be copied, so that action is disabled on the card.
   */
  const detach = (i: number) => {
    const src = value[i];
    if (src.kind !== "ref") return;
    const resolved = resolveActionRefs([src], lib)[0];
    if (!resolved || resolved.kind === "ref") return;
    onChange(value.map((a, j) => (j === i ? resolved : a)));
    setExpandedId(resolved.id);
  };

  /** Re-select a library item for a broken reference (keeps the original alias and enabled state) */
  const repick = (i: number, tpl: ActionTemplate) => {
    onChange(
      value.map((a, j) =>
        j === i && a.kind === "ref" ? { ...a, libraryId: tpl.id } : a,
      ),
    );
  };

  const duplicate = (i: number) => {
    const src = value[i];
    // The built-in interpolation node cannot be copied (unique in the whole list); ref items can be copied (same library item may be referenced in multiple places)
    if (src.kind === "interpolate") return;
    const copy: RequestAction =
      src.kind === "script" || src.kind === "ref"
        ? { ...src, id: newActionId() }
        : {
            ...src,
            id: newActionId(),
            retry: src.retry ? { ...src.retry } : undefined,
          };
    const next = [...value];
    next.splice(i + 1, 0, copy);
    setExpandedId(copy.id);
    onChange(next);
  };

  // One button covers three sources (new script / new DB query / reference from script library):
  // no need to pick a "drop position" when adding — just append to the end, then drag to reorder as needed.
  const addButtons = (
    <ActionAddMenu
      allowDb={allowDb}
      templates={lib}
      onAddScript={addScript}
      onAddDb={addDb}
      onPickTemplate={insertTemplate}
      onOpenLibrary={openLibrary}
    />
  );

  return (
    <div className="space-y-2">
      {/* Top action bar */}
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">
          {t("action.count")
            .replace("{0}", String(active))
            .replace("{1}", String(userActions.length))}
          {drag && (
            <span className="ml-2 text-fuchsia-400">
              {t("action.dragHint")} · #{drag.from + 1} → #{drag.to + 1}
            </span>
          )}
        </span>
        {/* Toolbar: action count on the left; on the right, external pre-entry (syntax reference) → add action → external post-entry (info ⓘ) */}
        <div className="flex items-center gap-1.5">
          {headerLeading}
          {addButtons}
          {headerTrailing}
        </div>
      </div>

      {!allowDb && (
        <div className="rounded-md border border-amber-500/30 bg-amber-500/5 px-2 py-1.5 text-[11px] text-amber-400">
          {t("action.dbUnsupported")}
        </div>
      )}

      {/* Action list */}
      {value.length === 0 ? (
        <div className="rounded-lg border border-dashed border-border px-3 py-8 text-center">
          <div className="text-xs text-muted-foreground">
            {t("action.empty")}
          </div>
          <div className="mt-1 text-[11px] text-muted-foreground/70">
            {t("action.emptyHint")}
          </div>
          <div className="mt-3 flex items-center justify-center gap-2">
            {addButtons}
          </div>
        </div>
      ) : (
        <div ref={listRef} className={cn("space-y-1.5", drag && "select-none")}>
          {value.map((action, i) => {
            const isSource = drag?.from === i;
            const isTarget = drag !== null && drag.to === i && !isSource;
            const ri = resultIndex(i);
            const result = ri !== undefined ? results?.[ri] : undefined;
            return (
              <div
                key={action.id}
                data-action-item
                className={cn(
                  "relative rounded-lg transition-all duration-150",
                  isSource && "opacity-60",
                  isTarget && "ring-1 ring-fuchsia-400/70",
                  drag && !isSource && !isTarget && "opacity-90",
                )}
              >
                {action.kind === "interpolate" ? (
                  <InterpolateNode result={result} />
                ) : action.kind === "ref" ? (
                  <RefActionCard
                    action={action}
                    index={i}
                    expanded={expandedId === action.id}
                    template={lib.find((x) => x.id === action.libraryId)}
                    result={result}
                    stage={stageOf(i)}
                    draggable={canDrag}
                    canMoveUp={i > 0}
                    canMoveDown={i < value.length - 1}
                    onToggle={() =>
                      setExpandedId(expandedId === action.id ? null : action.id)
                    }
                    onToggleEnabled={() => toggleEnabled(i)}
                    onDelete={() => onChange(value.filter((_, j) => j !== i))}
                    onDuplicate={() => duplicate(i)}
                    onMoveUp={() => move(i, i - 1)}
                    onMoveDown={() => move(i, i + 1)}
                    onHandleDown={(e) => onHandleDown(e, i)}
                    onHandleMove={onHandleMove}
                    onHandleUp={onHandleUp}
                    onDetach={() => detach(i)}
                    templates={lib}
                    onPickTemplate={(tpl) => repick(i, tpl)}
                    onOpenLibrary={openLibrary}
                  />
                ) : (
                  <ActionCard
                    action={action}
                    index={i}
                    expanded={expandedId === action.id}
                    result={result}
                    dataSources={dataSources}
                    variables={variables}
                    scriptKind={kind}
                    stage={stageOf(i)}
                    draggable={canDrag}
                    canMoveUp={i > 0}
                    canMoveDown={i < value.length - 1}
                    onReplace={(next) =>
                      onChange(value.map((x, j) => (j === i ? next : x)))
                    }
                    onToggle={() =>
                      setExpandedId(expandedId === action.id ? null : action.id)
                    }
                    onToggleEnabled={() => toggleEnabled(i)}
                    onDelete={() => onChange(value.filter((_, j) => j !== i))}
                    onDuplicate={() => duplicate(i)}
                    onMoveUp={() => move(i, i - 1)}
                    onMoveDown={() => move(i, i + 1)}
                    onHandleDown={(e) => onHandleDown(e, i)}
                    onHandleMove={onHandleMove}
                    onHandleUp={onHandleUp}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* When only the built-in interpolation node exists, show a hint (the pre list always has an anchor, so it never takes the empty-list branch above) */}
      {allowInterpolate && value.length > 0 && userActions.length === 0 && (
        <div className="rounded-lg border border-dashed border-border px-3 py-4">
          <div className="text-xs text-muted-foreground">
            {t("action.emptyInterpolate")}
          </div>
          <div className="mt-3 flex items-center justify-center gap-2">
            {addButtons}
          </div>
        </div>
      )}
    </div>
  );
}
