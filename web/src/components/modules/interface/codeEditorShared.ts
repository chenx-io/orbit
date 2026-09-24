// Shared CodeMirror configuration: VSCode default theme colors (Dark+ / Light+) plus the base appearance theme.
// Shared by ScriptEditor (scripts) and BodyEditor (request bodies) so they look identical.

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { EditorView, Decoration, keymap } from "@codemirror/view";
import { Prec, RangeSet, RangeSetBuilder, StateField } from "@codemirror/state";
import type { Extension } from "@codemirror/state";
import {
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
} from "@codemirror/search";
import { createSearchPanel } from "./cmSearchPanel";

/** Editor height: minHeight guarantees an initially visible area; content beyond maxHeight scrolls */
export const EDITOR_MIN_HEIGHT = 280;
export const EDITOR_MAX_HEIGHT = 480;

// ─── VSCode default theme syntax highlighting (Light+ / Dark+) ───
// Colors come from the tokenColors of VSCode's built-in default themes so keys (property) and values (string)
// separate by hue and contrast clearly: Light+ uses a dark blue key against a dark red value, Dark+ a light blue key against an orange value.

// VSCode Light+ (the light default)
const LIGHT_STYLE = HighlightStyle.define([
  {
    tag: [t.keyword, t.moduleKeyword, t.operatorKeyword, t.definitionKeyword],
    color: "#0000ff",
  }, // keyword: pure blue
  { tag: [t.controlKeyword], color: "#af00db" }, // control-flow keyword (if/for/return): purple
  { tag: [t.string, t.special(t.string), t.regexp], color: "#a31515" }, // string: dark red
  { tag: [t.number, t.integer, t.float], color: "#098658" }, // number: dark green
  {
    tag: [t.bool, t.null, t.atom, t.constant(t.variableName)],
    color: "#0000ff",
  }, // true/false/null/constant: blue
  { tag: [t.variableName, t.special(t.variableName)], color: "#001080" }, // variable name: dark blue
  { tag: [t.propertyName, t.attributeName], color: "#001080" }, // property name (JSON key): dark blue
  {
    tag: [t.function(t.variableName), t.function(t.propertyName), t.labelName],
    color: "#795e26",
  }, // function: olive brown
  { tag: [t.typeName, t.className, t.namespace], color: "#267f99" }, // type/class: teal
  { tag: [t.operator], color: "#000000" }, // operator: foreground color
  {
    tag: [t.comment, t.lineComment, t.blockComment],
    color: "#6a737d",
    fontStyle: "italic",
  }, // comment: neutral gray italic (clearly distinct from values such as green numbers / red strings)
  { tag: [t.escape], color: "#a31515" },
]);

// VSCode Dark+ (the dark default)
const DARK_STYLE = HighlightStyle.define([
  {
    tag: [t.keyword, t.moduleKeyword, t.operatorKeyword, t.definitionKeyword],
    color: "#569cd6",
  }, // keyword: bright blue
  { tag: [t.controlKeyword], color: "#c586c0" }, // control-flow keyword: purple
  { tag: [t.string, t.special(t.string), t.regexp], color: "#ce9178" }, // string: orange
  { tag: [t.number, t.integer, t.float], color: "#b5cea8" }, // number: light green
  {
    tag: [t.bool, t.null, t.atom, t.constant(t.variableName)],
    color: "#569cd6",
  }, // true/false/null/constant: blue
  { tag: [t.variableName, t.special(t.variableName)], color: "#9cdcfe" }, // variable name: light blue
  { tag: [t.propertyName, t.attributeName], color: "#9cdcfe" }, // property name (JSON key): light blue
  {
    tag: [t.function(t.variableName), t.function(t.propertyName), t.labelName],
    color: "#dcdcaa",
  }, // function: yellow
  { tag: [t.typeName, t.className, t.namespace], color: "#4ec9b0" }, // type/class: teal green
  { tag: [t.operator], color: "#d4d4d4" }, // operator: foreground color
  {
    tag: [t.comment, t.lineComment, t.blockComment],
    color: "#8b949e",
    fontStyle: "italic",
  }, // comment: neutral gray italic (clearly distinct from values such as green numbers / orange strings)
  { tag: [t.escape], color: "#ce9178" },
]);

// VSCode editor chrome (background / line numbers / selection / cursor / active line)
const DARK_BASE = EditorView.theme({
  "&": { backgroundColor: "#1e1e1e", color: "#d4d4d4" },
  ".cm-gutters": { backgroundColor: "#1e1e1e", color: "#858585" },
  ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.06)" },
  // Selection: a low-opacity dark blue-gray so the highlight is not harsh;
  // slightly deeper when focused and lighter when blurred, keeping the selected text readable at all times.
  // The selector specificity must match drawSelection()'s defaults, otherwise these rules are overridden.
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    backgroundColor: "rgba(38,79,120,0.32)",
  },
  "> .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    backgroundColor: "rgba(38,79,120,0.16)",
  },
  ".cm-cursor": { borderLeftColor: "#aeafad" },
  // JSON comments (lezer json does not recognize them; flagged by jsoncCommentHighlight): neutral gray italic.
  // Sub-tokens inside a comment (lezer keeps parsing numbers as number) inherit the comment color, avoiding mixed letter/number colors.
  ".cm-jsonc-comment, .cm-jsonc-comment *": {
    color: "#8b949e !important",
    fontStyle: "italic !important",
  },
  // Search panel styling lives in searchPanelTheme inside searchExtensions() (using semantic tokens throughout)
});

const LIGHT_BASE = EditorView.theme({
  "&": { backgroundColor: "#ffffff", color: "#000000" },
  ".cm-gutters": { backgroundColor: "#ffffff", color: "#237893" },
  ".cm-activeLine": { backgroundColor: "rgba(0,0,0,0.05)" },
  // The selector specificity must match drawSelection()'s defaults
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    backgroundColor: "#add6ff",
  },
  ".cm-cursor": { borderLeftColor: "#000000" },
  // JSON comment: neutral gray italic (muted, not harsh, distinct from values such as green numbers / red strings); inner sub-tokens inherit the comment color
  ".cm-jsonc-comment, .cm-jsonc-comment *": {
    color: "#6a737d !important",
    fontStyle: "italic !important",
  },
  // Search panel styling lives in searchPanelTheme inside searchExtensions() (using semantic tokens throughout)
});

// fallback: true and registered after the language extension → overrides the language's own highlighting (later registration wins)
export const lightExtensions: Extension[] = [
  LIGHT_BASE,
  syntaxHighlighting(LIGHT_STYLE, { fallback: true }),
];

export const darkExtensions: Extension[] = [
  DARK_BASE,
  syntaxHighlighting(DARK_STYLE, { fallback: true }),
];

// ─── Find / replace (@codemirror/search plus a custom panel)────────────────
// The classic in-editor find bar: Mod-f opens the find panel, Mod-r focuses the replace input,
// F3 / Shift-F3 (or Mod-g / Shift-Mod-g) go to the previous/next match and Esc closes it.
// Registered with Prec.high so defaultKeymap / closeBracketsKeymap cannot swallow it.
// The panel itself is rendered by cmSearchPanel.tsx (VS Code style: icon buttons plus switches embedded in the input,
// with copy following the app language); container placement and match highlighting come from searchPanelTheme below.
export function openReplaceMode(view: EditorView): boolean {
  openSearchPanel(view);
  const focusReplace = () => {
    const el = view.dom.querySelector<HTMLInputElement>(
      'input[name="replace"]',
    );
    if (el) el.focus();
  };
  // The panel node is inserted only after the state update, so wait one frame before focusing
  const doc = view.contentDOM.ownerDocument;
  const frame = doc.defaultView?.setTimeout ?? window.setTimeout;
  frame(focusReplace, 0);
  return true;
}

// Find panel container placement plus match highlighting (the panel itself is rendered by cmSearchPanel.tsx with React + Tailwind).
// Note: the CM base theme treats a theme not marked dark as light, drawing a 1px light bottom border on .cm-panels-top
// and a light background on .cm-panels (a "white line" appears under the overlay), so both are disabled explicitly here.
const searchPanelTheme = EditorView.theme({
  ".cm-panels": { backgroundColor: "transparent", border: "none" },
  ".cm-panels.cm-panels-top": {
    position: "absolute",
    top: "4px",
    right: "6px",
    left: "auto",
    borderBottom: "none",
  },
  // Match highlighting: other matches get a faint primary background, the current match a warning accent
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in oklab, var(--primary) 14%, transparent)",
  },
  ".cm-searchMatch-selected": {
    backgroundColor: "color-mix(in oklab, var(--warning) 45%, transparent)",
  },
  ".cm-selectionMatch": {
    backgroundColor: "color-mix(in oklab, var(--primary) 12%, transparent)",
  },
});

export function searchExtensions(opts?: { top?: boolean }): Extension[] {
  return [
    // The panel itself is custom-rendered by cmSearchPanel.tsx (a VS Code style icon layout)
    search({
      top: opts?.top !== false,
      createPanel: createSearchPanel,
    }),
    // Panel visuals (semantic tokens, adapting to light/dark)
    searchPanelTheme,
    // Selecting text highlights all matches faintly at once (matching the editor's native behavior)
    highlightSelectionMatches(),
    Prec.high(
      keymap.of([
        ...searchKeymap,
        // The familiar Ctrl/Cmd+R maps to replace while also absorbing the webview's default "reload page".
        // The scope matches searchKeymap: it takes effect in both the editor content and the find panel's input.
        { key: "Mod-r", run: openReplaceMode, scope: "editor search-panel" },
      ]),
    ),
  ];
}

// ─── JSON comment highlighting (JSONC)─────────────────────────────────────
// The lezer json grammar does not recognize comments (`// ...` / `/* ... */` parse as error nodes
// with no comment tag, so comment text falls back to the default foreground and looks "too bright").
// A StateField scans comments "outside strings" and applies Decoration.mark,
// rendered by the .cm-jsonc-comment style (VSCode comment green plus italic); a `//` / `/*` inside a string is not misdetected.

const jsoncCommentMark = Decoration.mark({ class: "cm-jsonc-comment" });

const jsoncCommentField = StateField.define<RangeSet<Decoration>>({
  create(state) {
    return scanJsoncComments(state.doc.toString());
  },
  update(value, tr) {
    if (!tr.docChanged) return value;
    return scanJsoncComments(tr.state.doc.toString());
  },
});

function scanJsoncComments(str: string): RangeSet<Decoration> {
  const builder = new RangeSetBuilder<Decoration>();
  let i = 0;
  let inString = false;
  let escape = false;
  const n = str.length;
  while (i < n) {
    const ch = str[i];
    if (inString) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inString = false;
      i++;
      continue;
    }
    if (ch === '"') {
      inString = true;
      i++;
      continue;
    }
    if (ch === "/" && str[i + 1] === "/") {
      let j = i;
      while (j < n && str[j] !== "\n") j++;
      builder.add(i, j, jsoncCommentMark);
      i = j;
      continue;
    }
    if (ch === "/" && str[i + 1] === "*") {
      let j = i + 2;
      while (j < n && !(str[j] === "*" && str[j + 1] === "/")) j++;
      if (j < n) j += 2;
      builder.add(i, j, jsoncCommentMark);
      i = j;
      continue;
    }
    i++;
  }
  return builder.finish();
}

/** Comment-highlighting extension for the JSON body editor (matching stripJsonComments' "no misdetection inside strings" semantics) */
export const jsoncCommentHighlight: Extension = [
  jsoncCommentField,
  EditorView.decorations.from(jsoncCommentField),
];

/** Base editor appearance theme (height / font / line-number gutter / completion popup and so on). */
export function codeEditorTheme(opts?: {
  minHeight?: number;
  maxHeight?: number;
  /** Fill the parent's height (flex layouts): 100% height while keeping a minimum height (EDITOR_MIN_HEIGHT by default) */
  fill?: boolean;
}): Extension {
  const minHeight = opts?.minHeight ?? EDITOR_MIN_HEIGHT;
  const maxHeight = opts?.maxHeight ?? EDITOR_MAX_HEIGHT;
  const fill = opts?.fill === true;
  return EditorView.theme({
    "&": {
      fontSize: "12px",
      // Corner clipping is handled by the host div (rounded-md + overflow-hidden) so that
      // this element carries no border-radius/overflow of its own; otherwise an opaque inner background would cover the host's 1px border
      // and the border would fade or vanish at the corners
      // fill mode: minHeight uses the default (staying usable in small panels) while the height is a full 100%
      minHeight: `${minHeight}px`,
      ...(fill ? { height: "100%" } : {}),
      // The background color comes from LIGHT_BASE / DARK_BASE (the VSCode themes)
    },
    "&.cm-focused": { outline: "none" },
    // Disable the browser's default square outline on inner focusable elements (.cm-content etc.),
    // otherwise focusing draws black square corners with no color at the rounded edges (clashing with the outer glow ring)
    ".cm-content": {
      outline: "none",
      padding: "6px 2px",
    },
    ".cm-scroller": {
      outline: "none",
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      lineHeight: "1.6",
      minHeight: `${minHeight}px`,
      maxHeight: fill ? "none" : `${maxHeight}px`,
      ...(fill ? { height: "100%", overflow: "auto" } : {}),
    },
    // Autocomplete popup: under a dark theme CodeMirror's default tooltip background is light while the text inherits the editor's
    // light foreground, making options unreadable. Overridden explicitly with theme variables (background/text/border/selected item).
    ".cm-tooltip-autocomplete": {
      backgroundColor: "var(--popover)",
      border: "1px solid var(--border)",
      color: "var(--popover-foreground)",
      borderRadius: "6px",
      boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
    },
    ".cm-tooltip-autocomplete > ul > li": {
      color: "var(--popover-foreground)",
    },
    // Selected item: the theme accent (a bright blue background with dark text under dark, consistent with light)
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--accent)",
      color: "var(--accent-foreground)",
    },
    // Match highlighting and detail text: kept readable under dark
    ".cm-tooltip-autocomplete .cm-completionMatchedText": {
      color: "var(--primary)",
      textDecoration: "none",
    },
    ".cm-tooltip-autocomplete .cm-completionDetail": {
      color: "var(--muted-foreground)",
      fontStyle: "italic",
    },
    // Maximum autocomplete popup height
    ".cm-tooltip-autocomplete > ul": { maxHeight: "260px" },
    // Lint markers (error squiggles / line-number gutter icons / hover tooltips) — adapted for dark
    ".cm-lintRange-error": {
      backgroundImage:
        "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='6' height='3'%3E%3Cpath fill='%23e5534b' d='m0 2 1-1 1 1 1-1 1 1 1-1 1 1'/%3E%3C/svg%3E\")",
      backgroundPosition: "left bottom",
      backgroundRepeat: "repeat-x",
    },
    ".cm-lintMarker-error": { color: "#e5534b" },
    ".cm-tooltip-lint": {
      backgroundColor: "var(--popover)",
      border: "1px solid var(--border)",
      color: "var(--popover-foreground)",
      borderRadius: "6px",
      boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
    },
    ".cm-tooltip-lint .cm-lint-message-error": { color: "#e5534b" },
  });
}
