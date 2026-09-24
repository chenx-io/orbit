// Request / load-test execution target picker: local, or via one or more agents (null = spread across all available agents).
import { ChevronsUpDown } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { useT, type TKey } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import type { AgentInfo } from "@/lib/bridge/distributed";

/** i18n key for an agent state (falls back to the raw state string). */
const STATE_LABEL_KEY: Record<AgentInfo["state"], TKey> = {
  idle: "agent.state.idle",
  running: "agent.state.running",
  paused: "agent.state.paused",
  offline: "agent.state.offline",
};

export function AgentExecutionTarget({ agents }: { agents: AgentInfo[] }) {
  const { t, format } = useT();
  const executionTarget = useAppStore((s) => s.executionTarget);
  const setExecutionTarget = useAppStore((s) => s.setExecutionTarget);

  const usable = agents.filter(
    (a) => a.state !== "paused" && a.state !== "offline" && !a.degraded,
  );
  const selected = executionTarget.agentIds ?? [];
  // null or an empty array = spread across all available agents
  const allSelected = executionTarget.mode === "agent" && selected.length === 0;

  const pickAll = () => setExecutionTarget({ mode: "agent", agentIds: null });
  const pickNone = () =>
    setExecutionTarget({
      mode: "agent",
      agentIds: usable.length > 0 ? [usable[0].id] : [],
    });
  const toggleAgent = (id: string) => {
    if (allSelected) {
      // Switching from "all" to an explicit subset: start from the agent just clicked
      setExecutionTarget({ mode: "agent", agentIds: [id] });
      return;
    }
    const next = selected.includes(id)
      ? selected.filter((x) => x !== id)
      : [...selected, id];
    setExecutionTarget({
      mode: "agent",
      agentIds: next.length > 0 ? next : null,
    });
  };

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex cursor-pointer items-center gap-1.5 text-xs">
          <input
            type="radio"
            className="accent-primary"
            checked={executionTarget.mode === "local"}
            onChange={() =>
              setExecutionTarget({ mode: "local", agentIds: null })
            }
          />
          {t("agent.exec.local")}
        </label>
        <label className="flex cursor-pointer items-center gap-1.5 text-xs">
          <input
            type="radio"
            className="accent-primary"
            checked={executionTarget.mode === "agent"}
            onChange={() =>
              setExecutionTarget({
                mode: "agent",
                agentIds: usable.length > 0 ? [usable[0].id] : [],
              })
            }
          />
          {t("agent.exec.viaAgent")}
        </label>
        {executionTarget.mode === "agent" && (
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" size="sm" className="h-7 gap-1 text-xs">
                {allSelected
                  ? t("agent.exec.allAgents")
                  : format("agent.exec.selectedCount", selected.length)}
                <ChevronsUpDown className="h-3 w-3 opacity-60" />
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-64 p-2">
              <div className="mb-1 px-1 text-xs font-medium text-muted-foreground">
                {t("agent.exec.pickHint")}
              </div>
              <label className="flex cursor-pointer items-center gap-2 rounded px-1 py-1 text-xs hover:bg-muted/60">
                <Checkbox
                  checked={allSelected}
                  onCheckedChange={(c) => (c ? pickAll() : pickNone())}
                />
                {usable.length > 0
                  ? format("agent.exec.allAgentsWithCount", usable.length)
                  : t("agent.exec.allAgents")}
              </label>
              <div className="my-1 h-px bg-border" />
              <div className="max-h-44 space-y-0.5 overflow-auto">
                {agents.map((a) => {
                  const on = allSelected || selected.includes(a.id);
                  return (
                    <label
                      key={a.id}
                      className="flex cursor-pointer items-center gap-2 rounded px-1 py-1 text-xs hover:bg-muted/60"
                    >
                      <Checkbox
                        checked={on}
                        onCheckedChange={() => toggleAgent(a.id)}
                      />
                      <span className="min-w-0 flex-1 truncate">{a.id}</span>
                      <Badge
                        variant="outline"
                        className={`shrink-0 px-1.5 text-xs ${
                          a.state === "idle"
                            ? "text-muted-foreground"
                            : a.state === "running"
                              ? "text-emerald-400"
                              : a.state === "paused"
                                ? "text-amber-400"
                                : "text-rose-400"
                        }`}
                      >
                        {t(STATE_LABEL_KEY[a.state])}
                      </Badge>
                    </label>
                  );
                })}
                {agents.length === 0 && (
                  <div className="px-1 py-2 text-xs text-muted-foreground">
                    {t("agent.exec.noneYet")}
                  </div>
                )}
              </div>
            </PopoverContent>
          </Popover>
        )}
      </div>
      {executionTarget.mode === "agent" && (
        <div className="text-xs text-muted-foreground">
          {agents.length === 0
            ? t("agent.exec.noneUsable")
            : allSelected
              ? format("agent.exec.spreadHint", usable.length)
              : format("agent.exec.dispatchHint", selected.length)}
        </div>
      )}
    </div>
  );
}
