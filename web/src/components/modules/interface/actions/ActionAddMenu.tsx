// Unified "add action" entry: one button + one popover containing three sources
// (new script / new DB query / reference from script library), avoiding a row of homogeneous buttons in the toolbar.
import { useState } from "react";
import { BookOpen, ChevronDown, Code2, Database, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { LibraryPickerPanel } from "./LibraryPicker";
import type { ActionTemplate } from "@/data/types";

export function ActionAddMenu({
  allowDb,
  templates,
  onAddScript,
  onAddDb,
  onPickTemplate,
  onOpenLibrary,
}: {
  /** Long-connection session protocols do not yet support DB actions */
  allowDb: boolean;
  templates: ActionTemplate[];
  onAddScript: () => void;
  onAddDb: () => void;
  onPickTemplate: (template: ActionTemplate) => void;
  onOpenLibrary: () => void;
}) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const row =
    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors";

  const close = () => setOpen(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs">
          <Plus className="h-3.5 w-3.5" /> {t("action.addAction")}
          <ChevronDown className="h-3 w-3 opacity-60" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 p-1">
        <button
          type="button"
          className={cn(row, "hover:bg-accent/15")}
          onClick={() => {
            onAddScript();
            close();
          }}
        >
          <Code2 className="h-3.5 w-3.5 text-fuchsia-400" />
          {t("action.addScript")}
        </button>
        <button
          type="button"
          disabled={!allowDb}
          title={allowDb ? undefined : t("action.dbUnsupported")}
          className={cn(
            row,
            allowDb ? "hover:bg-accent/15" : "cursor-not-allowed opacity-50",
          )}
          onClick={() => {
            if (!allowDb) return;
            onAddDb();
            close();
          }}
        >
          <Database className="h-3.5 w-3.5 text-sky-400" />
          {t("action.addDb")}
        </button>

        <Separator className="my-1" />

        <div className="flex items-center gap-1.5 px-2 pb-0.5 text-xs text-muted-foreground">
          <BookOpen className="h-3.5 w-3.5" />
          {t("action.addFromLibrary")}
        </div>
        <LibraryPickerPanel
          templates={templates}
          onPick={(tpl) => {
            onPickTemplate(tpl);
            close();
          }}
          onOpenLibrary={() => {
            close();
            onOpenLibrary();
          }}
        />
      </PopoverContent>
    </Popover>
  );
}
