// Scenario rename dialog.
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

export function RenameDialog({
  open,
  name,
  onConfirm,
  onCancel,
  t,
  title,
}: {
  open: boolean;
  name: string;
  onConfirm: (v: string) => void;
  onCancel: () => void;
  t: (k: string) => string;
  title?: string;
}) {
  const [val, setVal] = useState(name);
  useEffect(() => {
    if (open) setVal(name);
  }, [open, name]);
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel();
      }}
    >
      <DialogContent className="max-w-xs">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {title ?? t("scenario.rename")}
          </DialogTitle>
        </DialogHeader>
        <Input
          value={val}
          onChange={(e) => setVal(e.target.value)}
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Enter" && val.trim()) {
              onConfirm(val.trim());
            }
          }}
        />
        <DialogFooter>
          <Button size="sm" variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={() => val.trim() && onConfirm(val.trim())}>
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
