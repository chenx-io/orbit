// New case dialog: name + priority (priority is fixed at creation time, defaults to P2).
import { useEffect, useState } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useT } from "@/lib/i18n";
import type { ScenarioPriority } from "@/data/types";

const PRIORITIES: ScenarioPriority[] = ["p0", "p1", "p2", "p3"];

export function NewCaseDialog({
  open,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  onConfirm: (name: string, priority: ScenarioPriority) => void;
  onCancel: () => void;
}) {
  const { t } = useT();
  const [name, setName] = useState("");
  const [priority, setPriority] = useState<ScenarioPriority>("p2");

  useEffect(() => {
    if (open) {
      setName("");
      setPriority("p2");
    }
  }, [open]);

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel();
      }}
    >
      <DialogContent className="max-w-xs">
        <DialogHeader>
          <DialogTitle className="text-sm">{t("scenario.newCase")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-2.5">
          <Input
            value={name}
            placeholder={t("scenario.newPlaceholder")}
            autoFocus
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && name.trim())
                onConfirm(name.trim(), priority);
            }}
          />
          <div className="flex items-center gap-2">
            <span className="shrink-0 text-xs text-muted-foreground">
              {t("scenario.priority")}
            </span>
            <Select
              value={priority}
              onValueChange={(v) => setPriority(v as ScenarioPriority)}
            >
              <SelectTrigger size="sm" className="h-8 flex-1 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PRIORITIES.map((p) => (
                  <SelectItem key={p} value={p} className="text-xs">
                    {p.toUpperCase()}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter>
          <Button size="sm" variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button
            size="sm"
            disabled={!name.trim()}
            onClick={() => name.trim() && onConfirm(name.trim(), priority)}
          >
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
