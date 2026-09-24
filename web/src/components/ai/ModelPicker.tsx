// Model picker (the pill in the input's bottom right): search + a collapsible list grouped by provider + manual model ID entry.
//
// Why Popover instead of DropdownMenu: a **search box** must live in the popup, and a Radix menu would swallow
// typing as typeahead and steal keyboard events; managing highlight and keyboard navigation inside a Popover is more controllable.
//
// Model list sources (in priority order):
// 1. the watch list checked for this credential in settings (`credential.models`);
// 2. the real list obtained by "Fetch" inside the popup (session memory only; if there was no watch list at all it is persisted as well,
//    so a custom gateway with stale presets need not be fetched every time);
// 3. the built-in presets (`AI_MODEL_PRESETS`).
import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Cpu,
  Loader2,
  Pencil,
  RefreshCw,
  Search,
} from "lucide-react";
import { AI_MODEL_PRESETS } from "@/data/aiTypes";
import type { AiCredentialView } from "@/data/aiTypes";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { baseUrlHost, credentialDisplayName } from "@/lib/ai/credentials";
import { matchPreset } from "@/lib/ai/providers";
import { cn } from "@/lib/utils";

/** Models selectable for this credential: the user's watch list first, falling back to the built-in presets. */
function modelsOf(credential: AiCredentialView): string[] {
  const picked = credential.models ?? [];
  if (picked.length > 0) return picked;
  // When a provider preset matches, use **that vendor's** list: a fallback list aggregated by protocol kind would list other vendors'
  // models under this endpoint (kimi-k3 / gpt-6-astra showing up under a DeepSeek credential), and selecting one is a guaranteed 404.
  const preset = matchPreset(credential.kind, credential.baseUrl);
  const presets =
    preset && preset.models.length > 0
      ? preset.models
      : (AI_MODEL_PRESETS[credential.kind] ?? []);
  const withDefault = credential.defaultModel
    ? [
        credential.defaultModel,
        ...presets.filter((m) => m !== credential.defaultModel),
      ]
    : presets;
  return withDefault;
}

export function ModelPicker({ className }: { className?: string }) {
  const { t, format } = useT();
  const credentials = useAppStore((s) => s.aiCredentials);
  const session = useAppStore((s) => s.aiSession);
  const prefs = useAppStore((s) => s.aiPrefs);
  const setSessionModel = useAppStore((s) => s.aiSetSessionModel);
  const fetchModels = useAppStore((s) => s.aiListModels);
  const saveCredential = useAppStore((s) => s.aiSaveCredential);

  const providerId = session?.providerId ?? prefs.providerId ?? null;
  const model = session?.model || prefs.model || "";

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [manual, setManual] = useState(false);
  const [manualValue, setManualValue] = useState("");
  const [fetched, setFetched] = useState<Record<string, string[]>>({});
  const [fetching, setFetching] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [errorText, setErrorText] = useState<string | null>(null);
  const activeRef = useRef<HTMLButtonElement>(null);

  // Return to a clean state on every open: the search term, highlight and manual-entry mode must not persist across opens
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setManual(false);
    setHighlight(0);
  }, [open]);

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (
      credentials
        .map((c) => {
          const name = credentialDisplayName(c).toLowerCase();
          const host = baseUrlHost(c.baseUrl).toLowerCase();
          const all = fetched[c.id] ?? modelsOf(c);
          const list = q
            ? all.filter(
                (m) =>
                  m.toLowerCase().includes(q) ||
                  // Searching by "which credential" is allowed too: both the name and the gateway address count as a match
                  name.includes(q) ||
                  host.includes(q),
              )
            : all;
          return { credential: c, models: list };
        })
        // Hide non-matching groups while searching; show everything with no search (empty groups stay visible so "Fetch" is reachable)
        .filter((g) => (q ? g.models.length > 0 : true))
    );
  }, [credentials, fetched, query]);

  /** Keyboard navigation walks only the "model" rows (group headers do not participate) and the order must match the render order. */
  const flat = useMemo(() => {
    const searching = query.trim().length > 0;
    return groups.flatMap((g) =>
      searching || !collapsed.includes(g.credential.id)
        ? g.models.map((m) => ({ credId: g.credential.id, model: m }))
        : [],
    );
  }, [groups, collapsed, query]);

  useEffect(() => {
    setHighlight(0);
  }, [query]);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [highlight]);

  /** Collapsing applies only without a search: searching forces expansion, otherwise results would be "found but unseen" */
  function isCollapsed(id: string) {
    return collapsed.includes(id) && !query.trim();
  }

  const pick = (credId: string, value: string) => {
    void setSessionModel(credId, value);
    setOpen(false);
  };

  const onFetch = async (c: AiCredentialView) => {
    setFetching(c.id);
    try {
      const list = await fetchModels(c.id);
      setFetched((prev) => ({ ...prev, [c.id]: list }));
      // There was no watch list → persist it as well, avoiding a re-fetch every time
      if ((c.models ?? []).length === 0 && list.length > 0) {
        await saveCredential({
          id: c.id,
          label: c.label,
          kind: c.kind,
          baseUrl: c.baseUrl,
          defaultModel: c.defaultModel,
          models: list,
        });
      }
      setCollapsed((prev) => prev.filter((id) => id !== c.id));
    } catch (e) {
      setErrorText(e instanceof Error ? e.message : String(e));
    } finally {
      setFetching(null);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (manual) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (flat.length === 0) return;
      const next = e.key === "ArrowDown" ? highlight + 1 : highlight - 1;
      setHighlight(Math.min(Math.max(next, 0), flat.length - 1));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const item = flat[highlight];
      if (item) pick(item.credId, item.model);
    }
  };

  let cursor = -1;

  return (
    <Popover open={open} onOpenChange={(v) => setOpen(v)}>
      <PopoverTrigger asChild>
        <button
          type="button"
          // Hovering reveals the full model name (it can still truncate in a very narrow drawer)
          title={model || t("ai.model.pick")}
          className={cn(
            // Width grows with the content, giving the model name all remaining bottom-bar space; it truncates only when compressed (min-w-0 + shrink)
            "flex h-7 min-w-0 max-w-full cursor-pointer items-center gap-1 rounded-full border border-border px-2 text-xs transition-colors hover:bg-accent",
            model ? "text-foreground" : "text-muted-foreground",
            className,
          )}
        >
          <Cpu className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 truncate font-mono">
            {model || t("ai.model.none")}
          </span>
          <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
        </button>
      </PopoverTrigger>

      {/* The input sits at the bottom: it expands upwards so the window edge cannot crop it */}
      <PopoverContent
        side="top"
        align="end"
        sideOffset={6}
        className="w-80 p-0"
        onKeyDown={onKeyDown}
      >
        {manual ? (
          <ManualModelInput
            value={manualValue}
            onChange={setManualValue}
            onCancel={() => setManual(false)}
            onConfirm={(value) => {
              const id = providerId ?? credentials[0]?.id;
              if (!id || !value.trim()) return;
              pick(id, value.trim());
            }}
          />
        ) : (
          <>
            <div className="flex items-center gap-1.5 border-b border-border px-2.5 py-2">
              <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <input
                autoFocus
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("ai.model.search")}
                className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground"
              />
            </div>

            {errorText && (
              <p className="border-b border-border px-2.5 py-1.5 text-xs text-destructive">
                {format("ai.model.fetchFailed", errorText)}
              </p>
            )}

            <div className="max-h-80 overflow-y-auto py-1">
              {groups.length === 0 && (
                <p className="px-2.5 py-2 text-xs text-muted-foreground">
                  {credentials.length === 0
                    ? t("ai.settings.noCredential")
                    : t("ai.model.empty")}
                </p>
              )}
              {groups.map(({ credential: c, models }) => {
                const open2 = !isCollapsed(c.id);
                const busy = fetching === c.id;
                return (
                  <div key={c.id}>
                    <div className="flex items-center gap-1 px-1.5 py-0.5">
                      <button
                        type="button"
                        onClick={() =>
                          setCollapsed((prev) =>
                            prev.includes(c.id)
                              ? prev.filter((id) => id !== c.id)
                              : [...prev, c.id],
                          )
                        }
                        className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded px-1 py-0.5 text-left hover:bg-accent/60"
                      >
                        {open2 ? (
                          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        ) : (
                          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        )}
                        {/* The parent shows the **credential name** (as set in AI settings) with the gateway address as secondary info, so same-named credentials stay distinguishable */}
                        <span
                          title={`${credentialDisplayName(c)} · ${c.baseUrl}`}
                          className="min-w-0 flex-1 truncate text-xs font-semibold"
                        >
                          {credentialDisplayName(c)}
                        </span>
                      </button>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        title={t("ai.model.fetch")}
                        disabled={busy}
                        onClick={() => void onFetch(c)}
                      >
                        {busy ? (
                          <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                          <RefreshCw className="h-3 w-3" />
                        )}
                      </Button>
                    </div>

                    {open2 &&
                      (models.length === 0 ? (
                        <p className="px-6 py-1 text-xs text-muted-foreground">
                          {t("ai.model.empty")}
                        </p>
                      ) : (
                        models.map((m) => {
                          cursor += 1;
                          const active = highlight === cursor;
                          const selected = c.id === providerId && m === model;
                          return (
                            <button
                              key={`${c.id}-${m}`}
                              ref={active ? activeRef : undefined}
                              type="button"
                              onMouseEnter={() => setHighlight(cursor)}
                              onClick={() => pick(c.id, m)}
                              className={cn(
                                "flex w-full cursor-pointer items-center gap-2 px-6 py-1 text-left",
                                active && "bg-accent",
                              )}
                            >
                              <span className="min-w-0 flex-1 truncate font-mono text-xs">
                                {m}
                              </span>
                              {selected && (
                                <Check className="h-3.5 w-3.5 shrink-0" />
                              )}
                            </button>
                          );
                        })
                      ))}
                  </div>
                );
              })}
            </div>

            <button
              type="button"
              onClick={() => {
                setManualValue(model);
                setManual(true);
              }}
              className="flex w-full cursor-pointer items-center gap-2 border-t border-border px-2.5 py-2 text-left text-xs hover:bg-accent"
            >
              <Pencil className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              {t("ai.model.manual")}
            </button>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

/** Manually enter a model ID (custom gateways / not-yet-released models). */
function ManualModelInput({
  value,
  onChange,
  onCancel,
  onConfirm,
}: {
  value: string;
  onChange: (value: string) => void;
  onCancel: () => void;
  onConfirm: (value: string) => void;
}) {
  const { t } = useT();
  return (
    <div className="space-y-2 p-2.5">
      <input
        autoFocus
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            onConfirm(value);
          }
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        placeholder={t("ai.model.manualPlaceholder")}
        className="w-full rounded-md border border-input bg-transparent px-2 py-1 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
      <div className="flex items-center justify-end gap-1.5">
        <Button size="xs" variant="ghost" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button size="xs" onClick={() => onConfirm(value)}>
          {t("common.confirm")}
        </Button>
      </div>
    </div>
  );
}
