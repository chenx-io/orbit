// Automation module: left column tabs (scenario tree / suite list) plus the right column (scenario editor / suite details / empty state).
// Scenario editing lives in automation/ScenarioEditor and suites in automation/SuitePanel.
import { useState } from "react";
import { Layers, FileCode2 } from "lucide-react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { ScenarioTree } from "./automation/ScenarioTree";
import { SuiteList, SuiteDetail } from "./automation/SuitePanel";
import { ScenarioEditor } from "./automation/ScenarioEditor";

type SidebarTab = "cases" | "suites";

export function AutomationModule() {
  const { t } = useT();
  const [tab, setTab] = useState<SidebarTab>("cases");
  const activeScenarioId = useAppStore((s) => s.activeScenarioId);
  const activeSuiteId = useAppStore((s) => s.activeSuiteId);

  const tabBtn = (active: boolean): string =>
    cn(
      "flex items-center gap-1 rounded px-2 py-1 text-xs font-medium transition-colors",
      active
        ? "bg-accent/20 text-foreground"
        : "text-muted-foreground hover:text-foreground",
    );

  return (
    <PanelGroup
      direction="horizontal"
      autoSaveId="automation-layout"
      className="h-full"
    >
      {/* ─── Left column: scenario tree / suite list ─── */}
      <Panel
        defaultSize={25}
        minSize={20}
        className="flex min-h-0 flex-col border-r border-border bg-card/40"
      >
        <div className="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1.5">
          <button
            className={tabBtn(tab === "cases")}
            onClick={() => setTab("cases")}
          >
            <FileCode2 className="h-3.5 w-3.5" />
            {t("scenario.tab.cases")}
          </button>
          <button
            className={tabBtn(tab === "suites")}
            onClick={() => setTab("suites")}
          >
            <Layers className="h-3.5 w-3.5" />
            {t("scenario.tab.suites")}
          </button>
        </div>
        <div className="min-h-0 flex-1">
          {tab === "cases" ? <ScenarioTree /> : <SuiteList />}
        </div>
      </Panel>

      <PanelResizeHandle className="w-1 bg-border transition-colors hover:bg-primary" />

      {/* ─── Right column ─── */}
      <Panel minSize={40} className="min-h-0">
        {activeSuiteId ? (
          <SuiteDetail suiteId={activeSuiteId} />
        ) : activeScenarioId ? (
          <ScenarioEditor scenarioId={activeScenarioId} />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {t("suite.noSelection")}
          </div>
        )}
      </Panel>
    </PanelGroup>
  );
}
