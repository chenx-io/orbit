// Script editor: a CodeMirror 6 wrapper for JS code editing (shared by pre/post scripts).
// Provides JS syntax highlighting (light/dark adaptive), line numbers, and autocompletion (pm.* API / variables / snippets).
import { useCallback, useEffect, useRef, useState } from "react";
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
import { javascript } from "@codemirror/lang-javascript";
import {
  autocompletion,
  completionKeymap,
  closeBrackets,
  closeBracketsKeymap,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { useT } from "@/lib/i18n";
import { filterCompletions, filterTopLevel } from "@/lib/scriptCompletions";
import {
  lightExtensions,
  darkExtensions,
  codeEditorTheme,
} from "./codeEditorShared";

export type ScriptKind = "pre" | "post";

interface ScriptEditorProps {
  value: string;
  onChange: (value: string) => void;
  kind: ScriptKind;
  /** Environment variable names (global + environment + secrets), used for {{var}} completion */
  variables?: string[];
  placeholder?: string;
}

export function ScriptEditor({
  value,
  onChange,
  kind,
  variables,
  placeholder: ph,
}: ScriptEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const themeCompartmentRef = useRef<Compartment | null>(null);

  const { t, locale } = useT();

  // Mutable context: synced on render so the editor / completion-source closures always see the latest values
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const ctxRef = useRef({ kind, variables, locale, t });
  ctxRef.current = { kind, variables, locale, t };
  const valueRef = useRef(value);
  valueRef.current = value;

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

  // Autocompletion source: parse the dot-path before the cursor, filter by scope, then return the matching members/snippets
  const completionSource = useCallback(
    (context: CompletionContext): CompletionResult | null => {
      const { kind, variables, locale, t } = ctxRef.current;
      const line = context.state.doc.lineAt(context.pos);
      const before = line.text.slice(0, context.pos - line.from);

      // After typing {{ or {{prefix: complete variable names.
      // `from` covers only the typed prefix (excluding {{), otherwise CodeMirror would use [from,pos) including {{ to filter candidates and hide the panel;
      // the custom apply finds {{ backward and replaces it as a whole, avoiding duplicated {{.
      if (variables && variables.length > 0) {
        const m = before.match(/\{\{\s*([\w.-]*)$/);
        if (m) {
          const prefix = m[1];
          const options = Array.from(new Set(variables))
            .filter((v) => v.startsWith(prefix))
            .map((v) => ({
              label: v,
              apply: (
                view: EditorView,
                completion: Completion,
                from: number,
                to: number,
              ) => {
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
              detail: t("script.varLabel"),
              type: "variable" as const,
            }));
          if (options.length) {
            return { from: context.pos - prefix.length, options };
          }
          return null; // No match → fall back to language built-in completion (keywords, etc.)
        }
      }

      // Dot-path: pm.xxx. / console. / CryptoJS.xxx.
      const pathMatch = before.match(
        /((?:pm|console|CryptoJS)(?:\.[A-Za-z_$][\w$]*)*)\.([\w$]*)$/,
      );
      if (pathMatch) {
        const options = filterCompletions(
          pathMatch[1],
          pathMatch[2],
          kind,
          locale,
        );
        if (options.length) {
          return { from: context.pos - pathMatch[2].length, options };
        }
        return null; // No match → fall back to language built-in completion (keywords, etc.)
      }

      // Top-level: pm / CryptoJS / console / require / snippets
      const word = before.match(/([\w$]*)$/)?.[1] ?? "";
      const options = filterTopLevel(word, kind, locale);
      if (options.length) {
        return { from: context.pos - word.length, options };
      }
      return null;
    },
    [],
  );

  // Create the editor once
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const themeCompartment = new Compartment();
    themeCompartmentRef.current = themeCompartment;

    const view = new EditorView({
      state: EditorState.create({
        doc: valueRef.current,
        extensions: [
          lineNumbers(),
          history(),
          drawSelection(),
          highlightActiveLine(),
          javascript(),
          autocompletion({
            override: [completionSource],
            defaultKeymap: true,
          }),
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
          codeEditorTheme(),
          ...(ph ? [placeholder(ph)] : []),
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
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Theme switch (Compartment reconfigure, no editor rebuild)
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

  return (
    <div
      ref={hostRef}
      className="rounded-md border border-input bg-background overflow-hidden focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50"
    />
  );
}
