// App-level module shell (distributed / plugins): workspace-independent, showing the module content right below the menu bar.
// The App layer decides whether to render it from activeModule (a peer of AppShell / WorkspacePicker).
import { MenuBar } from "./MenuBar";
import { StatusBar } from "./StatusBar";
import { WindowResizeEdges } from "./WindowChrome";
import { DistributedModule } from "@/components/modules/DistributedModule";
import { PluginsModule } from "@/components/modules/PluginsModule";
import { useAppStore } from "@/store/useStore";

export function AppModuleShell() {
  const activeModule = useAppStore((s) => s.activeModule);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <WindowResizeEdges />
      <MenuBar />
      <div className="min-h-0 flex-1 overflow-hidden">
        {activeModule === "distributed" ? (
          <DistributedModule />
        ) : (
          <PluginsModule />
        )}
      </div>
      <StatusBar />
    </div>
  );
}
