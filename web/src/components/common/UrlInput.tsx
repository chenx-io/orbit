import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { useEffectiveVars, useEnvVarNames } from "@/store/useStore";
import type { Locale } from "@/data/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

export interface UrlInputHandle {
  focus: () => void;
  insert: (text: string) => void;
}

interface Props {
  value: string;
  onChange: (v: string) => void;
  locale: Locale;
}

const TOKEN_RE =
  /\{\{\s*\$?(?:([\w.-]+)\.([\w.-]+)(?:\(([^)]*)\))?(?:\|([^}]+))?|\$?([\w.-]+))\s*\}\}/g;

interface Token {
  raw: string;
  varName?: string;
  dyn?: boolean;
}

function tokenize(input: string): Token[] {
  const out: Token[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  TOKEN_RE.lastIndex = 0;
  while ((m = TOKEN_RE.exec(input))) {
    if (m.index > last) out.push({ raw: input.slice(last, m.index) });
    if (m[5]) out.push({ raw: m[5], varName: m[5] });
    else
      out.push({ raw: `${m[1]}.${m[2]}${m[4] ? `|${m[4]}` : ""}`, dyn: true });
    last = TOKEN_RE.lastIndex;
  }
  if (last < input.length) out.push({ raw: input.slice(last) });
  return out;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Range of the `{{prefix` to replace (replaced by `{{variable}}` when a completion is confirmed) */
interface ReplaceRange {
  startContainer: Node;
  startOffset: number;
  endContainer: Node;
  endOffset: number;
}

/**
 * A contentEditable-based URL input:
 * - variables ({{base_url}} / {{$random.uuid}}) render as non-editable colored chips that always show the resolved value,
 *   so the caret misalignment of a "transparent overlay" approach cannot happen.
 * - the rest of the text is freely editable; on blur the whole thing is deserialized back into a {{var}} template and written to the store.
 * - typing {{ or a {{prefix pops up completions for active environment variables ({{variable}}), confirmed with Enter/click.
 */
export const UrlInput = forwardRef<UrlInputHandle, Props>(function UrlInput(
  { value, onChange, locale },
  ref,
) {
  const { t, format } = useT();
  const hostRef = useRef<HTMLDivElement>(null);
  const divRef = useRef<HTMLDivElement>(null);
  const vars = useEffectiveVars();
  const envVarNames = useEnvVarNames();
  const [focused, setFocused] = useState(false);

  // Variable completion state
  const [completions, setCompletions] = useState<string[] | null>(null);
  const [completionIndex, setCompletionIndex] = useState(0);
  const [completionPos, setCompletionPos] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const replaceRangeRef = useRef<ReplaceRange | null>(null);

  const resolve = (t: Token): { text: string; cls: string; title: string } => {
    if (t.dyn) {
      const ref = `{{$${t.raw}}}`;
      return {
        text: ref,
        cls: "bg-primary/10 text-primary",
        title: format("url.dynamicValue", t.raw),
      };
    }
    const v = vars[t.varName as string];
    if (v == null) {
      return {
        text: `{{${t.varName}}}`,
        cls: "bg-destructive/20 text-destructive",
        title: format("url.varUndefined", t.varName ?? ""),
      };
    }
    return {
      text: v,
      cls: "bg-primary/20 text-primary",
      title: `${t.varName} = ${v}`,
    };
  };

  const buildHTML = (): string => {
    const tokens = tokenize(value);
    return tokens
      .map((t) => {
        if (!t.varName && !t.dyn) return escapeHtml(t.raw);
        const r = resolve(t);
        const dataVar = t.dyn ? `$${t.raw}` : (t.varName as string);
        return `<span contenteditable="false" data-var="${escapeHtml(dataVar)}" class="rounded px-0.5 ${r.cls}" title="${escapeHtml(r.title)}">${escapeHtml(r.text)}</span>`;
      })
      .join("");
  };

  const serialize = (): string => {
    const el = divRef.current;
    if (!el) return value;
    let out = "";
    const walk = (node: Node) => {
      node.childNodes.forEach((c) => {
        if (c.nodeType === Node.TEXT_NODE) out += c.textContent ?? "";
        else if (c.nodeType === Node.ELEMENT_NODE) {
          const e = c as HTMLElement;
          if (e.dataset.var !== undefined) out += `{{${e.dataset.var}}}`;
          else walk(c);
        }
      });
    };
    walk(el);
    return out;
  };

  // Sync external changes into the DOM. Rebuilt only while the input has no DOM focus, so the caret is never disturbed.
  useEffect(() => {
    const el = divRef.current;
    if (!el || document.activeElement === el) return;
    const html = buildHTML();
    if (el.innerHTML !== html) el.innerHTML = html;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, vars, locale]);

  const getCaret = (): number => {
    const el = divRef.current;
    const sel = window.getSelection();
    if (!el || !sel || sel.rangeCount === 0) return 0;
    const range = sel.getRangeAt(0);
    if (!el.contains(range.startContainer)) return 0;
    let offset = 0;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let n: Node | null;
    while ((n = walker.nextNode())) {
      if (n === range.startContainer) {
        offset += range.startOffset;
        break;
      }
      offset += n.textContent?.length ?? 0;
    }
    return offset;
  };

  /** Compute and show variable completion candidates for the caret position (matching `{{prefix`; candidates are the active environment variables) */
  const updateCompletions = () => {
    const el = divRef.current;
    if (!el) return;
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0) {
      setCompletions(null);
      return;
    }
    const range = sel.getRangeAt(0);
    if (!range.collapsed || !el.contains(range.startContainer)) {
      setCompletions(null);
      return;
    }
    // Match inside text nodes only (a `{{` the user just typed is always in a text node)
    if (range.startContainer.nodeType !== Node.TEXT_NODE) {
      setCompletions(null);
      return;
    }
    const text = (range.startContainer.textContent ?? "").slice(
      0,
      range.startOffset,
    );
    const m = text.match(/\{\{([\w.-]*)$/);
    if (!m) {
      setCompletions(null);
      return;
    }
    const prefix = m[1];
    const candidates = envVarNames.filter((v) => v.startsWith(prefix));
    if (candidates.length === 0) {
      setCompletions(null);
      return;
    }
    replaceRangeRef.current = {
      startContainer: range.startContainer,
      startOffset: range.startOffset - m[0].length,
      endContainer: range.startContainer,
      endOffset: range.startOffset,
    };
    // Popup placement: next to the caret (relative to the outer container)
    const caretRect = range.getBoundingClientRect();
    const hostRect = hostRef.current?.getBoundingClientRect();
    setCompletionPos(
      hostRect
        ? {
            top: caretRect.bottom - hostRect.top + 2,
            left: caretRect.left - hostRect.left,
          }
        : { top: 4, left: 0 },
    );
    setCompletionIndex(0);
    setCompletions(candidates);
  };

  /** Confirm a completion: replace `{{prefix` with `{{variable}}` and serialize back */
  const applyCompletion = (name: string) => {
    const el = divRef.current;
    const rr = replaceRangeRef.current;
    if (!el || !rr) return;
    const range = document.createRange();
    range.setStart(rr.startContainer, rr.startOffset);
    range.setEnd(rr.endContainer, rr.endOffset);
    range.deleteContents();
    const node = document.createTextNode(`{{${name}}}`);
    range.insertNode(node);
    range.setStartAfter(node);
    range.collapse(true);
    const sel = window.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(range);
    setCompletions(null);
    replaceRangeRef.current = null;
    const serialized = serialize();
    if (serialized !== value) onChange(serialized);
    el.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (completions && completions.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setCompletionIndex((i) => (i + 1) % completions.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setCompletionIndex(
          (i) => (i - 1 + completions.length) % completions.length,
        );
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const pick = completions[completionIndex];
        if (pick) applyCompletion(pick);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setCompletions(null);
        return;
      }
    }
  };

  useImperativeHandle(ref, () => ({
    focus: () => divRef.current?.focus(),
    insert: (text: string) => {
      const el = divRef.current;
      if (!el) return;
      el.focus();
      const sel = window.getSelection();
      const range = document.createRange();
      if (
        sel &&
        sel.rangeCount > 0 &&
        el.contains(sel.getRangeAt(0).startContainer)
      ) {
        range.setStart(
          sel.getRangeAt(0).startContainer,
          sel.getRangeAt(0).startOffset,
        );
      } else {
        range.selectNodeContents(el);
      }
      range.collapse(false);
      const node = document.createTextNode(text);
      range.insertNode(node);
      range.setStartAfter(node);
      range.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(range);
      onChange(serialize());
      // Rebuild as chips while blurred
      requestAnimationFrame(() => {
        if (!focused) {
          const html = buildHTML();
          if (el.innerHTML !== html) el.innerHTML = html;
        }
      });
    },
  }));

  return (
    <div ref={hostRef} className="relative min-w-0 flex-1">
      <div
        ref={divRef}
        contentEditable
        suppressContentEditableWarning
        spellCheck={false}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          setCompletions(null);
          const serialized = serialize();
          if (serialized !== value) onChange(serialized);
          else {
            // The value is unchanged but the DOM may differ due to browser behavior, so rebuild from the raw value
            const html = buildHTML();
            if (divRef.current && divRef.current.innerHTML !== html)
              divRef.current.innerHTML = html;
          }
        }}
        onInput={() => {
          const serialized = serialize();
          if (serialized !== value) onChange(serialized);
          updateCompletions();
        }}
        onKeyDown={handleKeyDown}
        onKeyUp={(e) => {
          getCaret();
          // Arrow keys/Enter/Escape are handled by keydown; keyup does not refresh candidates,
          // otherwise updateCompletions would reset the highlighted item back to the first
          if (["ArrowDown", "ArrowUp", "Enter", "Escape"].includes(e.key))
            return;
          updateCompletions();
        }}
        onMouseUp={() => {
          getCaret();
          updateCompletions();
        }}
        className={cn(
          "url-input h-7 min-w-50 w-full overflow-x-auto whitespace-nowrap rounded bg-transparent px-1 font-mono text-xs leading-7 outline-none",
          "focus:ring-1 focus:ring-primary/40",
        )}
        data-placeholder={t("url.placeholder")}
      />
      {completions && completions.length > 0 && (
        <div
          className="absolute z-50 max-h-48 min-w-40 overflow-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md"
          style={completionPos ?? { top: 4, left: 0 }}
          onMouseDown={(e) => e.preventDefault()}
        >
          {completions.map((c, i) => (
            <div
              key={c}
              onClick={() => applyCompletion(c)}
              onMouseEnter={() => setCompletionIndex(i)}
              className={cn(
                "flex cursor-pointer items-center justify-between gap-4 rounded px-2 py-1 text-xs",
                i === completionIndex && "bg-accent text-accent-foreground",
              )}
            >
              <span className="font-mono">{c}</span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {t("url.variables")}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
