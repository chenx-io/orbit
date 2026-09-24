// Read-only response-body viewer: reuses the body editor's VSCode theme colors (Light+/Dark+),
// picks json / xml syntax highlighting by Content-Type; oversized responses fall back to plain text (performance guard).

import { useEffect, useRef } from "react";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import {
  lightExtensions,
  darkExtensions,
  codeEditorTheme,
} from "./codeEditorShared";

/** Response bodies over this many bytes fall back to plain text (avoids CodeMirror render jank) */
const LARGE_BODY_THRESHOLD = 512 * 1024;

interface ResponseBodyViewerProps {
  body: string;
  /** Response Content-Type (used to infer the highlight language) */
  contentType?: string;
}

export function ResponseBodyViewer({
  body,
  contentType,
}: ResponseBodyViewerProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const themeRef = useRef<Compartment | null>(null);

  // Create the editor once; body changes are applied via dispatch
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    if (body.length > LARGE_BODY_THRESHOLD) return; // Oversized response: fall back to plain text

    const themeCompartment = new Compartment();
    themeRef.current = themeCompartment;
    const isDark = document.documentElement.classList.contains("dark");
    const ct = (contentType ?? "").toLowerCase();
    const lang =
      ct.includes("json") || ct.includes("+json")
        ? json()
        : ct.includes("xml")
          ? xml()
          : [];

    const view = new EditorView({
      state: EditorState.create({
        doc: body,
        extensions: [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          // Soft-wrap when width is insufficient (the read-only viewer does no horizontal scroll)
          EditorView.lineWrapping,
          codeEditorTheme({ minHeight: 0, maxHeight: 10_000_000 }),
          lang,
          themeCompartment.of(isDark ? darkExtensions : lightExtensions),
        ],
      }),
      parent: host,
    });
    viewRef.current = view;

    // Watch html.dark toggling and switch the theme live
    const obs = new MutationObserver(() => {
      const v = viewRef.current;
      const comp = themeRef.current;
      if (!v || !comp) return;
      const dark = document.documentElement.classList.contains("dark");
      v.dispatch({
        effects: comp.reconfigure(dark ? darkExtensions : lightExtensions),
      });
    });
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => {
      obs.disconnect();
      view.destroy();
      viewRef.current = null;
      themeRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // body change → update the document
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: body,
      },
    });
  }, [body]);

  // Oversized response (or when CodeMirror is not mounted) → fall back to plain text
  if (body.length > LARGE_BODY_THRESHOLD) {
    return (
      <pre className="p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words">
        {body}
      </pre>
    );
  }

  return (
    <div
      ref={hostRef}
      className="w-full"
      style={{ minHeight: 0, maxHeight: "100%", overflow: "auto" }}
    />
  );
}
