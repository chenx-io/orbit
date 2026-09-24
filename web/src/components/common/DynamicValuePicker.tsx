import { useState } from "react";
import { Braces, BookOpen, Languages } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { DYNAMIC_CATALOG, buildDynamicToken } from "@/lib/resolve";
import { useT, type TKey } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import type { DataLocale } from "@/data/types";
import { ReferenceDialog } from "./ReferenceDialog";

/**
 * Data locale options (aligned with the backend orbit_dynamic locale).
 *
 * Each name is an endonym, so the same string is used in every UI locale — a language switcher
 * should show a language in its own script.
 */
const DATA_LOCALES: { code: DataLocale; labelKey: TKey }[] = [
  { code: "zh", labelKey: "dataLocale.zh" },
  { code: "en", labelKey: "dataLocale.en" },
  { code: "ja", labelKey: "dataLocale.ja" },
];

export function DynamicValuePicker({
  onInsert,
}: {
  onInsert: (token: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [refOpen, setRefOpen] = useState(false);
  const dataLocale = useAppStore((s) => s.dataLocale);
  const setDataLocale = useAppStore((s) => s.setDataLocale);
  const { t } = useT();

  const groups = DYNAMIC_CATALOG.reduce<Record<string, typeof DYNAMIC_CATALOG>>(
    (acc, d) => {
      (acc[d.category] ||= []).push(d);
      return acc;
    },
    {},
  );

  const currentLabel = t(
    DATA_LOCALES.find((l) => l.code === dataLocale)?.labelKey ??
      "dataLocale.zh",
  );

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon-sm" title={t("dynamic.insert")}>
          <Braces className="h-3.5 w-3.5 text-primary" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 p-0">
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <span className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
            {t("dynamic.title")} · {currentLabel}
          </span>
          <button
            onClick={() => {
              setRefOpen(true);
              setOpen(false);
            }}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted-foreground transition-colors hover:bg-accent/15 hover:text-foreground"
            title={t("reference.openEntry")}
          >
            <BookOpen className="h-3 w-3" />
            {t("reference.tabScript")} · {t("reference.tabDynamic")}
          </button>
        </div>
        {/* Data locale switch: locale-sensitive entries (names / cities / products ...) append locale= based on it */}
        <div className="flex items-center gap-1 border-b border-border px-3 py-1.5">
          <Languages className="h-3 w-3 text-muted-foreground" />
          <div className="flex rounded-md bg-muted p-0.5">
            {DATA_LOCALES.map((l) => (
              <button
                key={l.code}
                onClick={() => setDataLocale(l.code)}
                className={`rounded px-1.5 py-0.5 text-xs transition-colors ${
                  dataLocale === l.code
                    ? "bg-background font-medium text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {t(l.labelKey)}
              </button>
            ))}
          </div>
          <span className="ml-auto text-xs text-muted-foreground/70">
            {t("dynamic.localeHint")}
          </span>
        </div>
        <ScrollArea className="h-64">
          <div className="p-1.5">
            {Object.entries(groups).map(([cat, defs]) => (
              <div key={cat} className="mb-1">
                <div className="px-2 py-1 text-xs uppercase tracking-wide text-muted-foreground">
                  {cat}
                </div>
                {defs.map((d) => {
                  const token = buildDynamicToken(d, dataLocale);
                  return (
                    <button
                      key={d.method}
                      onClick={() => {
                        onInsert(token);
                        setOpen(false);
                      }}
                      className="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-xs hover:bg-accent/15"
                    >
                      <span className="font-medium">
                        {t(d.labelKey as TKey)}
                      </span>
                      <code className="text-xs text-muted-foreground">{`{{${token}}}`}</code>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        </ScrollArea>
      </PopoverContent>

      {/* API reference dialog (dynamic values / scripts, two tabs) */}
      <ReferenceDialog open={refOpen} onOpenChange={setRefOpen} />
    </Popover>
  );
}
