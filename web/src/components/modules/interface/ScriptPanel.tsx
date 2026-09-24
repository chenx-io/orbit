// Script result panel: pre / post script console logs + post assertions.
//
// This does **not** show the execution results of actions (script / DB / built-in interpolation node): script-action console output
// already lands in the "pre script log / post script log", and DB-action results are shown via the status badge on the action card.
import { useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Terminal,
  XCircle,
} from "lucide-react";
import type { ScriptLog, TestResult } from "@/data/types";
import { useT } from "@/lib/i18n";

/** A single script log line: multi-line (pretty JSON etc.) can expand/collapse; single-line shows directly */
function ScriptLogItem({ log }: { log: ScriptLog }) {
  const isErr = log.level === "error";
  const isWarn = log.level === "warn";
  const color = isErr
    ? "text-rose-400"
    : isWarn
      ? "text-amber-400"
      : "text-muted-foreground";
  const badge = isErr ? "✗" : isWarn ? "⚠" : "›";
  const multiline = log.message.includes("\n");
  const [open, setOpen] = useState(false);

  if (!multiline) {
    return (
      <div className={`px-1.5 py-0.5 ${color}`}>
        <span className="select-none opacity-50">{badge}</span> {log.message}
      </div>
    );
  }
  return (
    <div
      className={`rounded-md border border-border/60 ${open ? "bg-muted/30" : ""}`}
    >
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className={`flex w-full items-start gap-1.5 px-1.5 py-1 text-left ${color}`}
      >
        {open ? (
          <ChevronDown className="mt-0.5 h-3 w-3 shrink-0" />
        ) : (
          <ChevronRight className="mt-0.5 h-3 w-3 shrink-0" />
        )}
        <span className="select-none opacity-50">{badge}</span>
        <span className="truncate font-mono text-xs leading-relaxed">
          {log.message.split("\n")[0]}
        </span>
      </button>
      {open && (
        <pre className="max-h-72 overflow-auto border-t border-border/60 px-2 py-1.5 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words">
          {log.message}
        </pre>
      )}
    </div>
  );
}

export function ScriptPanel({
  preLogs,
  postLogs,
  postTests,
}: {
  /** Pre-script (scripts before / after the built-in interpolation node) console logs */
  preLogs?: ScriptLog[];
  /** Post-script console logs */
  postLogs?: ScriptLog[];
  /** Post-script pm.test assertion results */
  postTests?: TestResult[];
}) {
  const { t } = useT();
  const hasContent =
    (preLogs?.length ?? 0) > 0 ||
    (postLogs?.length ?? 0) > 0 ||
    (postTests?.length ?? 0) > 0;

  if (!hasContent) {
    return (
      <div className="p-3 text-xs text-muted-foreground">
        {t("script.emptyLogs")}
      </div>
    );
  }

  const renderLogs = (title: string, logs?: ScriptLog[]) => {
    if (!logs || logs.length === 0) return null;
    return (
      <div className="mb-3">
        <div className="mb-1 flex items-center gap-1.5 text-sm font-semibold text-muted-foreground">
          <Terminal className="h-3.5 w-3.5" />
          {title}
        </div>
        <div className="space-y-0.5 rounded-md border border-border bg-background p-1 font-mono text-xs leading-relaxed">
          {logs.map((l, i) => (
            <ScriptLogItem key={i} log={l} />
          ))}
        </div>
      </div>
    );
  };

  return (
    <div className="min-w-max p-3">
      {renderLogs(t("script.preLogs"), preLogs)}
      {renderLogs(t("script.postLogs"), postLogs)}
      {postTests && postTests.length > 0 && (
        <div>
          <div className="mb-1 flex items-center gap-1.5 text-sm font-semibold text-muted-foreground">
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t("script.postTests")}
          </div>
          <div className="space-y-1.5">
            {postTests.map((a, i) => (
              <div
                key={i}
                className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-xs whitespace-nowrap"
              >
                {a.passed ? (
                  <CheckCircle2 className="h-4 w-4 text-emerald-400" />
                ) : (
                  <XCircle className="h-4 w-4 text-rose-400" />
                )}
                <span className="flex-1">{a.name}</span>
                {a.message && (
                  <span className="text-muted-foreground">{a.message}</span>
                )}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
