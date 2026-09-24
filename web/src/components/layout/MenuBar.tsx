// Top menu bar (the title-bar layer, a single row):
// Left menu: workspace (dropdown) / distributed / plugins (click to switch modules directly)
// Middle: draggable area; right: environment / mock + theme / language / syntax reference / data management (flat) + window controls
// Tauri decorations:false: the container and blank areas carry data-tauri-drag-region (drag the window)
// and buttons carry data-tauri-drag-region="false" (clickable)
import { useState } from "react";
import {
  Check,
  LayoutGrid,
  Network,
  ChevronDown,
  Sun,
  Moon,
  Monitor,
  Languages,
  Database,
  BookOpen,
  Server,
  HardDrive,
  Sparkles,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import { useT, type TKey } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import { DataManageDialog } from "@/components/common/DataManageDialog";
import { ReferenceDialog } from "@/components/common/ReferenceDialog";
import { EnvSwitcher } from "./EnvSwitcher";
import { WindowControls } from "./WindowChrome";

const THEME_OPTIONS: { key: "light" | "dark" | "system"; icon: typeof Sun }[] =
  [
    { key: "light", icon: Sun },
    { key: "dark", icon: Moon },
    { key: "system", icon: Monitor },
  ];

const menuBtnCls =
  "flex h-6 items-center gap-1 rounded px-2 text-xs font-medium text-muted-foreground hover:bg-accent/15 hover:text-foreground";

export function MenuBar() {
  const { t } = useT();
  const workspaces = useAppStore((s) => s.workspaces);
  const activeWorkspaceId = useAppStore((s) => s.activeWorkspaceId);
  const setActiveWorkspace = useAppStore((s) => s.setActiveWorkspace);
  const returnToWorkspaceList = useAppStore((s) => s.returnToWorkspaceList);
  const setActiveModule = useAppStore((s) => s.setActiveModule);
  const setMockOpen = useAppStore((s) => s.setMockOpen);
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const locale = useAppStore((s) => s.locale);
  const setLocale = useAppStore((s) => s.setLocale);
  const aiDrawerOpen = useAppStore((s) => s.aiDrawerOpen);
  const toggleAiDrawer = useAppStore((s) => s.aiToggleDrawer);

  const [dataOpen, setDataOpen] = useState(false);
  const [refOpen, setRefOpen] = useState(false);
  const curTheme =
    THEME_OPTIONS.find((o) => o.key === theme) ?? THEME_OPTIONS[1];
  // Selection page (no workspace entered): workspace-scoped tools (environment / mock) are hidden
  const isPicker = activeWorkspaceId === null;

  return (
    <div
      data-tauri-drag-region
      className="flex h-10 shrink-0 select-none items-center gap-0.5 border-b border-border bg-card/60 pl-2"
    >
      {/* Workspace */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button data-tauri-drag-region="false" className={menuBtnCls}>
            <LayoutGrid className="h-3.5 w-3.5" />
            {t("menu.workspace")}
            <ChevronDown className="h-3 w-3" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-56">
          <DropdownMenuLabel>{t("workspace.switchTo")}</DropdownMenuLabel>
          {workspaces.map((ws) => (
            <DropdownMenuItem
              key={ws.id}
              onSelect={() => setActiveWorkspace(ws.id)}
            >
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: ws.color ?? "#71717a" }}
              />
              <span className="min-w-0 flex-1 truncate">{ws.name}</span>
              {ws.id === activeWorkspaceId && (
                <Check className="h-3.5 w-3.5 shrink-0" />
              )}
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={returnToWorkspaceList}>
            <LayoutGrid className="h-3.5 w-3.5" /> {t("workspace.manageList")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {/* Distributed (app-level: opens directly, no menu) */}
      <button
        data-tauri-drag-region="false"
        className={menuBtnCls}
        onClick={() => setActiveModule("distributed")}
      >
        <Network className="h-3.5 w-3.5" />
        {t("nav.distributed")}
      </button>

      {/* Script library (workspace-level: the entry for maintaining reusable action templates; hidden on the selection page) */}
      {!isPicker && (
        <button
          data-tauri-drag-region="false"
          className={menuBtnCls}
          onClick={() => setActiveModule("actionlib")}
        >
          <BookOpen className="h-3.5 w-3.5" />
          {t("nav.actionlib")}
        </button>
      )}

      {/* Data sources (an app-level global entry: DB/Redis connection management, on the same level as workspace/distributed) */}
      <button
        data-tauri-drag-region="false"
        className={menuBtnCls}
        onClick={() => setActiveModule("datasource")}
      >
        <HardDrive className="h-3.5 w-3.5" />
        {t("nav.datasource")}
      </button>

      {/* Draggable area (fills the rest; double-click maximizes) */}
      <div data-tauri-drag-region className="min-w-0 flex-1" />

      {/* Workspace-scoped tools (environment / mock) are hidden on the selection page */}
      {!isPicker && (
        <>
          {/* Environment switcher */}
          <div data-tauri-drag-region="false">
            <EnvSwitcher />
          </div>
          {/* Mock */}
          <button
            data-tauri-drag-region="false"
            onClick={() => setMockOpen(true)}
            className={menuBtnCls}
          >
            <Server className="h-3.5 w-3.5" /> Mock
          </button>
        </>
      )}

      {/* AI assistant (a global right drawer; shortcut Ctrl/Cmd+L) */}
      <button
        data-tauri-drag-region="false"
        className={cn(
          menuBtnCls,
          aiDrawerOpen && "bg-accent/20 text-foreground",
        )}
        title={t("ai.toggle")}
        onClick={toggleAiDrawer}
      >
        <Sparkles className="h-3.5 w-3.5 text-violet-400" />
        {t("ai.title")}
      </button>

      {/* Syntax reference */}
      <button
        data-tauri-drag-region="false"
        className={menuBtnCls}
        title={t("reference.openEntry")}
        onClick={() => setRefOpen(true)}
      >
        <BookOpen className="h-3.5 w-3.5" />
      </button>

      {/* Data management */}
      <button
        data-tauri-drag-region="false"
        className={menuBtnCls}
        title={t("persist.manage")}
        onClick={() => setDataOpen(true)}
      >
        <Database className="h-3.5 w-3.5" />
      </button>

      {/* Theme */}
      <Popover>
        <PopoverTrigger asChild>
          <button
            data-tauri-drag-region="false"
            className={menuBtnCls}
            title={t("common.theme")}
          >
            <curTheme.icon className="h-3.5 w-3.5" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-32 p-1">
          {THEME_OPTIONS.map((opt) => (
            <button
              key={opt.key}
              onClick={() => setTheme(opt.key)}
              className={cn(
                "flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent/15",
                theme === opt.key && "bg-accent/20 text-foreground",
              )}
            >
              <opt.icon className="h-3.5 w-3.5" />
              {t(`common.${opt.key}` as any)}
            </button>
          ))}
        </PopoverContent>
      </Popover>

      {/* Language */}
      <Popover>
        <PopoverTrigger asChild>
          <button
            data-tauri-drag-region="false"
            className={menuBtnCls}
            title={t("common.language")}
          >
            <Languages className="h-3.5 w-3.5" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-32 p-1">
          {(["zh-CN", "en-US"] as const).map((l) => (
            <button
              key={l}
              onClick={() => setLocale(l)}
              className={cn(
                "flex w-full items-center rounded px-2 py-1.5 text-xs hover:bg-accent/15",
                locale === l && "bg-accent/20 text-foreground",
              )}
            >
              {t(`uiLocale.${l}` as TKey)}
            </button>
          ))}
        </PopoverContent>
      </Popover>

      {/* Window controls (drawn by Tauri; hidden on the web) */}
      <WindowControls />

      <DataManageDialog open={dataOpen} onOpenChange={setDataOpen} />
      <ReferenceDialog open={refOpen} onOpenChange={setRefOpen} />
    </div>
  );
}
