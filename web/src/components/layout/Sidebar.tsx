import { PanelLeft } from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAppStore } from "@/store/useStore";
import { WORKSPACE_MODULES, type ModuleMeta } from "./modules";

export function Sidebar() {
  const active = useAppStore((s) => s.activeModule);
  const setActive = useAppStore((s) => s.setActiveModule);
  const collapsed = useAppStore((s) => s.ui.sidebarCollapsed);
  const toggle = useAppStore((s) => s.toggleSidebar);
  const { t } = useT();

  const renderBtn = (m: ModuleMeta) => {
    const Icon = m.icon;
    const isActive = active === m.key;
    const label = t(m.i18nLabel as any) as string;
    const btn = (
      <button
        key={m.key}
        onClick={() => setActive(m.key)}
        className={cn(
          "flex items-center gap-3 rounded-md px-2.5 py-2 text-sm transition-colors",
          collapsed && "justify-center px-0",
          isActive
            ? "bg-sidebar-accent text-sidebar-accent-foreground"
            : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground",
        )}
      >
        <Icon className={cn("h-5 w-5 shrink-0", isActive && m.accent)} />
        {!collapsed && <span className="truncate">{label}</span>}
      </button>
    );
    return collapsed ? (
      <Tooltip key={m.key}>
        <TooltipTrigger asChild>{btn}</TooltipTrigger>
        <TooltipContent side="right">{label}</TooltipContent>
      </Tooltip>
    ) : (
      btn
    );
  };

  return (
    <aside
      className={cn(
        "flex h-full flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-all duration-200",
        collapsed ? "w-15" : "w-50",
      )}
    >
      {/* Pure workspace-scoped module navigation (app-level features moved to the top menu bar) */}
      <nav className="flex flex-1 flex-col gap-1 overflow-y-auto p-2">
        {WORKSPACE_MODULES.map(renderBtn)}
      </nav>

      {/* Collapse-button area: the same height as the right footer (StatusBar h-7) for visual alignment */}
      <div className="flex h-7 shrink-0 items-center border-t border-sidebar-border px-2">
        <Button
          variant="ghost"
          size={collapsed ? "icon" : "sm"}
          className="h-7 w-full justify-center gap-2 text-sidebar-foreground/70"
          onClick={toggle}
        >
          <PanelLeft
            className={cn(
              "h-4 w-4 transition-transform",
              collapsed && "rotate-180",
            )}
          />
          {!collapsed && <span>{t("common.collapse" as any)}</span>}
        </Button>
      </div>
    </aside>
  );
}
