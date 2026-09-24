// Message editor (interactive send area): on par with Postman/Apifox's Message editor.
// Format selection (text / json / base64 / hex) + JSON syntax highlighting (CodeMirror) + beautify + timed/continuous send.
import { useEffect, useRef, useState } from "react";
import { Repeat, Send, Sparkles } from "lucide-react";
import { EditorState } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  placeholder as cmPlaceholder,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { javascript } from "@codemirror/lang-javascript";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useT } from "@/lib/i18n";

export type MsgFormat = "text" | "json" | "base64" | "hex";
const MSG_FORMATS: { value: MsgFormat; label: string }[] = [
  { value: "text", label: "Text" },
  { value: "json", label: "JSON" },
  { value: "base64", label: "Base64" },
  { value: "hex", label: "Hex" },
];

/** Lightweight CodeMirror JSON editor (json highlighting + line numbers + dark-mode aware) */
function JsonEditor({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const [isDark, setIsDark] = useState(
    () =>
      typeof document !== "undefined" &&
      document.documentElement.classList.contains("dark"),
  );

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

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          highlightActiveLine(),
          javascript(),
          cmPlaceholder(placeholder),
          EditorView.theme({
            "&": { height: "100%", fontSize: "12px" },
            ".cm-scroller": { overflow: "auto" },
            "&.cm-focused": { outline: "none" },
          }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onChangeRef.current(u.state.doc.toString());
          }),
        ],
      }),
    });
    viewRef.current = view;
    return () => view.destroy();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Dark-theme switching (oneDark replaces the whole theme; light uses the default theme)
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    // Simplified: enabling oneDark in dark mode would require rebuilding; toggling the class here has little impact,
    // and editorTheme controls the background. The JSON editor background matches the panel.
  }, [isDark]);

  // Sync external value changes (user input is written back via updateListener to avoid cursor jumps)
  useEffect(() => {
    const view = viewRef.current;
    if (view && view.state.doc.toString() !== value) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value },
      });
    }
  }, [value]);

  return (
    <div
      ref={hostRef}
      className="h-full w-full overflow-hidden rounded border border-input bg-background"
    />
  );
}

export function MessageComposer({
  value,
  onChange,
  format,
  onFormatChange,
  onSend,
  sending,
  placeholder,
  autoSend,
  onAutoSendChange,
  intervalMs,
  onIntervalChange,
}: {
  value: string;
  onChange: (v: string) => void;
  format: MsgFormat;
  onFormatChange: (f: MsgFormat) => void;
  onSend: () => void;
  sending: boolean;
  placeholder: string;
  /** Timed / continuous send (common in TCP/UDP scenarios; optional) */
  autoSend?: boolean;
  onAutoSendChange?: (v: boolean) => void;
  intervalMs?: number;
  onIntervalChange?: (ms: number) => void;
}) {
  const { t } = useT();
  const beautify = () => {
    if (format !== "json") return;
    try {
      onChange(JSON.stringify(JSON.parse(value), null, 2));
    } catch {
      /* Ignore invalid JSON */
    }
  };

  // Timed / continuous send: call onSend on an interval (keep the message content; can be stopped anytime)
  const onSendRef = useRef(onSend);
  onSendRef.current = onSend;
  const valueRef = useRef(value);
  valueRef.current = value;
  useEffect(() => {
    if (!autoSend || !valueRef.current.trim()) return;
    const timer = setInterval(
      () => onSendRef.current(),
      Math.max(intervalMs ?? 1000, 50),
    );
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoSend, intervalMs]);

  return (
    <div className="space-y-1.5 border-b border-border px-3 py-2">
      <div className="flex items-center gap-1.5">
        <Select
          value={format}
          onValueChange={(v) => onFormatChange(v as MsgFormat)}
        >
          <SelectTrigger className="h-7 w-24 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MSG_FORMATS.map((f) => (
              <SelectItem key={f.value} value={f.value} className="text-xs">
                {f.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="ghost"
          size="icon-sm"
          className="h-7 w-7"
          title={t("msgComposer.beautify")}
          onClick={beautify}
        >
          <Sparkles className="h-3.5 w-3.5" />
        </Button>
        {onAutoSendChange && (
          <div className="flex items-center gap-1 rounded border border-border/70 px-1.5 py-0.5">
            <Button
              variant={autoSend ? "secondary" : "ghost"}
              size="icon-sm"
              className="h-5 w-5"
              title={t("msgComposer.autoSend")}
              onClick={() => onAutoSendChange(!autoSend)}
            >
              <Repeat className="h-3 w-3" />
            </Button>
            {autoSend && (
              <input
                className="h-5 w-14 rounded border border-input bg-background px-1 text-xs"
                type="number"
                min={50}
                value={intervalMs ?? 1000}
                onChange={(e) =>
                  onIntervalChange?.(parseInt(e.target.value) || 1000)
                }
                title={t("msgComposer.intervalMs")}
              />
            )}
          </div>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          <Button
            size="sm"
            className="gap-1"
            onClick={onSend}
            disabled={sending || !value.trim()}
          >
            <Send className="h-3.5 w-3.5" />{" "}
            {sending ? t("msgComposer.sending") : t("msgComposer.send")}
          </Button>
        </div>
      </div>
      <div className="h-28">
        {format === "json" ? (
          <JsonEditor
            value={value}
            onChange={onChange}
            placeholder={placeholder}
          />
        ) : (
          <Textarea
            className="h-full w-full text-xs font-mono"
            placeholder={placeholder}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) onSend();
            }}
          />
        )}
      </div>
    </div>
  );
}
