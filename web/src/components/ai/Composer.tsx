// AI input area: reference chips + an auto-sizing input + a bottom bar (+ references | mode | model | usage | send).
//
// The layout follows mainstream agent tools: the left of the bottom bar is "how it works" (reference entry, work mode)
// and the right is "what it runs on + how much room is left + execute" (model, context usage, round send button).
// Typing `@` in the input searches for and pins context directly (collections/requests/scenarios/files).
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowUp, Square, Wand2 } from "lucide-react";
import type { AiMode } from "@/data/aiTypes";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { nextAiMode } from "@/lib/ai/constants";
import {
  buildReferenceCandidates,
  filterReferenceCandidates,
  type ReferenceCandidate,
} from "@/lib/ai/references";
import { useAppStore } from "@/store/useStore";
import { ModeSelector } from "./ModeSelector";
import { ModelPicker } from "./ModelPicker";
import { MentionMenu } from "./MentionMenu";
import { ReferenceAddButton, ReferenceChips } from "./ReferencePicker";

/**
 * Maximum input height (px): beyond it the input scrolls internally.
 *
 * 260 rather than a percentage: the drawer width is draggable but the height is bound to the window, so fixed pixels keep
 * "input + toolbar + message area" leaving room for the body even on a 768p screen.
 */
const MAX_TEXTAREA_HEIGHT = 260;

export interface ComposerProps {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  onStop: () => void;
  /** Whether generation is running (the primary button becomes "Stop") */
  streaming: boolean;
  /** Whether sending is disabled (no credentials configured, etc.) */
  disabled?: boolean;
  /** Work mode (Ask / Agent / Plan) */
  mode: AiMode;
  onModeChange: (mode: AiMode) => void;
}

/** Parse an `@` mention from the text before the cursor: returns the start index and the query term. */
function parseMention(
  text: string,
  cursor: number,
): { start: number; query: string } | null {
  const head = text.slice(0, cursor);
  const at = head.lastIndexOf("@");
  if (at === -1) return null;
  // Only an @ "at line start or after whitespace" counts: an @ in an email or code must not trigger
  const before = at === 0 ? "" : head[at - 1];
  if (before && !/\s/.test(before)) return null;
  const query = head.slice(at + 1);
  // A space in the query term means the user has moved on to the next segment, so close
  if (/\s/.test(query)) return null;
  return { start: at, query };
}

export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  streaming,
  disabled,
  mode,
  onModeChange,
}: ComposerProps) {
  const { t } = useT();
  const ref = useRef<HTMLTextAreaElement>(null);
  const canSend = value.trim().length > 0 && !disabled && !streaming;

  const addReference = useAppStore((s) => s.aiAddReference);
  const attachFile = useAppStore((s) => s.aiAttachDefinitionFile);
  const collections = useAppStore((s) => s.collections);
  const requests = useAppStore((s) => s.requests);
  const scenarios = useAppStore((s) => s.scenarios);

  const [mention, setMention] = useState<{
    start: number;
    query: string;
  } | null>(null);
  const [highlight, setHighlight] = useState(0);

  const candidates = useMemo(
    () => buildReferenceCandidates({ collections, requests, scenarios }),
    [collections, requests, scenarios],
  );
  const matches = useMemo(
    () => filterReferenceCandidates(candidates, mention?.query ?? ""),
    [candidates, mention?.query],
  );

  // Grows with the content (scrolling internally at the cap): long descriptions and multi-paragraph scripts are the norm, and a fixed 3 rows would force squinting
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`;
  }, [value]);

  /** Remove the `@query` text (leaving the cursor in place). */
  const stripMention = (m: { start: number; query: string }) => {
    const cursor = m.start + 1 + m.query.length;
    const next = value.slice(0, m.start) + value.slice(cursor);
    onChange(next);
    // Restore focus and the cursor after changing the value, so typing can continue
    requestAnimationFrame(() => {
      const el = ref.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(m.start, m.start);
    });
  };

  const handleChange = (next: string, cursor: number) => {
    onChange(next);
    const parsed = parseMention(next, cursor);
    setMention(parsed);
    if (!parsed) return;
    setHighlight(0);
  };

  const pickCandidate = (item: ReferenceCandidate) => {
    if (!mention) return;
    addReference({
      kind: item.kind,
      id: item.id,
      name: item.name,
      detail: item.detail,
    });
    stripMention(mention);
    setMention(null);
  };

  const pickFile = () => {
    if (!mention) return;
    void attachFile();
    stripMention(mention);
    setMention(null);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Shift+Tab cycles the mode (muscle memory from mainstream agent tools)
    if (e.key === "Tab" && e.shiftKey) {
      e.preventDefault();
      onModeChange(nextAiMode(mode));
      return;
    }
    // While the mention menu is open the arrow keys/Enter go to the menu first, so "selecting" never sends the message by accident
    if (mention) {
      const total = matches.length + 1; // +1 = the trailing "Pick from file..."
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setHighlight((h) => (h + 1) % total);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setHighlight((h) => (h - 1 + total) % total);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setMention(null);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        if (highlight >= matches.length) {
          pickFile();
        } else if (matches[highlight]) {
          pickCandidate(matches[highlight]);
        }
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (canSend) onSend();
    }
  };

  return (
    <div className="relative shrink-0 border-t border-border px-3 py-2">
      {mention && (
        <MentionMenu
          items={matches}
          highlight={highlight}
          footer={{ label: t("ai.reference.filePick"), onPick: pickFile }}
          onHover={setHighlight}
          onPick={pickCandidate}
        />
      )}

      <ReferenceChips />

      <textarea
        ref={ref}
        value={value}
        onChange={(e) => handleChange(e.target.value, e.target.selectionStart)}
        onKeyDown={onKeyDown}
        onClick={() => setMention(null)}
        onBlur={() => setMention(null)}
        rows={5}
        placeholder={t("ai.inputPlaceholder")}
        style={{ maxHeight: MAX_TEXTAREA_HEIGHT }}
        className="w-full resize-none overflow-y-auto rounded-md border border-input bg-transparent px-2.5 py-1.5 text-xs leading-relaxed outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
      />

      <div className="mt-2 flex items-center gap-1.5">
        <ReferenceAddButton />
        <ModeSelector mode={mode} onChange={onModeChange} />
        <span className="ml-auto" />
        <ModelPicker />
        {streaming ? (
          <Button
            size="icon-sm"
            variant="outline"
            className="ml-0.5 rounded-full"
            onClick={onStop}
            title={t("ai.stop")}
          >
            <Square className="h-3.5 w-3.5" />
          </Button>
        ) : (
          <Button
            size="icon-sm"
            className="ml-0.5 rounded-full"
            onClick={onSend}
            disabled={!canSend}
            title={t("ai.enterHint")}
          >
            <ArrowUp className="h-4 w-4" />
          </Button>
        )}
      </div>

      {disabled && (
        <p className="mt-1.5 flex items-center gap-1 text-xs text-warning">
          <Wand2 className="h-3.5 w-3.5" />
          {t("ai.needCredential")}
        </p>
      )}
    </div>
  );
}
