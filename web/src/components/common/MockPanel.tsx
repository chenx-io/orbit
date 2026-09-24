import { useState } from "react";
import { Server, Play, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import { startMockServer, stopMockServer, getMockRules } from "@/lib/bridge";

export function MockPanel() {
  const open = useAppStore((s) => s.ui.mockOpen);
  const setOpen = useAppStore((s) => s.setMockOpen);
  const activeWorkspaceId = useAppStore((s) => s.activeWorkspaceId);

  const [running, setRunning] = useState(false);
  const [port, setPort] = useState(8787);
  const { t } = useT();

  const toggle = async () => {
    if (running) {
      await stopMockServer();
      setRunning(false);
    } else {
      // Log the saved rules before starting (filtered by the current workspace)
      const wsId = activeWorkspaceId ?? undefined;
      const rules = await getMockRules(wsId);
      console.log("[mock start] saved rules:", JSON.stringify(rules, null, 2));
      const ok = await startMockServer(port, wsId);
      console.log("[mock start] server started:", ok);
      setRunning(ok);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Server className="h-4 w-4 text-primary" /> {t("mock.title")}
          </DialogTitle>
          <DialogDescription>{t("mock.desc")}</DialogDescription>
        </DialogHeader>

        <div className="flex items-center gap-2 rounded-lg border border-border bg-accent/5 p-3">
          <div className="flex items-center gap-1.5">
            <span
              className={cn(
                "h-2.5 w-2.5 rounded-full",
                running
                  ? "bg-emerald-400 shadow-[0_0_8px] shadow-emerald-400"
                  : "bg-muted-foreground/40",
              )}
            />
            <span className="text-xs font-medium">
              {running ? t("mock.running") : t("mock.stopped")}
            </span>
          </div>
          <Label className="ml-2 text-xs text-muted-foreground">
            {t("mock.port")}
          </Label>
          <Input
            type="number"
            value={port}
            onChange={(e) => setPort(parseInt(e.target.value, 10) || 8787)}
            className="h-8 w-24 text-xs"
            disabled={running}
          />
          <Button
            size="sm"
            variant={running ? "destructive" : "default"}
            onClick={toggle}
            className="ml-auto gap-1.5"
          >
            {running ? (
              <>
                <Square className="h-3.5 w-3.5" /> {t("mock.stop")}
              </>
            ) : (
              <>
                <Play className="h-3.5 w-3.5" /> {t("mock.start")}
              </>
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
