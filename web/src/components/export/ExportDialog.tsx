import { useMemo, useState } from "react";
import { Download, Loader2, CheckCircle2, Boxes } from "lucide-react";
import { BrandIcon } from "@/components/common/BrandIcon";
import {
  siOpenapiinitiative,
  siPostman,
  siSwagger,
  type SimpleIcon,
} from "simple-icons";
import yaml from "js-yaml";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { cn, methodBg } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { toast } from "sonner";
import {
  defaultFileName,
  type ExportRequest,
  type ExportSpec,
  type ExportFileFormat,
} from "@/lib/exporters";
import { exportCollection, writeExportFile } from "@/lib/bridge";
import { isTauri } from "@/lib/bridge";
import type { DataModel } from "@/data/types";

interface ExportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  requests: ExportRequest[];
  models: DataModel[];
  title: string;
  /** Export scope: collection id (required) */
  collectionId: string;
  /** Export scope: node id (omitted = the whole collection; folder = a folder; request = a single request) */
  itemId?: string;
}

const SPECS: { value: ExportSpec; labelKey: string; icon: SimpleIcon }[] = [
  {
    value: "openapi",
    labelKey: "export.openapi",
    icon: siOpenapiinitiative,
  },
  { value: "swagger", labelKey: "export.swagger", icon: siSwagger },
  { value: "postman", labelKey: "export.postman", icon: siPostman },
];

export function ExportDialog({
  open,
  onOpenChange,
  requests,
  models,
  title,
  collectionId,
  itemId,
}: ExportDialogProps) {
  const { t } = useT();
  const [spec, setSpec] = useState<ExportSpec>("openapi");
  const [format, setFormat] = useState<ExportFileFormat>("json");
  const [busy, setBusy] = useState(false);

  const items = useMemo(() => requests, [requests]);

  const doExport = async () => {
    if (items.length === 0) return;
    setBusy(true);
    try {
      // The frontend only passes the "export scope"; the backend (orbit-data) collects data, assembles the ApiSpec and generates the document
      const json = await exportCollection(spec, title, collectionId, itemId);
      const content =
        format === "yaml"
          ? yaml.dump(JSON.parse(json), {
              noRefs: true,
              indent: 2,
              lineWidth: 120,
            })
          : json;
      const fileName = defaultFileName(title, spec, format);

      if (isTauri()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const filters =
          format === "yaml"
            ? [{ name: "YAML", extensions: ["yaml", "yml"] }]
            : [{ name: "JSON", extensions: ["json"] }];
        const path = await save({ defaultPath: fileName, filters });
        if (!path) {
          // The user cancelled
          setBusy(false);
          return;
        }
        await writeExportFile(path, content);
        toast.success(t("export.success", path));
      } else {
        writeExportFile(fileName, content);
        toast.success(t("export.success", fileName));
      }
      onOpenChange(false);
    } catch (e) {
      toast.error(
        t("export.failed", e instanceof Error ? e.message : String(e)),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[88vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-4 w-4 text-primary" />
            {t("export.title")}
          </DialogTitle>
          <DialogDescription>{t("export.desc")}</DialogDescription>
        </DialogHeader>

        {/* Selected endpoint list */}
        <div className="flex min-h-0 flex-col gap-2">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
            {t("export.selected")}
            <Badge variant="outline" className="text-xs">
              {items.length}
            </Badge>
            {models.length > 0 && (
              <>
                <Boxes className="ml-1 h-3.5 w-3.5 text-primary" />
                {t("export.models")}
                <Badge variant="outline" className="text-xs">
                  {models.length}
                </Badge>
              </>
            )}
          </div>
          <ScrollArea className="h-[34vh] rounded-lg border border-border">
            <div className="space-y-0.5 p-2">
              {items.length === 0 ? (
                <div className="px-2 py-6 text-center text-xs text-muted-foreground">
                  {t("export.empty")}
                </div>
              ) : (
                items.map(({ request, breadcrumb }, i) => (
                  <div
                    key={i}
                    className="flex items-center gap-2 rounded px-2 py-1 text-xs hover:bg-accent/10"
                  >
                    <Badge
                      variant="outline"
                      className={cn(
                        "font-mono text-xs shrink-0",
                        methodBg(request.method),
                      )}
                    >
                      {request.method}
                    </Badge>
                    <span className="min-w-0 flex-1 truncate font-medium">
                      {request.name}
                    </span>
                    {breadcrumb.length > 1 && (
                      <span className="shrink-0 truncate font-mono text-xs text-muted-foreground max-w-[40%]">
                        {breadcrumb.slice(1).join(" / ")}
                      </span>
                    )}
                  </div>
                ))
              )}
            </div>
          </ScrollArea>
        </div>

        {/* Options */}
        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">
              {t("export.spec")}
            </Label>
            <div className="flex flex-col gap-1">
              {SPECS.map((s) => {
                const active = spec === s.value;
                return (
                  <button
                    key={s.value}
                    type="button"
                    onClick={() => setSpec(s.value)}
                    className={cn(
                      "flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs transition-colors",
                      active
                        ? "border-primary bg-primary/10 text-foreground"
                        : "border-border text-muted-foreground hover:bg-accent/10",
                    )}
                  >
                    <BrandIcon icon={s.icon} className="h-3.5 w-3.5 shrink-0" />
                    {t(s.labelKey)}
                  </button>
                );
              })}
            </div>
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">
              {t("export.fileFormat")}
            </Label>
            <div className="flex gap-1.5">
              {(["json", "yaml"] as ExportFileFormat[]).map((f) => {
                const active = format === f;
                return (
                  <button
                    key={f}
                    type="button"
                    onClick={() => setFormat(f)}
                    className={cn(
                      "flex-1 rounded-md border px-2.5 py-1.5 text-xs font-medium uppercase transition-colors",
                      active
                        ? "border-primary bg-primary/10 text-foreground"
                        : "border-border text-muted-foreground hover:bg-accent/10",
                    )}
                  >
                    {f}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
            disabled={busy}
          >
            {t("common.cancel")}
          </Button>
          <Button
            size="sm"
            onClick={doExport}
            disabled={busy || items.length === 0}
            className="gap-1.5"
          >
            {busy ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            {t("export.button")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// Local Label (avoids an extra import from ui/label, keeping the component self-contained)
function Label({
  className,
  children,
}: {
  className?: string;
  children: React.ReactNode;
}) {
  return <div className={cn("font-medium", className)}>{children}</div>;
}
