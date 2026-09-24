// API reference dialog: dynamic values plus scripts, one reference across two tabs.
// - Dynamic values tab: every DYNAMIC_CATALOG entry grouped by category (label / token / desc).
// - Scripts tab: renders the built-in scripting API reference from getScriptsReference.
// Shared by three entry points: the top bar, the dynamic-value popup's top-right corner and the script syntax reference button.
import { useState } from "react";
import { Braces, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { DYNAMIC_CATALOG } from "@/lib/resolve";
import { useT, type TKey } from "@/lib/i18n";
import { getScriptsReference } from "@/lib/scriptsReference";

export type ReferenceTab = "dynamic" | "script";

/** Inline `code` becomes <code>; plain text is left as-is */
function renderInline(text: string) {
  const parts = text.split(/`([^`]+)`/g);
  return parts.map((p, i) =>
    i % 2 === 1 ? (
      <code key={i} className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
        {p}
      </code>
    ) : (
      <span key={i}>{p}</span>
    ),
  );
}

export function ReferenceDialog({
  open,
  onOpenChange,
  initialTab = "dynamic",
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /** Initially active tab (dynamic values by default) */
  initialTab?: ReferenceTab;
}) {
  const { t, locale } = useT();
  const [tab, setTab] = useState<ReferenceTab>(initialTab);
  const ref = getScriptsReference(locale);

  // Group the dynamic-value catalog by category
  const groups = DYNAMIC_CATALOG.reduce<Record<string, typeof DYNAMIC_CATALOG>>(
    (acc, d) => {
      (acc[d.category] ||= []).push(d);
      return acc;
    },
    {},
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[85vh] flex-col overflow-hidden sm:max-w-3xl">
        <DialogHeader className="flex-shrink-0">
          <DialogTitle>{t("reference.title")}</DialogTitle>
        </DialogHeader>

        <Tabs
          value={tab}
          onValueChange={(v) => setTab(v as ReferenceTab)}
          className="flex min-h-0 flex-1 flex-col"
        >
          <TabsList className="w-fit flex-shrink-0">
            <TabsTrigger value="dynamic" className="gap-1.5">
              <Braces className="h-3.5 w-3.5" />
              {t("reference.tabDynamic")}
            </TabsTrigger>
            <TabsTrigger value="script" className="gap-1.5">
              <Terminal className="h-3.5 w-3.5" />
              {t("reference.tabScript")}
            </TabsTrigger>
          </TabsList>

          {/* Dynamic values tab */}
          <TabsContent value="dynamic" className="min-h-0 flex-1">
            <ScrollArea className="h-full">
              <div className="space-y-4 pr-3 text-sm">
                <p className="text-xs text-muted-foreground">
                  {t("reference.dynamicIntro")}
                </p>
                {/* tz timezone parameter note */}
                <div className="rounded-md border border-border bg-muted/50 px-2.5 py-2 text-xs leading-relaxed text-muted-foreground">
                  <div className="mb-0.5 font-medium text-foreground">
                    {t("reference.tzTitle")}
                  </div>
                  <div className="whitespace-pre-line">
                    {t("reference.tzFormats")}
                  </div>
                </div>
                {/* locale data-language parameter note */}
                <div className="rounded-md border border-violet-500/30 bg-violet-500/5 px-2.5 py-2 text-xs leading-relaxed text-muted-foreground">
                  <div className="mb-0.5 font-medium text-foreground">
                    {t("reference.localeTitle")}
                  </div>
                  <div className="whitespace-pre-line">
                    {renderInline(t("reference.localeFormats"))}
                  </div>
                </div>
                {Object.entries(groups).map(([cat, defs]) => (
                  <div key={cat}>
                    <h3 className="mb-1 flex items-center gap-2 text-sm font-semibold">
                      <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                        {cat}
                      </code>
                      <span className="text-xs font-normal text-muted-foreground">
                        {defs.length} {t("reference.items")}
                      </span>
                    </h3>
                    <div className="space-y-1">
                      {defs.map((d) => {
                        // Example: with tzExample show the full form including tz; otherwise use the catalog's args
                        const displayArgs = d.tzExample
                          ? `${d.args ?? ""}${d.args ? ", " : ""}tz=${d.tzExample}`
                          : d.args;
                        const token = displayArgs
                          ? `$${d.category}.${d.method}(${displayArgs})`
                          : `$${d.category}.${d.method}`;
                        // When every argument is optional, show the "terse form" (default arguments)
                        const minToken = d.argsOptional
                          ? `$${d.category}.${d.method}`
                          : null;
                        return (
                          <div
                            key={d.method}
                            className="rounded-md border border-border bg-muted/20 px-2.5 py-1.5"
                          >
                            <div className="flex items-center justify-between gap-2">
                              <span className="flex min-w-0 items-center gap-1.5">
                                <span className="truncate text-xs font-medium">
                                  {t(d.labelKey as TKey)}
                                </span>
                                {d.argsOptional && (
                                  <span className="shrink-0 rounded border border-emerald-500/40 bg-emerald-500/10 px-1 text-xs leading-4 text-emerald-500">
                                    {t("reference.argsOptional")}
                                  </span>
                                )}
                                {d.localeSensitive && (
                                  <span className="shrink-0 rounded border border-violet-500/40 bg-violet-500/10 px-1 text-xs leading-4 text-violet-500">
                                    {t("reference.localeSensitive")}
                                  </span>
                                )}
                              </span>
                              <code className="shrink-0 text-xs text-primary">
                                {`{{${token}}}`}
                              </code>
                            </div>
                            <div className="mt-0.5 text-xs text-muted-foreground">
                              {t(d.descKey as TKey)}
                            </div>
                            {d.argsOptional && (
                              <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
                                <span className="text-muted-foreground/70">
                                  {t("reference.defaults")}：
                                  {d.defaultsKey
                                    ? t(d.defaultsKey as TKey)
                                    : ""}
                                </span>
                                {minToken && (
                                  <span>
                                    {t("reference.minimal")}：
                                    <code className="rounded bg-muted px-1 py-px font-mono text-xs text-primary">
                                      {`{{${minToken}}}`}
                                    </code>
                                  </span>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>

          {/* Scripts tab */}
          <TabsContent value="script" className="min-h-0 flex-1">
            <ScrollArea className="h-full">
              <div className="space-y-4 pr-3 text-sm">
                <p className="text-muted-foreground">{ref.intro}</p>
                {ref.sections.map((s) => (
                  <div key={s.title}>
                    <h3 className="mb-1 font-semibold">{s.title}</h3>
                    {s.body?.map((p, i) => (
                      <p key={i} className="mb-1 text-muted-foreground">
                        {renderInline(p)}
                      </p>
                    ))}
                    {s.bullets && (
                      <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
                        {s.bullets.map((b, i) => (
                          <li key={i}>{renderInline(b)}</li>
                        ))}
                      </ul>
                    )}
                    {s.code && (
                      <pre className="mt-1 overflow-x-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-xs leading-relaxed whitespace-pre">
                        <code>{s.code}</code>
                      </pre>
                    )}
                  </div>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>
        </Tabs>

        <DialogFooter className="flex-shrink-0">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
