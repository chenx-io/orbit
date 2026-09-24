// Script example popover: lists common examples by script kind (pre/post); clicking one appends its code to the script editor.
import { useState } from "react";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { useT } from "@/lib/i18n";
import {
  getScriptExampleCategories,
  type ScriptKind,
} from "@/lib/scriptExamples";

export function ScriptExamplesPopover({
  kind,
  onAppend,
}: {
  kind: ScriptKind;
  onAppend: (code: string) => void;
}) {
  const { t, locale } = useT();
  const [open, setOpen] = useState(false);
  const cats = getScriptExampleCategories(kind);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs">
          <Plus className="h-3.5 w-3.5 mr-1" />
          {t("script.addExample")}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 p-0">
        <div className="border-b border-border px-3 py-1.5 text-xs font-medium text-muted-foreground">
          {t("script.exampleTitle")}
        </div>
        <div className="max-h-80 overflow-y-auto py-1">
          {cats.map((cat) => (
            <div key={cat.id}>
              <div className="px-3 py-1 text-xs font-medium text-muted-foreground">
                {cat.label[locale]}
              </div>
              {cat.examples.map((ex) => {
                const snippet = ex.code[locale];
                const firstLine = snippet.split("\n")[0] ?? "";
                const hint = firstLine.replace(/^\/\/\s?/, "").trim();
                return (
                  <button
                    key={ex.id}
                    type="button"
                    onClick={() => {
                      onAppend(snippet);
                      setOpen(false);
                    }}
                    className="flex w-full flex-col items-start gap-0.5 px-3 py-1.5 text-left text-xs hover:bg-accent"
                  >
                    <span className="text-foreground">{ex.label[locale]}</span>
                    {hint && (
                      <span className="truncate font-mono text-xs text-muted-foreground">
                        {hint}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
