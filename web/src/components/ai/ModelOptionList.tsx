// Model checklist: after "Fetch models" in settings, pick the ones to watch, for the drawer's model picker to use.
import { CheckSquare, Square } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { useT } from "@/lib/i18n";

export interface ModelOptionListProps {
  /** Available models (the fetch result; the watched list is shown when empty) */
  catalog: string[];
  /** Checked for watching */
  selected: string[];
  onToggle: (model: string, checked: boolean) => void;
  onSelectAll: () => void;
  onClear: () => void;
}

export function ModelOptionList({
  catalog,
  selected,
  onToggle,
  onSelectAll,
  onClear,
}: ModelOptionListProps) {
  const { t } = useT();
  const rows = catalog.length > 0 ? catalog : selected;

  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-2">
        <Label className="text-xs text-muted-foreground">
          {t("ai.settings.models")}
        </Label>
        <span className="text-xs text-muted-foreground">
          {t("ai.settings.modelsCount")} {selected.length}
        </span>
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            className="flex cursor-pointer items-center gap-1 rounded px-1 text-xs text-muted-foreground hover:text-foreground"
            onClick={onSelectAll}
          >
            <CheckSquare className="h-3 w-3" />
            {t("ai.settings.selectAll")}
          </button>
          <button
            type="button"
            className="flex cursor-pointer items-center gap-1 rounded px-1 text-xs text-muted-foreground hover:text-foreground"
            onClick={onClear}
          >
            <Square className="h-3 w-3" />
            {t("ai.settings.clearAll")}
          </button>
        </div>
      </div>

      {rows.length === 0 ? (
        <p className="rounded-md border border-dashed border-border px-2.5 py-2 text-xs text-muted-foreground">
          {t("ai.settings.modelsEmpty")}
        </p>
      ) : (
        <div className="max-h-44 overflow-y-auto rounded-md border border-border p-1">
          {rows.map((m) => {
            const checked = selected.includes(m);
            return (
              <label
                key={m}
                className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 hover:bg-accent/60"
              >
                <Checkbox
                  checked={checked}
                  onCheckedChange={(v) => onToggle(m, v === true)}
                />
                <span className="min-w-0 flex-1 truncate font-mono text-xs">
                  {m}
                </span>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}
