// Global "data sources" module: manages DB (MySQL/PostgreSQL/SQLite) and Redis connections in one place.
// List on the left, form on the right: create / edit / test connection / delete; the data persists with the snapshot and is registered with the backend at runtime.
import { useState, type ReactNode } from "react";
import { CheckCircle2, Copy, Plus, Save, Trash2, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import { testDataSource } from "@/lib/bridge";
import type { DataSource, DataSourceKind } from "@/data/types";
import { cn } from "@/lib/utils";

const KIND_META: Record<
  DataSourceKind,
  { label: string; color: string; placeholder: string }
> = {
  mysql: {
    label: "MySQL",
    color: "text-sky-400",
    placeholder: "mysql://user:pwd@host:3306/db",
  },
  postgres: {
    label: "PostgreSQL",
    color: "text-blue-400",
    placeholder: "postgres://user:pwd@host:5432/db",
  },
  sqlite: {
    label: "SQLite",
    color: "text-amber-400",
    placeholder: "sqlite:/path/to/app.db",
  },
  redis: {
    label: "Redis",
    color: "text-rose-400",
    placeholder: "redis://:pwd@host:6379/0",
  },
};

export function DataSourceModule() {
  const { t } = useT();
  const dataSources = useAppStore((s) => s.dataSources);
  const upsert = useAppStore((s) => s.upsertDataSource);
  const remove = useAppStore((s) => s.removeDataSource);
  const blank = useAppStore((s) => s.blankDataSource);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<DataSource | null>(null);
  const [testing, setTesting] = useState(false);
  const [report, setReport] = useState<{ ok: boolean; text: string } | null>(
    null,
  );

  const open = (ds: DataSource) => {
    setSelectedId(ds.id);
    setDraft({ ...ds });
    setReport(null);
  };
  const createNew = () => {
    const ds = blank();
    setSelectedId(ds.id);
    setDraft(ds);
    setReport(null);
  };
  const patch = (p: Partial<DataSource>) =>
    setDraft((d) => (d ? { ...d, ...p } : d));

  const save = async () => {
    if (!draft) return;
    if (!draft.name.trim() || !draft.url.trim()) return;
    upsert({ ...draft });
    setReport({ ok: true, text: t("datasource.saved") });
  };
  const duplicate = () => {
    if (!draft) return;
    const fresh = blank();
    upsert({ ...draft, id: fresh.id, name: `${draft.name} copy` });
  };
  const doTest = async () => {
    if (!draft) return;
    setTesting(true);
    setReport(null);
    try {
      const r = await testDataSource({ config: draft });
      setReport(
        r.ok
          ? {
              ok: true,
              text: `${r.latencyMs}ms${r.detail ? ` · ${r.detail}` : ""}`,
            }
          : { ok: false, text: r.error || t("ds.connectFailed") },
      );
    } catch (e) {
      setReport({
        ok: false,
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setTesting(false);
    }
  };
  const doRemove = () => {
    if (!draft) return;
    remove(draft.id);
    setDraft(null);
    setSelectedId(null);
  };

  const selected = draft ?? null;

  return (
    <div className="flex h-full flex-col">
      {/* Top toolbar */}
      <div className="flex items-center justify-between border-b border-border px-4 py-2.5">
        <div>
          <h1 className="text-sm font-semibold">{t("nav.datasource")}</h1>
          <p className="text-xs text-muted-foreground">
            {t("datasource.subtitle")}
          </p>
        </div>
        <Button size="sm" onClick={createNew}>
          <Plus className="mr-1 h-4 w-4" /> {t("datasource.new")}
        </Button>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* Left: list */}
        <aside className="w-72 shrink-0 overflow-y-auto border-r border-border p-2">
          {dataSources.length === 0 ? (
            <div className="px-2 py-10 text-center text-xs text-muted-foreground">
              {t("datasource.empty")}
            </div>
          ) : (
            <div className="space-y-1">
              {dataSources.map((ds) => {
                const meta = KIND_META[ds.kind];
                const active = ds.id === selectedId;
                return (
                  <button
                    key={ds.id}
                    onClick={() => open(ds)}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors",
                      active
                        ? "border-primary/40 bg-accent/20"
                        : "border-border hover:bg-accent/10",
                    )}
                  >
                    <span
                      className={cn(
                        "inline-block h-2 w-2 shrink-0 rounded-full",
                        ds.enabled
                          ? "bg-emerald-500"
                          : "bg-muted-foreground/40",
                      )}
                      title={ds.enabled ? t("ds.enabled") : t("ds.disabled")}
                    />
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {ds.name || t("ds.unnamed")}
                    </span>
                    <span className={cn("shrink-0 text-xs", meta.color)}>
                      {meta.label}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </aside>

        {/* Right: form */}
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {!selected ? (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
              {t("datasource.selectHint")}
            </div>
          ) : (
            <div className="mx-auto max-w-3xl space-y-4">
              {/* Basic info */}
              <section className="space-y-3 rounded-lg border border-border bg-card p-4">
                <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t("datasource.basic")}
                </h3>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={t("datasource.name")}>
                    <Input
                      className="text-sm"
                      value={selected.name}
                      onChange={(e) => patch({ name: e.target.value })}
                      placeholder={t("datasource.namePlaceholder")}
                    />
                  </Field>
                  <Field label={t("datasource.kind")}>
                    <Select
                      value={selected.kind}
                      onValueChange={(v) =>
                        patch({ kind: v as DataSourceKind })
                      }
                    >
                      <SelectTrigger className="text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {Object.entries(KIND_META).map(([k, m]) => (
                          <SelectItem key={k} value={k}>
                            {m.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                </div>
                <Field
                  label={`${t("datasource.url")}（${t("datasource.envHint")}）`}
                >
                  <Input
                    className="font-mono text-sm"
                    value={selected.url}
                    onChange={(e) => patch({ url: e.target.value })}
                    placeholder={KIND_META[selected.kind].placeholder}
                  />
                </Field>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={t("datasource.username")}>
                    <Input
                      className="font-mono text-sm"
                      value={selected.username ?? ""}
                      onChange={(e) =>
                        patch({ username: e.target.value || undefined })
                      }
                    />
                  </Field>
                  <Field label={t("datasource.password")}>
                    <Input
                      type="password"
                      className="font-mono text-sm"
                      value={selected.password ?? ""}
                      onChange={(e) =>
                        patch({ password: e.target.value || undefined })
                      }
                      placeholder="•••••• / {{env:DB_PWD}}"
                    />
                  </Field>
                </div>
              </section>

              {/* Security */}
              <section className="space-y-3 rounded-lg border border-border bg-card p-4">
                <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t("datasource.security")}
                </h3>
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-sm">{t("datasource.readonly")}</div>
                    <p className="text-xs text-muted-foreground">
                      {t("datasource.readonlyDesc")}
                    </p>
                  </div>
                  <Switch
                    checked={selected.readonly}
                    onCheckedChange={(v) => patch({ readonly: v })}
                  />
                </div>
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-sm">{t("datasource.enabled")}</div>
                    <p className="text-xs text-muted-foreground">
                      {t("datasource.enabledDesc")}
                    </p>
                  </div>
                  <Switch
                    checked={selected.enabled}
                    onCheckedChange={(v) => patch({ enabled: v })}
                  />
                </div>
              </section>

              {/* Connection pool settings */}
              <section className="space-y-3 rounded-lg border border-border bg-card p-4">
                <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t("datasource.pool")}
                </h3>
                <div className="grid grid-cols-3 gap-3">
                  <Field label={t("datasource.maxConns")}>
                    <NumInput
                      value={selected.maxConnections}
                      on={(v) => patch({ maxConnections: v })}
                    />
                  </Field>
                  <Field label={t("datasource.minIdle")}>
                    <NumInput
                      value={selected.minIdle}
                      on={(v) => patch({ minIdle: v })}
                    />
                  </Field>
                  <Field label={t("datasource.idleTtl")}>
                    <NumInput
                      value={selected.idleTtlSecs}
                      on={(v) => patch({ idleTtlSecs: v })}
                    />
                  </Field>
                  <Field label={t("datasource.connectTimeout")}>
                    <NumInput
                      value={selected.connectTimeoutMs}
                      on={(v) => patch({ connectTimeoutMs: v })}
                    />
                  </Field>
                  <Field label={t("datasource.acquireTimeout")}>
                    <NumInput
                      value={selected.acquireTimeoutMs}
                      on={(v) => patch({ acquireTimeoutMs: v })}
                    />
                  </Field>
                  <Field label={t("datasource.queryTimeout")}>
                    <NumInput
                      value={selected.queryTimeoutMs}
                      on={(v) => patch({ queryTimeoutMs: v })}
                    />
                  </Field>
                </div>
              </section>

              {/* Test result */}
              {report && (
                <div
                  className={cn(
                    "flex items-start gap-2 rounded-md border px-3 py-2 text-sm",
                    report.ok
                      ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-400"
                      : "border-destructive/40 bg-destructive/10 text-destructive",
                  )}
                >
                  {report.ok ? (
                    <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
                  ) : (
                    <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
                  )}
                  <span className="min-w-0 break-all">{report.text}</span>
                </div>
              )}

              {/* Action bar */}
              <div className="flex items-center gap-2 border-t border-border pt-4">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={testing}
                  onClick={doTest}
                >
                  {testing ? "…" : <CheckCircle2 className="mr-1 h-4 w-4" />}
                  {testing ? t("datasource.testing") : t("datasource.test")}
                </Button>
                <Button variant="outline" size="sm" onClick={duplicate}>
                  <Copy className="mr-1 h-4 w-4" /> {t("datasource.duplicate")}
                </Button>
                <div className="flex-1" />
                <Button
                  variant="destructive"
                  size="sm"
                  disabled={!selectedId}
                  onClick={doRemove}
                >
                  <Trash2 className="mr-1 h-4 w-4" /> {t("datasource.remove")}
                </Button>
                <Button
                  size="sm"
                  disabled={!selected.name.trim() || !selected.url.trim()}
                  onClick={save}
                >
                  <Save className="mr-1 h-4 w-4" /> {t("datasource.save")}
                </Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      {children}
    </div>
  );
}

function NumInput({ value, on }: { value: number; on: (v: number) => void }) {
  return (
    <Input
      type="number"
      min={0}
      className="h-8 font-mono text-sm"
      value={Number.isFinite(value) ? value : ""}
      onChange={(e) => on(e.target.value === "" ? 0 : Number(e.target.value))}
    />
  );
}
