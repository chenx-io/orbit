// Global overlays shared by AppShell / AppModuleShell / WorkspacePicker (mock / import / environment management).
import { TooltipProvider } from "@/components/ui/tooltip";
import { MockPanel } from "@/components/common/MockPanel";
import { ImportDialog } from "@/components/import/ImportDialog";
import { ScenarioImportDialog } from "@/components/import/ScenarioImportDialog";
import { EnvironmentManager } from "@/components/environment/EnvironmentManager";

export function GlobalOverlays() {
  return (
    <TooltipProvider delayDuration={150}>
      <MockPanel />
      <ImportDialog />
      <ScenarioImportDialog />
      <EnvironmentManager />
    </TooltipProvider>
  );
}
