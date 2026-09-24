import { Database, Radio, Gauge } from "lucide-react";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { collectRefRequestIds } from "@/lib/persistence";

export function StatusBar() {
  const environments = useAppStore((s) => s.environments);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const collections = useAppStore((s) => s.collections);
  const scenarios = useAppStore((s) => s.scenarios);
  const loadRunning = useAppStore((s) => s.loadTestRunning);
  const active = environments.find((e) => e.id === activeEnvId);
  const { t } = useT();
  // Request count = the requests referenced by this workspace's collection trees / scenarios (the store's global requests include other workspaces)
  const interfaceCount = collectRefRequestIds(collections, scenarios).size;

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-card/40 px-3 text-xs text-muted-foreground">
      <span className="flex items-center gap-1">
        <Database className="h-3 w-3" /> {interfaceCount}{" "}
        {t("status.interfaces")}
      </span>
      <span className="flex items-center gap-1">
        <Radio className="h-3 w-3 text-emerald-400" /> {t("status.env")}：
        {active?.name ?? t("status.envNone")}
      </span>
      <span className="flex items-center gap-1">
        <Gauge className={`h-3 w-3 ${loadRunning ? "text-amber-400" : ""}`} />
        {t("status.loadTest")}：
        {loadRunning ? t("status.loadRunning") : t("status.loadIdle")}
      </span>
    </footer>
  );
}
