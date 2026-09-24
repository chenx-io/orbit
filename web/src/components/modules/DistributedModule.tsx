// Distributed settings page:
// - controller (the client-mode gRPC server): port configuration and start/stop
// - agent list (state / resource monitoring) with refresh and "add agent" in the top right
// - request execution mode: local or a specific agent (any request can run on an agent)

import { useEffect, useRef, useState } from "react";
import {
  Loader2,
  Pause,
  Play,
  Plus,
  Power,
  PowerOff,
  RefreshCw,
  Server,
  Trash2,
  Zap,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { AgentExecutionTarget } from "@/components/common/AgentExecutionTarget";
import { useT, type TKey } from "@/lib/i18n";
import { getUiLocale, tFormat } from "@/lib/localeDict";
import { useAppStore } from "@/store/useStore";
import {
  addAgent,
  agentAction,
  connectDistributedEvents,
  controllerStart,
  controllerStatus,
  controllerStop,
  listAgents,
  type AgentInfo,
  type AgentState,
} from "@/lib/bridge/distributed";

/** Add a timeout fallback to a promise so a hung backend cannot leave a button spinning forever. */
function withTimeout<T>(p: Promise<T>, ms: number, msg?: string): Promise<T> {
  return Promise.race([
    p,
    new Promise<never>((_, reject) =>
      setTimeout(
        () =>
          reject(
            new Error(
              msg ?? tFormat("distributed.opTimeout", Math.round(ms / 1000)),
            ),
          ),
        ms,
      ),
    ),
  ]);
}

/** i18n key and badge styling per agent state. */
const STATE_META: Record<AgentState, { labelKey: string; cls: string }> = {
  idle: { labelKey: "agent.state.idle", cls: "bg-muted text-muted-foreground" },
  running: {
    labelKey: "agent.state.running",
    cls: "bg-emerald-500/15 text-emerald-400",
  },
  paused: {
    labelKey: "agent.state.paused",
    cls: "bg-amber-500/15 text-amber-400",
  },
  offline: {
    labelKey: "agent.state.offline",
    cls: "bg-muted text-muted-foreground",
  },
};

function StateBadge({ state }: { state: AgentState }) {
  const { t } = useT();
  const meta = STATE_META[state] ?? STATE_META.offline;
  return (
    <span
      className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-semibold ${meta.cls}`}
    >
      <span className="h-1.5 w-1.5 rounded-full bg-current" />
      {t(meta.labelKey as TKey)}
    </span>
  );
}

function UsageBar({ percent }: { percent: number }) {
  const p = Math.max(0, Math.min(100, percent));
  const color =
    p > 85 ? "bg-rose-500" : p > 60 ? "bg-amber-500" : "bg-emerald-500";
  return (
    <div className="flex w-24 items-center gap-1">
      <div className="h-1.5 flex-1 overflow-hidden rounded bg-muted">
        <div className={`h-full ${color}`} style={{ width: `${p}%` }} />
      </div>
      <span className="w-10 text-right font-mono text-xs">{p.toFixed(1)}%</span>
    </div>
  );
}

function Sparkline({ history }: { history: number[] }) {
  if (history.length < 2)
    return <span className="text-xs text-muted-foreground">—</span>;
  const w = 72;
  const h = 18;
  const max = Math.max(...history, 1);
  const pts = history
    .map((v, i) => {
      const x = (i / (history.length - 1)) * w;
      const y = h - (v / max) * h;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width={w} height={h} className="shrink-0">
      <polyline
        points={pts}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        className="text-primary"
      />
    </svg>
  );
}

function fmtTime(ms: number): string {
  if (!ms) return "—";
  const now = Date.now();
  const diff = Math.max(0, Math.floor((now - ms) / 1000));
  if (diff < 60) return tFormat("distributed.agoSeconds", diff);
  if (diff < 3600)
    return tFormat("distributed.agoMinutes", Math.floor(diff / 60));
  return new Date(ms).toLocaleString(getUiLocale(), { hour12: false });
}

export function DistributedModule() {
  const { t, format } = useT();
  const executionTarget = useAppStore((s) => s.executionTarget);
  const setExecutionTarget = useAppStore((s) => s.setExecutionTarget);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const streamRef = useRef<{ unlisten: () => void } | null>(null);

  // controller
  const [port, setPort] = useState("50051");
  const [ctrlAddr, setCtrlAddr] = useState<string | null>(null);
  const [ctrlBusy, setCtrlBusy] = useState(false);

  // Add agent dialog
  const [addOpen, setAddOpen] = useState(false);
  const [addr, setAddr] = useState("");
  const [agentId, setAgentId] = useState("");
  const [labels, setLabels] = useState("");
  const [adding, setAdding] = useState(false);
  /** Dialog-scoped error (shown inside the dialog instead of on the main UI it covers) */
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [claimConfirm, setClaimConfirm] = useState<{
    addr: string;
    agentId?: string;
    labels: [string, string][];
  } | null>(null);

  const refresh = async () => {
    try {
      setAgents(await listAgents());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const refreshController = async () => {
    try {
      const st = await controllerStatus();
      setCtrlAddr(st.running ? st.addr : null);
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    void refresh();
    void refreshController();
    streamRef.current = connectDistributedEvents((ev) => {
      if (ev.type === "agent_removed") {
        setAgents((prev) => prev.filter((a) => a.id !== ev.agent_id));
      } else if (ev.type === "agent_updated") {
        setAgents((prev) => {
          const idx = prev.findIndex((a) => a.id === ev.agent.id);
          if (idx < 0) return [...prev, ev.agent];
          const next = [...prev];
          next[idx] = ev.agent;
          return next;
        });
      }
    });
    const timer = setInterval(() => void refresh(), 5000);
    return () => {
      streamRef.current?.unlisten();
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onStartController = async () => {
    setCtrlBusy(true);
    setError(null);
    try {
      const r = await controllerStart(parseInt(port, 10) || 50051);
      setCtrlAddr(r.addr);
      setNotice(format("distributed.controllerStarted", r.addr));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCtrlBusy(false);
    }
  };

  const onStopController = async () => {
    setCtrlBusy(true);
    try {
      await controllerStop();
      setCtrlAddr(null);
      setNotice(t("distributed.controllerStopped"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCtrlBusy(false);
    }
  };

  const onAdd = async () => {
    if (!addr.trim()) return;
    setAdding(true);
    setDialogError(null);
    setError(null);
    try {
      const labelsArr: [string, string][] = labels
        .split(",")
        .map((kv) => kv.trim())
        .filter(Boolean)
        .map((kv) => {
          const [k, v] = kv.split("=");
          return [k, v ?? ""] as [string, string];
        });
      const r = await withTimeout(
        addAgent(addr.trim(), agentId.trim() || undefined, labelsArr, false),
        15000,
        t("distributed.connectTimeout"),
      );
      if (!r.ok && "claimed" in r && r.claimed) {
        // Already claimed by another controller: confirm in the dialog before taking it over
        setClaimConfirm({
          addr: addr.trim(),
          agentId: agentId.trim() || undefined,
          labels: labelsArr,
        });
        return;
      }
      if (r.ok) {
        setAgents((prev) => [
          ...prev.filter((a) => a.id !== r.agent.id),
          r.agent,
        ]);
        setNotice(format("distributed.connectedAgent", r.agent.id));
        setAddr("");
        setAgentId("");
        setLabels("");
        setAddOpen(false);
      }
    } catch (e) {
      setDialogError(e instanceof Error ? e.message : String(e));
    } finally {
      setAdding(false);
    }
  };

  const onForceAdd = async () => {
    if (!claimConfirm) return;
    setAdding(true);
    setDialogError(null);
    setError(null);
    try {
      const r = await withTimeout(
        addAgent(
          claimConfirm.addr,
          claimConfirm.agentId,
          claimConfirm.labels,
          true,
        ),
        15000,
        t("distributed.claimTimeout"),
      );
      setClaimConfirm(null);
      if (r.ok) {
        setAgents((prev) => [
          ...prev.filter((a) => a.id !== r.agent.id),
          r.agent,
        ]);
        setNotice(format("distributed.claimedAgent", r.agent.id));
        setAddr("");
        setAgentId("");
        setLabels("");
        setAddOpen(false);
      }
    } catch (e) {
      setDialogError(e instanceof Error ? e.message : String(e));
    } finally {
      setAdding(false);
    }
  };

  const onAction = async (
    id: string,
    action: "pause" | "resume" | "remove" | "ping",
  ) => {
    setBusyId(id);
    setError(null);
    try {
      await agentAction(id, action);
      if (action === "remove") {
        setAgents((prev) => prev.filter((a) => a.id !== id));
        if ((executionTarget.agentIds ?? []).includes(id)) {
          const rest = (executionTarget.agentIds ?? []).filter((x) => x !== id);
          setExecutionTarget({
            mode: rest.length > 0 ? "agent" : "local",
            agentIds: rest.length > 0 ? rest : null,
          });
        }
      } else {
        await refresh();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div className="flex items-center gap-2">
        <h2 className="text-sm font-semibold">{t("distributed.title")}</h2>
        <span className="text-xs text-muted-foreground">
          {t("distributed.cliHint")}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="sm"
            className="gap-1"
            onClick={() => void refresh()}
          >
            <RefreshCw className="h-3.5 w-3.5" /> {t("distributed.refresh")}
          </Button>
          <Button
            size="sm"
            className="gap-1"
            onClick={() => {
              setDialogError(null);
              setAddOpen(true);
            }}
          >
            <Plus className="h-3.5 w-3.5" /> {t("distributed.addAgent")}
          </Button>
        </div>
      </div>

      {error && (
        <div className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-400">
          ✗ {error}
        </div>
      )}
      {notice && (
        <div className="rounded-md border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-400">
          ✓ {notice}
        </div>
      )}

      {/* Controller (client-mode gRPC server) configuration */}
      <div className="rounded-md border border-border p-3">
        <div className="mb-2 text-xs font-medium">
          {t("distributed.controllerTitle")}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Label className="text-sm text-muted-foreground">
            {t("distributed.port")}
          </Label>
          <Input
            className="h-8 w-24 text-xs"
            type="number"
            value={port}
            disabled={!!ctrlAddr}
            onChange={(e) => setPort(e.target.value)}
          />
          {ctrlAddr ? (
            <>
              <span className="rounded bg-emerald-500/10 px-2 py-1 font-mono text-xs text-emerald-400">
                {format("distributed.running", ctrlAddr)}
              </span>
              <Button
                size="sm"
                variant="outline"
                className="gap-1"
                onClick={onStopController}
                disabled={ctrlBusy}
              >
                {ctrlBusy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <PowerOff className="h-3.5 w-3.5" />
                )}
                {t("distributed.off")}
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              className="gap-1"
              onClick={onStartController}
              disabled={ctrlBusy}
            >
              {ctrlBusy ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Power className="h-3.5 w-3.5" />
              )}
              {t("distributed.on")}
            </Button>
          )}
          <span className="text-xs text-muted-foreground">
            {t("distributed.clientHint")}
          </span>
        </div>
      </div>

      {/* Request execution mode */}
      <div className="rounded-md border border-border p-3">
        <div className="mb-2 text-xs font-medium">
          {t("distributed.execMode")}
        </div>
        <AgentExecutionTarget agents={agents} />
      </div>

      {/* Agent list */}
      <div className="rounded-md border border-border">
        <div className="flex items-center gap-2 border-b border-border px-3 py-2">
          <Server className="h-4 w-4 text-muted-foreground" />
          <span className="text-xs font-medium">
            {t("distributed.agentList")}
          </span>
          <span className="ml-auto text-xs text-muted-foreground">
            {format(
              "distributed.agentCounts",
              agents.length,
              agents.filter((a) => a.state === "idle").length,
              agents.filter((a) => a.state === "running").length,
            )}
          </span>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead className="text-xs text-muted-foreground">
              <tr className="border-b border-border/60">
                <th className="px-3 py-2 font-medium">Agent ID</th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colMode")}
                </th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colAddr")}
                </th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colState")}
                </th>
                <th className="px-2 py-2 font-medium">CPU</th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colMemory")}
                </th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colTags")}
                </th>
                <th className="px-2 py-2 font-medium">
                  {t("distributed.colHeartbeat")}
                </th>
                <th className="px-3 py-2 font-medium">
                  {t("distributed.colActions")}
                </th>
              </tr>
            </thead>
            <tbody>
              {agents.length === 0 && (
                <tr>
                  <td
                    colSpan={9}
                    className="px-3 py-6 text-center text-muted-foreground"
                  >
                    {t("distributed.emptyHint")}
                  </td>
                </tr>
              )}
              {agents.map((a) => (
                <tr
                  key={a.id}
                  className="border-b border-border/40 hover:bg-accent/5"
                >
                  <td className="px-3 py-2 font-medium">
                    {a.id}
                    {a.degraded && (
                      <span className="ml-1 rounded bg-rose-500/15 px-1 text-xs text-rose-400">
                        degraded
                      </span>
                    )}
                    {a.taken_over && (
                      <span className="ml-1 rounded bg-amber-500/15 px-1 text-xs text-amber-400">
                        {t("distributed.claimed")}
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-2">
                    <span
                      className={`rounded px-1.5 py-0.5 text-xs font-semibold ${a.mode === "server" ? "bg-violet-500/15 text-violet-400" : "bg-cyan-500/15 text-cyan-400"}`}
                    >
                      {a.mode === "server" ? "server" : "client"}
                    </span>
                  </td>
                  <td className="px-2 py-2 font-mono text-xs">
                    {a.addr || t("distributed.viaController")}
                  </td>
                  <td className="px-2 py-2">
                    <StateBadge state={a.state} />
                  </td>
                  <td className="px-2 py-2">
                    {a.resource ? (
                      <div className="flex items-center gap-1.5">
                        <UsageBar percent={a.resource.cpu_percent} />
                        <Sparkline
                          history={a.resource_history.map((r) => r.cpu_percent)}
                        />
                      </div>
                    ) : (
                      <span className="text-xs text-muted-foreground">
                        {t("distributed.awaitingReport")}
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-2">
                    {a.resource ? (
                      <div className="flex items-center gap-1.5">
                        <UsageBar percent={a.resource.mem_percent} />
                        <Sparkline
                          history={a.resource_history.map((r) => r.mem_percent)}
                        />
                      </div>
                    ) : (
                      <span className="text-xs text-muted-foreground">
                        {t("distributed.awaitingReport")}
                      </span>
                    )}
                  </td>
                  <td className="px-2 py-2">
                    <div className="flex max-w-40 flex-wrap gap-1">
                      {a.labels.map(([k, v]) => (
                        <span
                          key={k}
                          className="rounded bg-muted px-1 py-px text-xs text-muted-foreground"
                        >
                          {k}={v}
                        </span>
                      ))}
                    </div>
                  </td>
                  <td className="px-2 py-2 text-xs text-muted-foreground">
                    {fmtTime(a.last_heartbeat_ms)}
                  </td>
                  <td className="px-3 py-2">
                    <div className="flex items-center gap-1">
                      {a.state === "paused" ? (
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="h-6 w-6"
                          title={t("distributed.resume")}
                          disabled={busyId === a.id}
                          onClick={() => void onAction(a.id, "resume")}
                        >
                          <Play className="h-3.5 w-3.5 text-emerald-400" />
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="h-6 w-6"
                          title={t("distributed.pause")}
                          disabled={busyId === a.id || a.state !== "idle"}
                          onClick={() => void onAction(a.id, "pause")}
                        >
                          <Pause className="h-3.5 w-3.5 text-amber-400" />
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="h-6 w-6"
                        title={t("distributed.testConnection")}
                        disabled={busyId === a.id}
                        onClick={() => void onAction(a.id, "ping")}
                      >
                        <Zap className="h-3.5 w-3.5 text-warning" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="h-6 w-6"
                        title={t("distributed.remove")}
                        disabled={busyId === a.id}
                        onClick={() => void onAction(a.id, "remove")}
                      >
                        <Trash2 className="h-3.5 w-3.5 text-rose-400" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      {loading && (
        <div className="text-center text-xs text-muted-foreground">
          {t("distributed.loading")}
        </div>
      )}

      {/* Add Agent dialog */}
      <Dialog
        open={addOpen}
        onOpenChange={(o) => {
          setAddOpen(o);
          if (!o) setDialogError(null);
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("distributed.newAgentTitle")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">
                {t("distributed.addrLabel")}
              </Label>
              <Input
                className="h-8 text-xs"
                placeholder={t("distributed.addrPlaceholder")}
                value={addr}
                onChange={(e) => setAddr(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">
                {t("distributed.agentIdLabel")}
              </Label>
              <Input
                className="h-8 text-xs"
                placeholder={t("distributed.agentIdPlaceholder")}
                value={agentId}
                onChange={(e) => setAgentId(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-muted-foreground">
                {t("distributed.tagsLabel")}
              </Label>
              <Input
                className="h-8 text-xs"
                placeholder="zone=cn,group=g1"
                value={labels}
                onChange={(e) => setLabels(e.target.value)}
              />
            </div>
          </div>
          {dialogError && (
            <div className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-400">
              ✗ {dialogError}
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setAddOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              className="gap-1"
              onClick={onAdd}
              disabled={adding || !addr.trim()}
            >
              {adding ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Plus className="h-3.5 w-3.5" />
              )}
              {t("distributed.connect")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Second confirmation when another controller already holds the agent */}
      <Dialog
        open={claimConfirm !== null}
        onOpenChange={(o) => {
          if (!o) {
            setClaimConfirm(null);
            setDialogError(null);
          }
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("distributed.claimTitle")}</DialogTitle>
          </DialogHeader>
          <div className="py-2 text-xs text-muted-foreground">
            {format("distributed.claimMessage", claimConfirm?.addr ?? "")}
          </div>
          {dialogError && (
            <div className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-400">
              ✗ {dialogError}
            </div>
          )}
          <DialogFooter>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setClaimConfirm(null);
                setDialogError(null);
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              className="gap-1"
              onClick={onForceAdd}
              disabled={adding}
            >
              {adding ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Plus className="h-3.5 w-3.5" />
              )}
              {t("distributed.claim")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
