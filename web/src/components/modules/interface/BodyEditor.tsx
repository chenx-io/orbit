// Request-body editor: a CodeMirror 6 wrapper.
// - json/xml syntax highlighting (light/dark adaptive); raw plain text.
// - json/xml live syntax validation: wavy underline on error lines + a gutter marker, with the error message on hover
//   (lint mechanism, does not obscure the content being edited).
// - Exposes insertAtCursor (so DynamicValuePicker can insert {{var}} at the cursor).
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  drawSelection,
  placeholder,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import {
  linter,
  lintGutter,
  forceLinting,
  type Diagnostic,
} from "@codemirror/lint";
import {
  closeSearchPanel,
  findNext,
  findPrevious,
  openSearchPanel,
} from "@codemirror/search";
import { validateBody, type BodyValidateMode } from "@/lib/bodyValidate";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import {
  lightExtensions,
  darkExtensions,
  codeEditorTheme,
  jsoncCommentHighlight,
  openReplaceMode,
  searchExtensions,
} from "./codeEditorShared";

export interface BodyEditorHandle {
  /** Insert text at the cursor (replacing the current selection) and focus */
  insertAtCursor: (text: string) => void;
  /** Open the editor's find panel (for external buttons, equivalent to Ctrl/Cmd+F) */
  openFind?: () => void;
  /** Open the editor's replace panel (for external buttons, equivalent to Ctrl/Cmd+R) */
  openReplace?: () => void;
}

interface BodyEditorProps {
  value: string;
  onChange: (value: string) => void;
  mode: BodyValidateMode;
  placeholder?: string;
  /** Environment variable names (global + environment + secrets), used for {{var}} completion */
  variables?: string[];
  /** Fill the parent container height (parent must be a flexible, stretchable flex container): ignore the default min/maxHeight */
  fill?: boolean;
}

export const BodyEditor = forwardRef<BodyEditorHandle, BodyEditorProps>(
  function BodyEditor(
    { value, onChange, mode, placeholder: ph, variables, fill },
    ref,
  ) {
    const hostRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const { t } = useT();
    const themeCompartmentRef = useRef<Compartment | null>(null);
    const languageCompartmentRef = useRef<Compartment | null>(null);

    const onChangeRef = useRef(onChange);
    onChangeRef.current = onChange;
    const valueRef = useRef(value);
    valueRef.current = value;
    const modeRef = useRef(mode);
    modeRef.current = mode;
    const variablesRef = useRef(variables);
    variablesRef.current = variables;

    const [isDark, setIsDark] = useState(
      () =>
        typeof document !== "undefined" &&
        document.documentElement.classList.contains("dark"),
    );
    const isDarkRef = useRef(isDark);
    isDarkRef.current = isDark;

    // Watch html.dark class toggling
    useEffect(() => {
      const obs = new MutationObserver(() => {
        setIsDark(document.documentElement.classList.contains("dark"));
      });
      obs.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["class"],
      });
      return () => obs.disconnect();
    }, []);

    // Lint data source: turn validation results into CodeMirror diagnostics (wavy underline at the error position + hover message)
    const lintSource = (view: EditorView): Diagnostic[] => {
      const text = view.state.doc.toString();
      const result = validateBody(modeRef.current, text);
      if (!result) return [];
      let from = 0;
      if (result.line > 0) {
        const lineCount = view.state.doc.lines;
        const line = Math.min(result.line, lineCount);
        const lineInfo = view.state.doc.line(line);
        const col = Math.max(0, result.column - 1);
        from = Math.min(lineInfo.from + col, lineInfo.to);
      }
      const to = Math.min(from + 1, view.state.doc.length);
      return [
        {
          from,
          to: to > from ? to : from,
          severity: "error",
          message: result.message,
        },
      ];
    };

    // Autocompletion source: after typing {{ or {{prefix, complete environment variables.
    // - `from` covers only the typed prefix (excluding {{), otherwise CodeMirror would use the [from, pos) text
    //   (i.e. {{) to filter candidates, causing all variables to be filtered out and the panel to hide.
    // - apply uses a custom function that finds {{ backward and replaces it as a whole, avoiding duplicated {{.
    const completionSource = useCallback(
      (context: CompletionContext): CompletionResult | null => {
        const vars = variablesRef.current;
        if (!vars || vars.length === 0) return null;
        const line = context.state.doc.lineAt(context.pos);
        const before = line.text.slice(0, context.pos - line.from);
        const m = before.match(/\{\{\s*([\w.-]*)$/);
        if (!m) return null;
        const prefix = m[1];
        const options = Array.from(new Set(vars))
          .filter((v) => v.startsWith(prefix))
          .map((v) => ({
            label: v,
            apply: (
              view: EditorView,
              completion: Completion,
              from: number,
              to: number,
            ) => {
              // From the completion start, find the {{ start position backward and replace it as a whole with {{variable}}
              const lineInfo = view.state.doc.lineAt(from);
              const beforeText = view.state.doc.sliceString(
                lineInfo.from,
                from,
              );
              const m2 = beforeText.match(/\{\{\s*([\w.-]*)$/);
              const start = m2 ? from - m2[0].length : from;
              const insert = `{{${completion.label}}}`;
              view.dispatch({
                changes: { from: start, to, insert },
                selection: { anchor: start + insert.length },
              });
            },
            detail: t("body.varCompletionDetail"),
            type: "variable" as const,
          }));
        if (options.length === 0) return null;
        return { from: context.pos - prefix.length, options };
      },
      [t],
    );

    // Create the editor once
    useEffect(() => {
      const host = hostRef.current;
      if (!host) return;

      const themeCompartment = new Compartment();
      themeCompartmentRef.current = themeCompartment;
      const languageCompartment = new Compartment();
      languageCompartmentRef.current = languageCompartment;

      const view = new EditorView({
        state: EditorState.create({
          doc: valueRef.current,
          extensions: [
            lineNumbers(),
            history(),
            drawSelection(),
            highlightActiveLine(),
            // Find/replace (@codemirror/search): top-right floating toolbar, Mod-f to find, Mod-r to replace
            searchExtensions(),
            // Exclude { from auto-closing: avoid closeBrackets inserting a stray } when typing {{var}}.
            // Note closeBrackets() takes no args; brackets are supplied via languageData.
            closeBrackets(),
            EditorState.languageData.of(() => [
              {
                closeBrackets: {
                  brackets: ["(", "[", "'", '"', "`"],
                },
              },
            ]),
            autocompletion({
              override: [completionSource],
              defaultKeymap: true,
            }),
            keymap.of([
              ...closeBracketsKeymap,
              ...defaultKeymap,
              ...historyKeymap,
              ...completionKeymap,
              indentWithTab,
            ]),
            EditorView.lineWrapping,
            EditorView.updateListener.of((update) => {
              if (update.docChanged) {
                onChangeRef.current(update.state.doc.toString());
              }
            }),
            codeEditorTheme(
              fill ? { fill: true } : { minHeight: 280, maxHeight: 480 },
            ),
            lintGutter(),
            // Lint mechanism: wavy error underline + gutter marker + hover tooltip; deferred validation keeps input smooth
            linter(lintSource, { delay: 250 }),
            ...(ph ? [placeholder(ph)] : []),
            languageCompartment.of(
              modeRef.current === "json"
                ? [json(), jsoncCommentHighlight]
                : modeRef.current === "xml"
                  ? xml()
                  : [],
            ),
            themeCompartment.of(
              isDarkRef.current ? darkExtensions : lightExtensions,
            ),
          ],
        }),
        parent: host,
      });
      viewRef.current = view;

      return () => {
        view.destroy();
        viewRef.current = null;
        themeCompartmentRef.current = null;
        languageCompartmentRef.current = null;
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Find/replace shortcut fallback: CodeMirror's keymap is only attached to .cm-content, so when focus is
    // inside the editor area but not on a content line (e.g. clicking the empty area didn't place the cursor in the text),
    // Ctrl+F etc. won't fire. Here we add a layer at the window bubbling phase: if the event wasn't handled by CodeMirror/the panel
    // (!defaultPrevented) and focus is still within this editor's host, we run the command on its behalf.
    // It must not preventDefault during the capture phase — that would make CodeMirror ignore the keystroke entirely (see the earlier bug).
    useEffect(() => {
      const host = hostRef.current;
      if (!host) return;
      const onKeyDown = (e: KeyboardEvent) => {
        const view = viewRef.current;
        if (!view) return;
        // Focus is not within this editor's area → let other editors / the page handle it
        const active = view.contentDOM.ownerDocument.activeElement;
        if (!(active instanceof Node) || !host.contains(active)) return;
        // CodeMirror / the find panel already handled it (including normal content-line focus) → don't repeat
        if (e.defaultPrevented) return;

        const mod = e.ctrlKey || e.metaKey;
        const key = e.key.toLowerCase();
        if (mod && key === "f") {
          e.preventDefault();
          openSearchPanel(view);
        } else if (mod && key === "r") {
          e.preventDefault();
          openReplaceMode(view);
        } else if (key === "f3") {
          e.preventDefault();
          if (e.shiftKey) findPrevious(view);
          else findNext(view);
        } else if (mod && key === "g") {
          e.preventDefault();
          if (e.shiftKey) findPrevious(view);
          else findNext(view);
        } else if (e.key === "Escape" && closeSearchPanel(view)) {
          e.preventDefault();
        }
      };
      window.addEventListener("keydown", onKeyDown);
      return () => window.removeEventListener("keydown", onKeyDown);
    }, []);

    // When clicking a "non-text" part of the editor area (line-number gutter / bottom blank / border),
    // if CodeMirror itself didn't take over (didn't place the cursor in a content line), give focus to the host,
    // so subsequent Ctrl+F etc. are caught by the fallback logic above.
    useEffect(() => {
      const host = hostRef.current;
      const view = viewRef.current;
      if (!host || !view) return;
      const onPointerDown = (e: PointerEvent) => {
        const t = e.target as Node | null;
        if (!t) return;
        // Inside the text content area / the find panel → CodeMirror manages focus itself; don't intervene
        if (view.contentDOM.contains(t)) return;
        if (host.querySelector(".cm-panels")?.contains(t)) return;
        host.focus({ preventScroll: true });
      };
      host.addEventListener("pointerdown", onPointerDown);
      return () => host.removeEventListener("pointerdown", onPointerDown);
    }, []);

    // mode change → switch language extension and re-validate (json/xml/raw)
    useEffect(() => {
      const view = viewRef.current;
      const comp = languageCompartmentRef.current;
      if (!view || !comp) return;
      view.dispatch({
        effects: comp.reconfigure(
          mode === "json"
            ? [json(), jsoncCommentHighlight]
            : mode === "xml"
              ? xml()
              : [],
        ),
      });
      forceLinting(view);
    }, [mode]);

    // Theme switch
    useEffect(() => {
      const view = viewRef.current;
      const comp = themeCompartmentRef.current;
      if (!view || !comp) return;
      view.dispatch({
        effects: comp.reconfigure(isDark ? darkExtensions : lightExtensions),
      });
    }, [isDark]);

    // External value change → sync into the editor (only when content differs, to avoid clobbering the cursor)
    useEffect(() => {
      const view = viewRef.current;
      if (!view) return;
      const cur = view.state.doc.toString();
      if (cur !== value) {
        view.dispatch({
          changes: { from: 0, to: cur.length, insert: value },
        });
      }
    }, [value]);

    // Expose insertAtCursor / openFind / openReplace
    useImperativeHandle(
      ref,
      () => ({
        insertAtCursor: (text: string) => {
          const view = viewRef.current;
          if (!view) return;
          const sel = view.state.selection.main;
          const pos = sel.from + text.length;
          view.dispatch({
            changes: { from: sel.from, to: sel.to, insert: text },
            selection: { anchor: pos },
            scrollIntoView: true,
          });
          view.focus();
        },
        openFind: () => {
          const view = viewRef.current;
          if (view) openSearchPanel(view);
        },
        openReplace: () => {
          const view = viewRef.current;
          if (view) openReplaceMode(view);
        },
      }),
      [],
    );

    return (
      <div
        ref={hostRef}
        // tabIndex lets blank areas / line numbers also receive keyboard focus: as long as focus is within the editor area,
        // shortcuts like Ctrl+F work (CodeMirror itself only responds to keymap on content lines).
        tabIndex={-1}
        className={cn(
          "rounded-md border border-input bg-background overflow-hidden outline-none focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50",
          fill && "min-h-[280px] flex-1",
        )}
      />
    );
  },
);
