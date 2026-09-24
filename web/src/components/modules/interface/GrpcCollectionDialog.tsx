// New gRPC collection dialog: just enter a collection name and create (empty collection).
// Interface import (proto file / server reflection) is done via the collection's right-click menu "import proto / reflection import" entries,
// see GrpcImportDialog.tsx.
import { useState } from "react";
import { Cable, Info } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";

export function GrpcCollectionDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useT();
  const createGrpcCollection = useAppStore((s) => s.createGrpcCollection);
  const [name, setName] = useState(t("grpc.newCollectionName"));

  const reset = () => setName(t("grpc.newCollectionName"));

  const close = () => {
    onOpenChange(false);
    reset();
  };

  const handleCreate = () => {
    createGrpcCollection(name.trim() || t("grpc.newCollectionName"), {
      source: { type: "proto", files: [] },
      packages: [],
    });
    close();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) close();
        else onOpenChange(true);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Cable className="h-4 w-4 text-fuchsia-400" />
            {t("grpc.newCollectionTitle")}
          </DialogTitle>
        </DialogHeader>

        <div className="grid gap-4 py-2">
          <div className="grid gap-2">
            <Label className="text-xs">{t("grpc.collectionName")}</Label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreate();
              }}
              className="text-sm"
              placeholder={t("grpc.newCollectionName")}
              autoFocus
            />
          </div>
          <div className="flex items-start gap-2 rounded-md border border-border bg-muted/20 px-3 py-2 text-xs text-muted-foreground">
            <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {t("grpc.newCollectionHint")}
          </div>
        </div>

        <DialogFooter className="mt-2">
          <Button variant="outline" size="sm" onClick={close}>
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={handleCreate}>
            <Cable className="mr-1.5 h-3.5 w-3.5" />
            {t("grpc.createCollection")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
