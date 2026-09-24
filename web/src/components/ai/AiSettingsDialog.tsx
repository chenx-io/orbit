// AI settings: BYOK credential management (Provider / Base URL / API Key / default model) + test connection +
// assistant preferences and provider credentials (provider presets, endpoint, models, custom headers, reply language).
//
// Security convention: API keys are write-only — saved credentials only show a masked hint (`sk-...1234`) and
// leaving the field blank keeps the existing value.
import { useEffect, useState } from "react";
import { CheckCircle2, Loader2, Plus, Trash2, XCircle } from "lucide-react";
import type {
  AiAuthStyle,
  AiCredentialView,
  AiHeaderPair,
  AiProviderKind,
  AiTestConnectionResult,
} from "@/data/aiTypes";
import {
  AI_DEFAULT_BASE_URL,
  AI_DEFAULT_MODEL,
  AI_MODEL_PRESETS,
} from "@/data/aiTypes";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { isTauri } from "@/lib/bridge";
import {
  credentialDisplayName,
  defaultCredentialName,
} from "@/lib/ai/credentials";
import {
  defaultAuthStyle,
  matchPreset,
  normalizeAuthStyle,
  presetLabel,
  standardAuthHeaderName,
  PROVIDER_PRESETS,
} from "@/lib/ai/providers";
import { cn } from "@/lib/utils";
import { ModelOptionList } from "./ModelOptionList";

interface FormState {
  id: string | null;
  label: string;
  kind: AiProviderKind;
  baseUrl: string;
  apiKey: string;
  defaultModel: string;
  /** Auth header style: the vendor's standard header (`x-api-key` / `api-key`) or `Authorization: Bearer` */
  authStyle: AiAuthStyle;
  /** Models checked for watching (used by the drawer's model picker) */
  models: string[];
  /** Custom headers (vendor / gateway differences) */
  headers: AiHeaderPair[];
}

const EMPTY_FORM: FormState = {
  id: null,
  label: "",
  kind: "openai",
  baseUrl: AI_DEFAULT_BASE_URL.openai,
  apiKey: "",
  defaultModel: AI_DEFAULT_MODEL.openai,
  authStyle: defaultAuthStyle("openai"),
  models: [],
  headers: [],
};

function formFrom(view: AiCredentialView): FormState {
  return {
    id: view.id,
    // Prefill the display name: when legacy data has none (an empty label or a bare protocol value) a usable name is given here,
    // frozen once the user saves, so the model popup's parent group finally has a decent heading
    label: credentialDisplayName(view),
    kind: view.kind,
    baseUrl: view.baseUrl || AI_DEFAULT_BASE_URL[view.kind],
    apiKey: "",
    defaultModel: view.defaultModel ?? AI_DEFAULT_MODEL[view.kind],
    authStyle: normalizeAuthStyle(view.kind, view.authStyle),
    models: view.models ?? [],
    headers: view.headers ?? [],
  };
}

export interface AiSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function AiSettingsDialog({
  open,
  onOpenChange,
}: AiSettingsDialogProps) {
  const { t } = useT();
  const credentials = useAppStore((s) => s.aiCredentials);
  const prefs = useAppStore((s) => s.aiPrefs);
  const saveCredential = useAppStore((s) => s.aiSaveCredential);
  const removeCredential = useAppStore((s) => s.aiRemoveCredential);
  const testConnection = useAppStore((s) => s.aiTestConnection);
  const savePrefs = useAppStore((s) => s.aiSavePrefs);
  const loadConfig = useAppStore((s) => s.aiLoadConfig);
  const listModels = useAppStore((s) => s.aiListModels);

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [test, setTest] = useState<AiTestConnectionResult | null>(null);
  /** Available models from the fetch (empty = show the watched list only) */
  const [catalog, setCatalog] = useState<string[]>([]);
  /**
   * Whether the display name was manually edited by the user.
   *
   * When untouched the name follows the selected provider (picking DeepSeek then switching to OpenAI must rename it);
   * once the user types one it is never overwritten — that is their name.
   */
  const [labelTouched, setLabelTouched] = useState(false);

  useEffect(() => {
    if (open) {
      void loadConfig();
      setError(null);
      setTest(null);
      setCatalog([]);
      const first = credentials.length > 0 ? credentials[0] : null;
      setForm(first ? formFrom(first) : EMPTY_FORM);
      // A credential with an explicit name counts as "user-edited": switching providers must not rename it
      setLabelTouched(Boolean(first?.label.trim()));
    }
    // Reset only on open; credential list changes are driven by user clicks
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const desktop = isTauri();
  // Current provider: look up the preset by "protocol + address" (a private gateway matches nothing → shown as "custom")
  const activePreset = matchPreset(form.kind, form.baseUrl);
  // Candidates for the default-model input: this provider's own list first, then the fallback aggregated by protocol kind.
  // Using the kind-level list directly would suggest other vendors' models (such as gpt-6-astra) under a DeepSeek endpoint.
  const presets =
    activePreset && activePreset.models.length > 0
      ? activePreset.models
      : AI_MODEL_PRESETS[form.kind];

  /** Provider display name for a credential (falling back to the protocol name when no preset matches). */
  const providerNameOf = (c: AiCredentialView) => {
    const preset = matchPreset(c.kind, c.baseUrl);
    return preset ? presetLabel(preset, t) : t(`ai.settings.kind.${c.kind}`);
  };

  const patch = (next: Partial<FormState>) => {
    setForm((f) => ({ ...f, ...next }));
    setTest(null);
  };

  /**
   * Selecting a provider preset fills in the protocol kind + endpoint + default model at once.
   *
   * It also fills in the name (only when creating and unnamed) and the watched model list (only when there is none yet),
   * so "pick a provider + paste a key" suffices without copying addresses or model names one by one.
   */
  const onPickPreset = (presetId: string) => {
    const preset = PROVIDER_PRESETS.find((p) => p.id === presetId);
    if (!preset) return; // "Custom": keep what the user already filled in
    patch({
      kind: preset.kind,
      baseUrl: preset.baseUrl || form.baseUrl,
      defaultModel: preset.defaultModel || form.defaultModel,
      // A changed protocol kind switches the default auth style; within one kind the user's manual choice is kept
      // (for example Claude through its own gateway needs Bearer)
      authStyle:
        preset.kind === form.kind
          ? form.authStyle
          : defaultAuthStyle(preset.kind),
      models: form.models.length > 0 ? form.models : [...preset.models],
      // The name follows the provider and is overwritten only when the user has not edited it.
      // Note that requiring an empty label here would be wrong — the previous preset already set a name,
      // and without following along the user would end up "picking OpenAI while the name says DeepSeek".
      label: labelTouched ? form.label : presetLabel(preset, t),
    });
  };

  const onSave = async () => {
    setBusy(true);
    setError(null);
    try {
      const id = await saveCredential({
        id: form.id,
        // A blank value no longer degrades to a raw protocol value such as `openai` — otherwise the popup could not tell which credential it is
        label:
          form.label.trim() || defaultCredentialName(form.kind, form.baseUrl),
        kind: form.kind,
        baseUrl: form.baseUrl.trim(),
        // Blank = keep the existing secret
        apiKey: form.apiKey.trim() ? form.apiKey.trim() : null,
        defaultModel: form.defaultModel.trim() || null,
        authStyle: form.authStyle,
        models: form.models,
        // Always submit the whole document: only then does "delete the last custom header" take effect (omitting = keep)
        headers: form.headers
          .filter((h) => h.key.trim())
          .map((h) => ({ key: h.key.trim(), value: h.value.trim() })),
      });
      // Default credential: kept when it still exists, otherwise it points at the one just saved (avoiding a dangling id)
      const current = useAppStore.getState().aiCredentials;
      const stillValid = current.some((c) => c.id === prefs.providerId);
      await savePrefs({
        ...prefs,
        providerId: stillValid ? prefs.providerId : id,
        provider: form.kind,
      });
      setForm((f) => ({ ...f, id, apiKey: "" }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Fetch the model list (`GET {base}/models`), giving a readable reason on failure and still allowing manual entry. */
  const onFetchModels = async () => {
    if (!form.id) {
      setError(t("ai.settings.saveFirst"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const list = await listModels(form.id);
      setCatalog(list);
      // When the watch list is empty the current default model is pre-checked (if the server really offers it)
      if (form.models.length === 0 && form.defaultModel.trim()) {
        const preferred = form.defaultModel.trim();
        if (list.includes(preferred)) {
          setForm((f) => ({ ...f, models: [preferred] }));
        }
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onTest = async () => {
    if (!form.id) {
      setError(t("ai.settings.saveFirst"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setTest(await testConnection(form.id, form.defaultModel || null));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async () => {
    if (!form.id) return;
    const removedId = form.id;
    setBusy(true);
    try {
      await removeCredential(removedId);
      const rest = useAppStore
        .getState()
        .aiCredentials.filter((c) => c.id !== removedId);
      setForm(rest.length > 0 ? formFrom(rest[0]) : EMPTY_FORM);
      // When the deleted one was the default, point at the first remaining credential (otherwise starting a conversation reports "credential not found")
      if (prefs.providerId === removedId) {
        await savePrefs({
          ...prefs,
          providerId: rest[0]?.id ?? null,
          provider: rest[0]?.kind ?? prefs.provider,
        });
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* A long form: three rows are declared explicitly (header / scroll area / footer buttons) with a height cap.
          Otherwise oversized content lets you reach neither end — the footer button is invisible and nothing scrolls
          (guaranteed to happen with a preset that carries a hint block, such as Gemini). */}
      <DialogContent className="max-h-[85vh] max-w-3xl grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t("ai.settings.title")}</DialogTitle>
          <DialogDescription>{t("ai.settings.description")}</DialogDescription>
          {/* The non-desktop notice goes in the header: it is dialog-level information and this leaves a single scroll area in the middle */}
          {!desktop && (
            <p className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
              {t("ai.error.desktopOnly")}
            </p>
          )}
        </DialogHeader>

        <div className="grid grid-cols-[minmax(0,220px)_minmax(0,1fr)] gap-4 overflow-y-auto pr-1">
          {/* ── Credential list ── */}
          <div className="flex min-h-0 flex-col gap-1.5">
            <Label className="text-xs text-muted-foreground">
              {t("ai.settings.credentials")}
            </Label>
            <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto rounded-md border border-border p-1">
              {credentials.length === 0 && (
                <span className="px-2 py-1 text-xs text-muted-foreground">
                  {t("ai.settings.noCredential")}
                </span>
              )}
              {credentials.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => {
                    setForm(formFrom(c));
                    setTest(null);
                    setLabelTouched(Boolean(c.label.trim()));
                  }}
                  className={cn(
                    "flex cursor-pointer flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors",
                    form.id === c.id
                      ? "bg-accent text-accent-foreground"
                      : "hover:bg-accent/60",
                  )}
                >
                  <span className="flex items-center gap-1.5">
                    <span className="min-w-0 flex-1 truncate text-xs font-medium">
                      {credentialDisplayName(c)}
                    </span>
                    <span className="shrink-0 font-mono text-xs text-muted-foreground">
                      {c.keyHint}
                    </span>
                  </span>
                  {/* The subtitle uses the **vendor name** rather than the protocol code: users recognize "DeepSeek", not "openai" */}
                  <span className="truncate text-xs text-muted-foreground">
                    {providerNameOf(c)} · {c.defaultModel ?? "—"}
                  </span>
                </button>
              ))}
            </div>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setForm(EMPTY_FORM);
                setTest(null);
                // Create: the name follows the provider again
                setLabelTouched(false);
              }}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("ai.settings.addCredential")}
            </Button>
          </div>

          {/* ── Form ── */}
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="ai-preset">
                  {t("ai.settings.providerPreset")}
                </Label>
                <Select
                  value={activePreset?.id ?? "custom"}
                  onValueChange={onPickPreset}
                >
                  <SelectTrigger id="ai-preset" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PROVIDER_PRESETS.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        {presetLabel(p, t)}
                      </SelectItem>
                    ))}
                    {/* "Custom / private gateway" is the fallback item and comes last (not competing for the first screen);
                       it is selected when the address is custom, so the dropdown is never left blank */}
                    <SelectItem value="custom">
                      {t("ai.settings.preset.custom")}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("ai.settings.providerPresetHint")}
                </p>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="ai-label">{t("ai.settings.label")}</Label>
                <Input
                  id="ai-label"
                  value={form.label}
                  onChange={(e) => {
                    // Once the user types, the name no longer auto-follows the provider
                    setLabelTouched(true);
                    patch({ label: e.target.value });
                  }}
                  placeholder={t("ai.settings.labelPlaceholder")}
                />
                <p className="text-xs text-muted-foreground">
                  {t("ai.settings.labelHint")}
                </p>
              </div>
            </div>

            {activePreset?.hintKey && (
              <p className="rounded-md border border-border bg-muted/40 px-2.5 py-1.5 text-xs text-muted-foreground">
                {t(activePreset.hintKey)}
              </p>
            )}

            <div className="space-y-1.5">
              <Label htmlFor="ai-base">{t("ai.settings.baseUrl")}</Label>
              <Input
                id="ai-base"
                className="font-mono text-xs"
                value={form.baseUrl}
                onChange={(e) => patch({ baseUrl: e.target.value })}
              />
              <p className="text-xs text-muted-foreground">
                {t("ai.settings.baseUrlHint")}
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="ai-key">{t("ai.settings.apiKey")}</Label>
              <Input
                id="ai-key"
                type="password"
                autoComplete="off"
                value={form.apiKey}
                onChange={(e) => patch({ apiKey: e.target.value })}
                placeholder={
                  form.id
                    ? t("ai.settings.apiKeyKeep")
                    : t("ai.settings.apiKeyPlaceholder")
                }
              />
              <p className="text-xs text-muted-foreground">
                {t("ai.settings.apiKeyHint")}
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="ai-auth-style">
                {t("ai.settings.authStyle")}
              </Label>
              <Select
                value={form.authStyle}
                onValueChange={(v) => patch({ authStyle: v as AiAuthStyle })}
              >
                <SelectTrigger id="ai-auth-style" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {/* Options state the header name actually sent, so users need not guess */}
                  <SelectItem value="apiKey">
                    {standardAuthHeaderName(form.kind)}
                  </SelectItem>
                  <SelectItem value="bearer">Authorization: Bearer</SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("ai.settings.authStyleHint")}
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="ai-model">{t("ai.settings.model")}</Label>
              <div className="flex items-center gap-1.5">
                <Input
                  id="ai-model"
                  list="ai-model-presets"
                  className="font-mono text-xs"
                  value={form.defaultModel}
                  onChange={(e) => patch({ defaultModel: e.target.value })}
                />
                <Button
                  variant="outline"
                  size="sm"
                  className="shrink-0"
                  onClick={() => void onFetchModels()}
                  disabled={busy || !desktop}
                  title={t("ai.settings.fetchModelsHint")}
                >
                  {t("ai.settings.fetchModels")}
                </Button>
              </div>
              <datalist id="ai-model-presets">
                {(catalog.length > 0 ? catalog : presets).map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            </div>

            <ModelOptionList
              catalog={catalog}
              selected={form.models}
              onToggle={(model, checked) =>
                setForm((f) => ({
                  ...f,
                  models: checked
                    ? [...f.models.filter((m) => m !== model), model]
                    : f.models.filter((m) => m !== model),
                }))
              }
              onSelectAll={() =>
                setForm((f) => ({
                  ...f,
                  models: catalog.length > 0 ? [...catalog] : f.models,
                }))
              }
              onClear={() => setForm((f) => ({ ...f, models: [] }))}
            />

            <div className="space-y-1.5">
              <Label>{t("ai.settings.headers")}</Label>
              <div className="space-y-1.5">
                {form.headers.map((h, idx) => (
                  <div key={idx} className="flex items-center gap-1.5">
                    <Input
                      value={h.key}
                      placeholder="X-Tenant"
                      className="h-8 flex-1 font-mono text-xs"
                      onChange={(e) =>
                        patch({
                          headers: form.headers.map((it, i) =>
                            i === idx ? { ...it, key: e.target.value } : it,
                          ),
                        })
                      }
                    />
                    <Input
                      value={h.value}
                      placeholder="value"
                      className="h-8 flex-1 font-mono text-xs"
                      onChange={(e) =>
                        patch({
                          headers: form.headers.map((it, i) =>
                            i === idx ? { ...it, value: e.target.value } : it,
                          ),
                        })
                      }
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      title={t("common.delete")}
                      onClick={() =>
                        patch({
                          headers: form.headers.filter((_, i) => i !== idx),
                        })
                      }
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                ))}
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-muted-foreground"
                  onClick={() =>
                    patch({
                      headers: [...form.headers, { key: "", value: "" }],
                    })
                  }
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("ai.settings.headerAdd")}
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("ai.settings.headersHint")}
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="ai-lang">{t("ai.settings.language")}</Label>
              <Select
                value={prefs.language}
                onValueChange={(v) =>
                  void savePrefs({ ...prefs, language: v as "zh" | "en" })
                }
              >
                <SelectTrigger id="ai-lang" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="zh">{t("ai.settings.lang.zh")}</SelectItem>
                  <SelectItem value="en">{t("ai.settings.lang.en")}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {test && (
              <div
                className={cn(
                  "flex items-start gap-2 rounded-md border px-2.5 py-1.5 text-xs",
                  test.ok
                    ? "border-success/40 bg-success/10 text-success"
                    : "border-destructive/40 bg-destructive/10 text-destructive",
                )}
              >
                {test.ok ? (
                  <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                ) : (
                  <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                )}
                <span className="min-w-0 flex-1 break-words">
                  {test.ok
                    ? t("ai.settings.testOk")
                    : t("ai.settings.testFailed")}{" "}
                  · {test.latencyMs}ms · {test.message}
                </span>
              </div>
            )}

            {error && (
              <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
                {error}
              </p>
            )}
          </div>
        </div>

        <DialogFooter className="gap-1.5 sm:justify-between">
          <div className="flex gap-1.5">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void onTest()}
              disabled={busy || !desktop}
            >
              {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
              {t("ai.settings.testConnection")}
            </Button>
            {form.id && (
              <Button
                variant="destructive"
                size="sm"
                onClick={() => void onDelete()}
                disabled={busy}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("ai.settings.deleteCredential")}
              </Button>
            )}
          </div>
          <div className="flex gap-1.5">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button size="sm" onClick={() => void onSave()} disabled={busy}>
              {t("common.save")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
