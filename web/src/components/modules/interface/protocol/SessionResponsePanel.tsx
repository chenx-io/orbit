// Session response panel: real-time message timeline (newest first) + message detail expansion (text/hex/base64) + type filter/search + connection status breakdown.
import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Search, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { getUiLocale } from "@/lib/localeDict";
import { useT } from "@/lib/i18n";
import { decodeMessage } from "@/hooks/useSession";
import type { SessionMessage } from "@/lib/bridge";

type ViewMode = "text" | "hex" | "base64";
type FilterMode = "all" | "send" | "recv";

function formatTime(time: number): string {
  const d = new Date(time);
  return `${d.toLocaleTimeString(getUiLocale(), { hour12: false })}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

function formatJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** Payload byte count (base64 → byte length) */
function dataBytes(data: string): number {
  if (!data) return 0;
  try {
    return atob(data).length;
  } catch {
    return 0;
  }
}

/** Human-readable byte count (1024 → "1.0 KB") */
function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

function MessageItem({
  msg,
  expanded,
  onToggle,
}: {
  msg: SessionMessage;
  expanded: boolean;
  onToggle: () => void;
}) {
  const { t } = useT();
  const [detail, setDetail] = useState<ViewMode>("text");
  const { text, hex } = decodeMessage(msg.data);
  const isSend = msg.direction === "send";
  const isSystem = msg.error != null || (msg.data === "" && msg.text == null);
  const base64 = msg.data;
  const preview = isSystem ? (msg.error ?? "") : (text ?? hex).slice(0, 80);

  let detailContent: string;
  if (isSystem) {
    detailContent = msg.error ?? "";
  } else if (detail === "hex") {
    detailContent = hex;
  } else if (detail === "base64") {
    detailContent = base64;
  } else {
    detailContent =
      text != null ? (isJsonLike(text) ? formatJson(text) : text) : hex;
  }

  return (
    <div
      className={cn(
        "rounded-md border border-border bg-card/50 text-xs",
        expanded && "bg-card",
      )}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2 py-1.5 text-left"
        onClick={onToggle}
      >
        <span
          className={cn(
            "shrink-0 text-xs",
            isSystem
              ? "text-muted-foreground"
              : msg.error
                ? "text-rose-400"
                : isSend
                  ? "text-blue-400"
                  : "text-emerald-400",
          )}
        >
          {msg.error ? "✗" : isSystem ? "●" : isSend ? "↑" : "↓"}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatTime(msg.time)}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono">{preview}</span>
        {expanded ? (
          <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground" />
        )}
      </button>
      {expanded && (
        <div className="border-t border-border p-2">
          <div className="mb-1.5 flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">
              {t("session.detail")}
            </span>
            <Select
              value={detail}
              onValueChange={(v) => setDetail(v as ViewMode)}
            >
              <SelectTrigger className="ml-auto h-6 w-24 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="text" className="text-xs">
                  text
                </SelectItem>
                <SelectItem value="hex" className="text-xs">
                  hexdump
                </SelectItem>
                <SelectItem value="base64" className="text-xs">
                  base64
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all font-mono text-xs leading-relaxed">
            {detailContent}
          </pre>
        </div>
      )}
    </div>
  );
}

/** Decide whether a text is JSON (used to auto-format the detail text view) */
function isJsonLike(s: string): boolean {
  const t = s.trim();
  return (
    (t.startsWith("{") && t.endsWith("}")) ||
    (t.startsWith("[") && t.endsWith("]"))
  );
}

export function SessionResponsePanel({
  protocol,
  messages,
  connected,
  connecting,
  onClear,
}: {
  protocol: string;
  messages: SessionMessage[];
  connected: boolean;
  connecting: boolean;
  onClear: () => void;
}) {
  const { t, format } = useT();
  const [expandedSeq, setExpandedSeq] = useState<number | null>(null);
  const [filter, setFilter] = useState<FilterMode>("all");
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    let list = messages;
    if (filter !== "all") {
      list = list.filter((m) => m.direction === filter);
    }
    if (search.trim()) {
      const q = search.trim().toLowerCase();
      list = list.filter((m) => {
        if (m.error?.toLowerCase().includes(q)) return true;
        const { text } = decodeMessage(m.data);
        return (
          (text ?? "").toLowerCase().includes(q) ||
          m.data.toLowerCase().includes(q)
        );
      });
    }
    return list;
  }, [messages, filter, search]);

  // Sent/received byte statistics (SocketTool-style status bar)
  const stats = useMemo(() => {
    let sent = 0;
    let recv = 0;
    for (const m of messages) {
      if (m.error) continue;
      if (m.direction === "send") sent += dataBytes(m.data);
      else recv += dataBytes(m.data);
    }
    return { sent, recv };
  }, [messages]);

  const status = connecting
    ? { label: t("session.connecting"), cls: "text-amber-400" }
    : connected
      ? { label: t("session.connected"), cls: "text-emerald-500" }
      : { label: t("session.disconnected"), cls: "text-muted-foreground" };

  return (
    <div className="flex h-full flex-col border-t border-border">
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
        <span className="text-xs font-medium">{t("session.response")}</span>
        <span className={cn("text-xs", status.cls)}>{status.label}</span>
        <span className="text-xs text-muted-foreground">
          {format("session.msgCount", messages.length)}
        </span>
        <span
          className="text-xs text-muted-foreground"
          title={format(
            "session.byteStat",
            formatBytes(stats.sent),
            formatBytes(stats.recv),
          )}
        >
          ↑ {formatBytes(stats.sent)} ↓ {formatBytes(stats.recv)}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <div className="relative">
            <Search className="pointer-events-none absolute left-1.5 top-1/2 h-3 w-3 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="h-6 w-36 pl-6 text-xs"
              placeholder={t("session.searchMsg")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <Select
            value={filter}
            onValueChange={(v) => setFilter(v as FilterMode)}
          >
            <SelectTrigger className="h-6 w-20 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all" className="text-xs">
                {t("session.filterAll")}
              </SelectItem>
              <SelectItem value="send" className="text-xs">
                {t("session.filterSend")}
              </SelectItem>
              <SelectItem value="recv" className="text-xs">
                {t("session.filterRecv")}
              </SelectItem>
            </SelectContent>
          </Select>
          <Button
            variant="ghost"
            size="icon-sm"
            className="h-6 w-6"
            title={t("session.clear")}
            onClick={onClear}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
      <div className="flex-1 space-y-1 overflow-y-auto p-2">
        {filtered.length === 0 && (
          <div className="py-8 text-center text-xs text-muted-foreground">
            {messages.length === 0 ? t("session.empty") : t("session.noMatch")}
          </div>
        )}
        {protocol === "sse" ? (
          <SseTableView messages={filtered} />
        ) : (
          [...filtered]
            .reverse()
            .map((m) => (
              <MessageItem
                key={m.seq}
                msg={m}
                expanded={expandedSeq === m.seq}
                onToggle={() =>
                  setExpandedSeq((p) => (p === m.seq ? null : m.seq))
                }
              />
            ))
        )}
      </div>
    </div>
  );
}

/** SSE-specific table view (id / event / data / retry / time) */
function SseTableView({ messages }: { messages: SessionMessage[] }) {
  const hasRetry = messages.some((m) => m.sse?.retry != null);
  const cols = hasRetry
    ? "grid-cols-[92px_96px_1fr_64px_92px]"
    : "grid-cols-[92px_96px_1fr_92px]";
  const gridCls = `grid ${cols} gap-2 px-2 font-mono`;
  return (
    <>
      <div
        className={`${gridCls} border-b border-border pb-1 text-xs font-semibold text-muted-foreground`}
      >
        <span>id</span>
        <span>event</span>
        <span>data</span>
        {hasRetry && <span>retry</span>}
        <span>time</span>
      </div>
      {[...messages].reverse().map((m) => {
        if (m.error) {
          return (
            <div
              key={m.seq}
              className="rounded px-2 py-1 text-xs text-rose-400"
            >
              ✗ {m.error}
            </div>
          );
        }
        const decoded = decodeMessage(m.data);
        const dataText = m.sse?.data ?? decoded.text ?? decoded.hex;
        return (
          <div key={m.seq} className={gridCls}>
            <span className="truncate text-xs">{m.sse?.id ?? ""}</span>
            <span className="truncate text-xs">{m.sse?.event ?? ""}</span>
            <span className="break-all text-xs">{dataText}</span>
            {hasRetry && <span className="text-xs">{m.sse?.retry ?? ""}</span>}
            <span className="text-xs text-muted-foreground">
              {formatTime(m.time)}
            </span>
          </div>
        );
      })}
    </>
  );
}
