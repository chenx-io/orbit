// Edit the connection config of a connection-type collection (the single source of truth at connection level).
import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import type { ConnectionConfig } from "@/data/types";
import { ConnectionConfigForm } from "./ConnectionConfigForm";

export function ConnectionEditDialog({
  open,
  initial,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  initial: ConnectionConfig | undefined;
  onOpenChange: (o: boolean) => void;
  onConfirm: (config: ConnectionConfig) => void;
}) {
  const { t } = useT();
  const [conn, setConn] = useState<ConnectionConfig>({});

  useEffect(() => {
    if (open) setConn(initial ?? {});
  }, [open, initial]);

  const canSubmit = (conn.url ?? "").trim().length > 0;

  const submit = () => {
    if (!canSubmit) return;
    onConfirm(conn);
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onOpenChange(false);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("collection.editConnection")}</DialogTitle>
        </DialogHeader>
        <div className="py-2">
          <ConnectionConfigForm value={conn} onChange={setConn} />
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={submit} disabled={!canSubmit}>
            {t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
