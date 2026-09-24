// Library item picker (popover content): search + flat list + "manage script library" entry.
//
// Renders content only, without its own trigger button; shared by the "add action" popover and the broken-reference "reselect" popover.
import { useState } from "react";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { filterTemplates } from "@/lib/actionLibrary";
import type { ActionTemplate } from "@/data/types";

export function LibraryPickerPanel({
  templates,
  onPick,
  onOpenLibrary,
}: {
  templates: ActionTemplate[];
  onPick: (template: ActionTemplate) => void;
  onOpenLibrary: () => void;
}) {
  const { t } = useT();
  const [query, setQuery] = useState("");
  const items = filterTemplates(templates, query);

  return (
    <div className="space-y-1">
      <Input
        className="h-7 text-xs"
        placeholder={t("lib.searchPlaceholder")}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <div className="max-h-60 space-y-0.5 overflow-y-auto">
        {templates.length === 0 && (
          <div className="px-1 py-3 text-xs text-muted-foreground">
            {t("lib.emptyPicker")}
          </div>
        )}
        {templates.length > 0 && items.length === 0 && (
          <div className="px-1 py-3 text-xs text-muted-foreground">
            {t("lib.noMatch")}
          </div>
        )}
        {items.map((tpl) => (
          <button
            key={tpl.id}
            type="button"
            onClick={() => onPick(tpl)}
            className={cn(
              "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
              "hover:bg-accent/15",
            )}
          >
            <span className="min-w-0 flex-1 truncate text-xs">{tpl.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {tpl.action.kind === "script"
                ? t("action.kindScript")
                : t("action.kindDb")}
            </span>
          </button>
        ))}
      </div>
      <div className="border-t border-border pt-1">
        <Button
          variant="ghost"
          size="sm"
          className="h-6 gap-1 px-2 text-xs"
          onClick={onOpenLibrary}
        >
          <Plus className="h-3 w-3" /> {t("lib.manage")}
        </Button>
      </div>
    </div>
  );
}
