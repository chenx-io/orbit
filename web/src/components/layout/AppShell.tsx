import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Sidebar } from "./Sidebar";
import { MenuBar } from "./MenuBar";
import { StatusBar } from "./StatusBar";
import { WindowResizeEdges } from "./WindowChrome";
import { TabBar } from "./TabBar";
import { CollectionTree } from "./collection-tree/CollectionTree";
import { InterfaceModule } from "@/components/modules/InterfaceModule";
import { ModelEditor } from "@/components/modules/ModelEditor";
import { AutomationModule } from "@/components/modules/AutomationModule";
import { LoadModule } from "@/components/modules/LoadModule";
import { PerformanceModule } from "@/components/modules/PerformanceModule";
import { PluginsModule } from "@/components/modules/PluginsModule";
import { HistoryModule } from "@/components/modules/HistoryModule";
import { DistributedModule } from "@/components/modules/DistributedModule";
import { DataSourceModule } from "@/components/modules/DataSourceModule";
import { ActionLibraryModule } from "@/components/modules/ActionLibraryModule";
import { AiDrawer } from "@/components/ai/AiDrawer";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import type { ModuleKey } from "@/data/types";

const TREE_MODULES: ModuleKey[] = ["api"];
/** App-level global modules: no workspace sidebar, with the content filling the width (same as distributed) */
const FULL_WIDTH_MODULES: ModuleKey[] = [
  "datasource",
  "distributed",
  "plugins",
  // Script library: the library name + code + reference list need a full-width screen (the same "config entity management" shape as data sources)
  "actionlib",
];

/** Request-manager list width: the existing w-65 (260px) is the minimum/default, only expandable to the right */
const TREE_MIN_WIDTH = 260;
/** Expansion cap: the maximum width is twice the minimum */
const TREE_MAX_WIDTH = TREE_MIN_WIDTH * 2;

function ActiveModule({ module }: { module: ModuleKey }) {
  switch (module) {
    case "api":
      return <InterfaceModule />;
    case "automation":
      return <AutomationModule />;
    case "load":
      return <LoadModule />;
    case "performance":
      return <PerformanceModule />;
    case "plugins":
      return <PluginsModule />;
    case "history":
      return <HistoryModule />;
    case "distributed":
      return <DistributedModule />;
    case "datasource":
      return <DataSourceModule />;
    case "actionlib":
      return <ActionLibraryModule />;
    default:
      return null;
  }
}

export function AppShell() {
  const { t } = useT();
  const activeModule = useAppStore((s) => s.activeModule);
  const sidebarTab = useAppStore((s) => s.sidebarTab);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const loadPanelOpen = useAppStore((s) => s.loadTestPanelOpen);
  const setLoadPanelOpen = useAppStore((s) => s.setLoadPanelOpen);
  const toggleAiDrawer = useAppStore((s) => s.aiToggleDrawer);

  // Global shortcut Ctrl/Cmd+L: open / collapse the AI drawer
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "l") return;
      e.preventDefault();
      toggleAiDrawer();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggleAiDrawer]);

  const showTree = TREE_MODULES.includes(activeModule);
  const showModelEditor =
    activeModule === "api" &&
    (activeModelId !== null || sidebarTab === "models");

  // Main content (tab bar + module view): reused when the tree panel is present
  const mainContent = (
    <>
      {activeModule === "api" && !showModelEditor && <TabBar />}
      <main className="min-h-0 flex-1 overflow-hidden">
        {showModelEditor ? (
          <ModelEditor />
        ) : (
          <ActiveModule module={activeModule} />
        )}
      </main>
    </>
  );

  // Request-manager list width (persisted to localStorage, following the req/resp layout style)
  const [treeWidth, setTreeWidth] = useState<number>(() => {
    try {
      const saved = localStorage.getItem("orbit:collection-tree-width");
      const n = saved ? Number(saved) : NaN;
      return Number.isFinite(n) && n >= TREE_MIN_WIDTH
        ? Math.min(n, TREE_MAX_WIDTH)
        : TREE_MIN_WIDTH;
    } catch {
      return TREE_MIN_WIDTH;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem("orbit:collection-tree-width", String(treeWidth));
    } catch {
      /* ignore */
    }
  }, [treeWidth]);

  // Drag to resize: pointer capture keeps tracking after the pointer leaves the handle
  const treeResize = useRef<{ startX: number; startWidth: number } | null>(
    null,
  );
  const onTreeResizeDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    treeResize.current = { startX: e.clientX, startWidth: treeWidth };
    e.currentTarget.setPointerCapture(e.pointerId);
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
  };
  const onTreeResizeMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const r = treeResize.current;
    if (!r) return;
    const next = r.startWidth + (e.clientX - r.startX);
    setTreeWidth(Math.min(Math.max(next, TREE_MIN_WIDTH), TREE_MAX_WIDTH));
  };
  const onTreeResizeUp = (e: React.PointerEvent<HTMLDivElement>) => {
    treeResize.current = null;
    document.body.style.userSelect = "";
    document.body.style.cursor = "";
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };
  // Keyboard-adjustable (accessibility): the left/right arrow keys change it by ±20px
  const onTreeResizeKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      setTreeWidth((w) => Math.max(TREE_MIN_WIDTH, w - 20));
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      setTreeWidth((w) => Math.min(TREE_MAX_WIDTH, w + 20));
    }
  };

  return (
    <TooltipProvider delayDuration={150}>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
        {/* Frameless window: edge / corner resize (Tauri only) */}
        <WindowResizeEdges />

        {/* Menu bar (the very top of the window: menu + search + environment/mock + window controls) */}
        <MenuBar />

        <div className="flex min-h-0 flex-1 overflow-hidden">
          {!FULL_WIDTH_MODULES.includes(activeModule) && <Sidebar />}

          <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
            <div className="flex min-h-0 flex-1 overflow-hidden">
              {showTree && (
                <>
                  <div
                    style={{ width: treeWidth }}
                    className="flex h-full shrink-0 flex-col overflow-hidden"
                  >
                    <CollectionTree />
                  </div>
                  {/* Drag handle: widen the request-manager list */}
                  <div
                    role="separator"
                    aria-orientation="vertical"
                    aria-label={t("appShell.resizeListWidth")}
                    title={t("appShell.dragToResize")}
                    tabIndex={0}
                    onPointerDown={onTreeResizeDown}
                    onPointerMove={onTreeResizeMove}
                    onPointerUp={onTreeResizeUp}
                    onKeyDown={onTreeResizeKeyDown}
                    className="w-1 shrink-0 cursor-col-resize bg-border transition-colors hover:bg-primary/70 focus-visible:bg-primary focus-visible:outline-none"
                  />
                </>
              )}

              <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
                {mainContent}
              </div>
            </div>

            <StatusBar />
          </div>

          {/* AI assistant (right docked panel): on the same level as the sidebar / request tree / main content,
              it compresses the main area instead of floating above it, so it must be a child of the same flex row */}
          <AiDrawer />
        </div>

        {/* Load-test floating panel (when not in the load module) */}
        {loadPanelOpen && activeModule !== "load" && (
          <div className="fixed bottom-4 right-4 z-30 flex h-[72vh] w-115 flex-col overflow-hidden rounded-lg border border-border bg-card shadow-2xl">
            <div className="flex items-center justify-between border-b border-border px-3 py-2">
              <span className="text-sm font-semibold">
                {t("load.consoleTitle")}
              </span>
              <button
                onClick={() => setLoadPanelOpen(false)}
                className="text-muted-foreground hover:text-foreground"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="min-h-0 flex-1">
              <LoadModule />
            </div>
          </div>
        )}
      </div>
    </TooltipProvider>
  );
}
