import { useMemo, useState } from "react";
import {
  Search,
  Trash2,
  Play,
  Clock,
  ArrowDownToLine,
  Filter,
  X,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn, methodBg, statusColor, formatBytes, timeAgo } from "@/lib/utils";
import { useAppStore } from "@/store/useStore";
import type { HistoryEntry } from "@/data/types";
import { TimingBreakdown } from "@/components/common/TimingBreakdown";
import { useT } from "@/lib/i18n";

function HistoryRow({
  entry,
  active,
  onSelect,
  onRerun,
}: {
  entry: HistoryEntry;
  active: boolean;
  onSelect: () => void;
  onRerun: () => void;
}) {
  const { t } = useT();
  return (
    <div
      onClick={onSelect}
      className={cn(
        "group flex cursor-pointer items-center gap-2 rounded-md border px-1.5 py-1 text-sm transition-colors",
        active
          ? "border-primary/40 bg-accent/20"
          : "border-border hover:bg-accent/10",
      )}
    >
      <span
        className={cn(
          "w-14 shrink-0 rounded border px-1.5 py-0.5 text-center text-sm font-bold",
          methodBg(entry.method),
        )}
      >
        {entry.method}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <span
            className={cn(
              "font-mono text-xs font-bold",
              statusColor(entry.status),
            )}
          >
            {entry.status ?? "—"}
          </span>
          <span className="min-w-0 break-all font-medium">{entry.name}</span>
        </div>
        <div className="break-all font-mono text-xs text-muted-foreground">
          {entry.url}
        </div>
        <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
          {entry.duration != null && (
            <span className="flex items-center gap-1">
              <Clock className="h-3 w-3" />
              {entry.duration}ms
            </span>
          )}
          {entry.size != null && <span>{formatBytes(entry.size)}</span>}
          <span>{timeAgo(entry.timestamp)}</span>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon-sm"
        className="shrink-0 opacity-0 group-hover:opacity-100"
        title={t("history.resend")}
        onClick={(e) => {
          e.stopPropagation();
          onRerun();
        }}
      >
        <Play className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}

export function HistoryModule() {
  const { t } = useT();
  const history = useAppStore((s) => s.history);
  const requests = useAppStore((s) => s.requests);
  const openTab = useAppStore((s) => s.openTab);
  const sendRequest = useAppStore((s) => s.sendRequest);
  const clearHistory = useAppStore((s) => s.clearHistory);

  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [selectedId, setSelectedId] = useState<string | null>(
    history[0]?.id ?? null,
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return history.filter((h) => {
      if (
        q &&
        !(
          h.name.toLowerCase().includes(q) ||
          h.url.toLowerCase().includes(q) ||
          h.method.toLowerCase().includes(q)
        )
      )
        return false;
      if (statusFilter === "ok" && (h.status == null || h.status >= 400))
        return false;
      if (statusFilter === "err" && (h.status == null || h.status < 400))
        return false;
      return true;
    });
  }, [history, query, statusFilter]);

  const selected =
    history.find((h) => h.id === selectedId) ?? filtered[0] ?? null;

  const rerun = (entry: HistoryEntry) => {
    const req = requests[entry.requestId];
    if (req) {
      openTab(req.id);
      void sendRequest(req.id);
    }
  };

  return (
    <PanelGroup
      direction="horizontal"
      autoSaveId="history-layout"
      className="h-full"
    >
      {/* List */}
      <Panel defaultSize={25} minSize={25}>
        <div className="flex h-full flex-col border-r border-border bg-card/30">
          <div className="space-y-2 border-b border-border p-3">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("history.searchPlaceholder")}
                className="h-8 pl-8 text-xs"
              />
              {query && (
                <button
                  onClick={() => setQuery("")}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              )}
            </div>
            <div className="flex items-center gap-2">
              <Filter className="h-3.5 w-3.5 text-muted-foreground" />
              <Select value={statusFilter} onValueChange={setStatusFilter}>
                <SelectTrigger className="h-8 flex-1 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t("history.allStatus")}</SelectItem>
                  <SelectItem value="ok">{t("history.okStatus")}</SelectItem>
                  <SelectItem value="err">{t("history.errStatus")}</SelectItem>
                </SelectContent>
              </Select>
              <Button
                variant="ghost"
                size="sm"
                className="text-destructive"
                onClick={clearHistory}
                title={t("history.clear")}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
          <ScrollArea className="flex-1">
            <div className="space-y-1.5 p-2">
              {filtered.length === 0 && (
                <div className="p-8 text-center text-xs text-muted-foreground">
                  {t("history.noMatch")}
                </div>
              )}
              {filtered.map((h) => (
                <HistoryRow
                  key={h.id}
                  entry={h}
                  active={selected?.id === h.id}
                  onSelect={() => setSelectedId(h.id)}
                  onRerun={() => rerun(h)}
                />
              ))}
            </div>
          </ScrollArea>
        </div>
      </Panel>

      <PanelResizeHandle className="w-1 bg-border transition-colors hover:bg-primary" />

      {/* Details */}
      <Panel minSize={30}>
        {!selected ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {t("history.selectOne")}
          </div>
        ) : (
          <div className="space-y-3 p-2">
            <div className="flex items-center gap-2">
              <span
                className={cn(
                  "rounded border px-2 py-0.5 text-xs font-bold",
                  methodBg(selected.method),
                )}
              >
                {selected.method}
              </span>
              <h2 className="text-lg font-semibold">{selected.name}</h2>
              <Button
                variant="outline"
                size="sm"
                className="ml-auto gap-1.5"
                onClick={() => rerun(selected)}
              >
                <Play className="h-3.5 w-3.5" /> {t("history.resend")}
              </Button>
            </div>
            <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
              <span
                className={cn(
                  "font-mono font-bold",
                  statusColor(selected.status),
                )}
              >
                {selected.status ?? "—"}
              </span>
              <span className="flex items-center gap-1">
                <Clock className="h-3.5 w-3.5" />
                {selected.duration}ms
              </span>
              <span>{formatBytes(selected.size)}</span>
              <span>{new Date(selected.timestamp).toLocaleString()}</span>
            </div>
            <Card>
              <CardContent className="p-3">
                <div className="mb-1 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
                  {t("history.url")}
                </div>
                <div className="break-all font-mono text-xs">
                  {selected.url}
                </div>
              </CardContent>
            </Card>

            <Tabs defaultValue="body">
              <TabsList className="h-8">
                <TabsTrigger value="body" className="text-xs">
                  {t("history.resBody")}
                </TabsTrigger>
                <TabsTrigger value="meta" className="text-xs">
                  {t("history.summary")}
                </TabsTrigger>
              </TabsList>
              <TabsContent value="body" className="mt-2">
                <Card>
                  <CardContent className="p-0">
                    <div className="flex items-center gap-1.5 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
                      <ArrowDownToLine className="h-3 w-3" /> Response Body
                    </div>
                    <pre className="max-h-105 overflow-auto p-3 font-mono text-xs leading-relaxed">
                      {selected.responseBody || t("history.noBody")}
                    </pre>
                  </CardContent>
                </Card>
              </TabsContent>
              <TabsContent value="meta" className="mt-2">
                <div className="grid grid-cols-3 gap-2 text-sm">
                  <div className="rounded border border-border p-2">
                    <div className="text-xs text-muted-foreground">
                      {t("history.statusCode")}
                    </div>
                    <div
                      className={cn(
                        "font-mono font-bold",
                        statusColor(selected.status),
                      )}
                    >
                      {selected.status ?? "—"}
                    </div>
                  </div>
                  <div className="rounded border border-border p-2">
                    <div className="text-xs text-muted-foreground">
                      {t("history.duration")}
                    </div>
                    <div className="font-mono font-bold">
                      {selected.duration}ms
                    </div>
                  </div>
                  <div className="rounded border border-border p-2">
                    <div className="text-xs text-muted-foreground">
                      {t("history.size")}
                    </div>
                    <div className="font-mono font-bold">
                      {formatBytes(selected.size)}
                    </div>
                  </div>
                </div>
                {selected.timing && (
                  <div className="mt-2">
                    <TimingBreakdown
                      timing={selected.timing}
                      totalMs={selected.duration ?? undefined}
                    />
                  </div>
                )}
              </TabsContent>
            </Tabs>
          </div>
        )}
      </Panel>
    </PanelGroup>
  );
}
