// Assertion editor (the shared body of a request's "Assertions" tab).
// The layout mirrors mock expectations: the list is compact cards (name / kind / summary / enable / delete)
// plus an edit dialog instead of flattening every parameter, and it writes back to the request only on
// save (the wire format matches the backend orbit-config::Check JSON).
import { useState, type ReactNode } from "react";
import { Copy, Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { cn } from "@/lib/utils";
import type { Assertion, AssertionKind } from "@/data/types";
import {
  DataSourceSelect,
  DbTargetPicker,
  PreviewBox,
  PreviewButton,
  RedisCommandRow,
} from "./DbQueryForm";
import {
  runRedisPreview,
  runSqlPreview,
  SQL_SOURCE_KINDS,
  type DataSourceOption,
  type QueryPreview,
} from "@/lib/dbQuery";
import { t, tFormat } from "@/lib/localeDict";
import { useT, type TKey } from "@/lib/i18n";

const COMPARATORS = [
  ["equal", "assert.cmp.equal"],
  ["not_equal", "assert.cmp.not_equal"],
  ["contains", "assert.cmp.contains"],
  ["not_contains", "assert.cmp.not_contains"],
  ["exists", "assert.cmp.exists"],
  ["matches", "assert.cmp.matches"],
  ["gt", "assert.cmp.gt"],
  ["lt", "assert.cmp.lt"],
] as const satisfies readonly (readonly [string, TKey])[];

/** i18n key per assertion kind. JSONPath / JMESPath / XPath / JSON Schema / Redis stay literal. */
const KIND_LABEL_KEY: Record<AssertionKind, string> = {
  status: "assert.kind.status",
  body_contains: "assert.kind.body_contains",
  duration_lt: "assert.kind.duration_lt",
  size_lt: "assert.kind.size_lt",
  jsonpath: "JSONPath",
  jmespath: "JMESPath",
  regex: "assert.kind.regex",
  xpath: "XPath",
  header: "assert.kind.header",
  css_selector: "assert.kind.css_selector",
  jsonschema: "JSON Schema",
  db: "assert.kind.db",
  redis: "Redis",
};

function blank(type: AssertionKind): Assertion {
  const base: Assertion = { type, meta: { enabled: true } };
  switch (type) {
    case "status":
      return { ...base, value: 200 };
    case "jsonpath":
      return { ...base, path: "", comparator: "equal", expected: "" };
    case "db":
      return {
        ...base,
        datasource: "",
        sql: "SELECT 1",
        target: { type: "scalar" },
        comparator: "equal",
        expected: "1",
        hard: true,
      };
    case "redis":
      return {
        ...base,
        datasource: "",
        command: "GET",
        args: [],
        comparator: "not_contains",
        expected: "null",
        hard: true,
      };
    default:
      return { ...base, value: type === "duration_lt" ? "1s" : "" };
  }
}

function kindBadgeClass(type: AssertionKind): string {
  if (type === "db") return "border-sky-500/30 text-sky-400";
  if (type === "redis") return "border-rose-500/30 text-rose-400";
  return "text-slate-300";
}

/** Summary line: matches the mock expectation card's condition summary (single line of gray mono text). */
function summarize(a: Assertion, dsName: (id?: string) => string): string {
  const cmp = t(`assert.cmp.${a.comparator ?? "equal"}` as TKey);
  /** `field comparator "expected"` — the shape shared by most kinds. */
  const pair = (field: string, expected = a.expected ?? "") =>
    tFormat("assert.summary.pair", field, cmp, expected);
  switch (a.type) {
    case "status":
      return tFormat("assert.summary.status", String(a.value ?? ""));
    case "body_contains":
      return tFormat("assert.summary.bodyContains", String(a.value ?? ""));
    case "duration_lt":
      return tFormat("assert.summary.durationLt", String(a.value ?? ""));
    case "size_lt":
      return tFormat("assert.summary.sizeLt", String(a.value ?? ""));
    case "jsonpath":
      return pair(a.path || t("assert.summary.pathEmpty"));
    case "jmespath":
      return pair(a.expression || t("assert.summary.exprEmpty"));
    case "regex":
      return tFormat("assert.summary.regex", a.pattern ?? "");
    case "xpath":
      return pair(a.path || t("assert.summary.pathEmpty"));
    case "header":
      return pair(a.name || t("assert.summary.headerEmpty"));
    case "css_selector":
      return pair(a.selector || t("assert.summary.selectorEmpty"));
    case "jsonschema":
      return t("assert.summary.jsonschema");
    case "db": {
      const tgt = a.target;
      const mode =
        tgt?.type === "row_count"
          ? t("assert.summary.dbRowCount")
          : tgt?.type === "cell"
            ? tFormat("assert.summary.dbCell", tgt.row ?? 0, tgt.column || "?")
            : tgt?.type === "row"
              ? tFormat("assert.summary.dbRow", tgt.row ?? 0)
              : tgt?.type === "json_path"
                ? tFormat(
                    "assert.summary.dbJsonPath",
                    tgt.row ?? 0,
                    tgt.path || "?",
                  )
                : t("assert.summary.dbFirstCell");
      return `${dsName(a.datasource)} · ${a.sql || ""} · ${mode} ${cmp} "${a.expected ?? ""}"`;
    }
    case "redis":
      return `${dsName(a.datasource)} · ${a.command ?? ""} ${(a.args ?? []).join(" ")} · ${cmp} "${a.expected ?? ""}"`;
    default:
      return "";
  }
}

function AssertionCard({
  a,
  dsName,
  onEdit,
  onToggleEnabled,
  onDelete,
  onDuplicate,
}: {
  a: Assertion;
  dsName: (id?: string) => string;
  onEdit: () => void;
  onToggleEnabled: (v: boolean) => void;
  onDelete: () => void;
  onDuplicate: () => void;
}) {
  const { t } = useT();
  return (
    <div className="rounded-md border border-border bg-card">
      <div className="flex items-center gap-2 px-2.5 py-2">
        <Badge
          variant="outline"
          className={cn(
            "w-24 shrink-0 justify-center font-normal",
            kindBadgeClass(a.type),
          )}
        >
          {t(KIND_LABEL_KEY[a.type] as TKey)}
        </Badge>
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
          onClick={onEdit}
          title={t("assert.edit")}
        >
          <span className="truncate text-xs font-medium text-foreground">
            {a.meta?.name || t(KIND_LABEL_KEY[a.type] as TKey)}
          </span>
        </button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onDuplicate}
          title={t("common.duplicate")}
        >
          <Copy className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onEdit}
          title={t("common.edit")}
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>
        <Switch
          checked={a.meta?.enabled !== false}
          onCheckedChange={onToggleEnabled}
          aria-label={t("assert.toggle")}
        />
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground hover:text-destructive"
          onClick={onDelete}
          title={t("common.delete")}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="truncate border-t border-border px-3 py-1.5 font-mono text-xs text-muted-foreground">
        {summarize(a, dsName)}
      </div>
    </div>
  );
}

// ─── Main component ────────────────────────────────────────
export function AssertionEditor({
  value,
  onChange,
}: {
  value: Assertion[];
  onChange: (a: Assertion[]) => void;
}) {
  const { t, format } = useT();
  const dataSources = useAppStore((s) => s.dataSources);
  const [dialog, setDialog] = useState<{
    open: boolean;
    draft: Assertion;
    idx: number | null;
  } | null>(null);

  const openCreate = (type: AssertionKind) =>
    setDialog({ open: true, draft: blank(type), idx: null });
  const openEdit = (a: Assertion, idx: number) =>
    setDialog({
      open: true,
      idx,
      draft: {
        ...a,
        meta: { ...a.meta },
        target: a.target ? { ...a.target } : undefined,
        retry: a.retry ? { ...a.retry } : undefined,
      },
    });
  const handleSave = () => {
    if (!dialog) return;
    if (dialog.idx === null) {
      onChange([...value, dialog.draft]);
    } else {
      onChange(value.map((x, i) => (i === dialog.idx ? dialog.draft : x)));
    }
    setDialog(null);
  };

  const enabledCount = value.filter((a) => a.meta?.enabled !== false).length;
  // Summaries / dropdowns prefer the data source name (falling back to the id, since ds-xxx is opaque)
  const dsName = (id?: string): string => {
    if (!id) return t("assert.datasourceEmpty");
    return dataSources.find((d) => d.id === id)?.name || id;
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          {format("assert.enabledCount", enabledCount, value.length)}
        </span>
      </div>
      <Button
        variant="outline"
        size="sm"
        className="w-full gap-1 text-xs"
        onClick={() => openCreate("status")}
      >
        <Plus className="h-3 w-3" /> {t("assert.add")}
      </Button>
      {value.length === 0 ? (
        <div className="py-10 text-center text-xs text-muted-foreground">
          {t("assert.emptyHint")}
        </div>
      ) : (
        <div className="space-y-2">
          {value.map((a, i) => (
            <AssertionCard
              key={i}
              a={a}
              dsName={dsName}
              onEdit={() => openEdit(a, i)}
              onToggleEnabled={(v) =>
                onChange(
                  value.map((x, idx) =>
                    idx === i
                      ? { ...x, meta: { enabled: v, name: x.meta?.name } }
                      : x,
                  ),
                )
              }
              onDelete={() => onChange(value.filter((_, idx) => idx !== i))}
              onDuplicate={() => onChange([...value, { ...a }])}
            />
          ))}
        </div>
      )}

      {dialog && (
        <AssertionDialog
          draft={dialog.draft}
          dataSources={dataSources}
          title={dialog.idx === null ? t("assert.add") : t("assert.edit")}
          onCancel={() => setDialog(null)}
          onSave={handleSave}
          onChanged={(d) =>
            setDialog({ open: true, draft: d, idx: dialog.idx })
          }
        />
      )}
    </div>
  );
}

function AssertionDialog({
  draft,
  dataSources,
  title,
  onCancel,
  onSave,
  onChanged,
}: {
  draft: Assertion;
  dataSources: { id: string; name: string; kind: string }[];
  title: string;
  onCancel: () => void;
  onSave: () => void;
  onChanged: (a: Assertion) => void;
}) {
  const { t } = useT();
  const patch = (p: Partial<Assertion>) => onChanged({ ...draft, ...p });
  return (
    <Dialog open onOpenChange={(v) => !v && onCancel()}>
      <DialogContent className="flex h-[85vh] flex-col overflow-hidden sm:max-w-3xl">
        <DialogHeader className="flex-shrink-0">
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto pr-3">
          <div className="flex items-center gap-2">
            <Label className="w-12 shrink-0 text-xs text-muted-foreground">
              {t("assert.type")}
            </Label>
            <Select
              value={draft.type}
              onValueChange={(v) => onChanged(blank(v as AssertionKind))}
            >
              <SelectTrigger className="h-7 w-40 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(KIND_LABEL_KEY).map(([k, key]) => (
                  <SelectItem key={k} value={k}>
                    {t(key as TKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Input
              className="h-7 flex-1 text-xs"
              placeholder={t("assert.namePlaceholder")}
              value={draft.meta?.name ?? ""}
              onChange={(e) =>
                patch({
                  meta: {
                    enabled: draft.meta?.enabled !== false,
                    name: e.target.value || undefined,
                  },
                })
              }
            />
          </div>

          <ParamsForm draft={draft} patch={patch} dataSources={dataSources} />

          {(draft.type === "db" || draft.type === "redis") && (
            <div className="rounded border border-border/70 p-2">
              <div className="mb-2 text-xs font-medium text-muted-foreground">
                {t("assert.retrySection")}
              </div>
              <div className="grid grid-cols-3 gap-2">
                <Num
                  label={t("assert.retry.interval")}
                  v={draft.retry?.interval_ms}
                  set={(n) =>
                    patch({ retry: { ...draft.retry, interval_ms: n } })
                  }
                />
                <Num
                  label={t("assert.retry.maxCount")}
                  v={draft.retry?.max_attempts}
                  set={(n) =>
                    patch({ retry: { ...draft.retry, max_attempts: n } })
                  }
                />
                <Num
                  label={t("assert.retry.timeout")}
                  v={draft.retry?.timeout_ms ?? undefined}
                  set={(n) =>
                    patch({ retry: { ...draft.retry, timeout_ms: n || null } })
                  }
                />
              </div>
              <div className="mt-2 flex items-center gap-2">
                <Input
                  className="h-7 flex-1 text-xs"
                  placeholder={t("assert.resultVarPlaceholder")}
                  value={draft.extract_var ?? ""}
                  onChange={(e) =>
                    patch({ extract_var: e.target.value || undefined })
                  }
                />
                <label className="flex items-center gap-1 text-xs text-muted-foreground">
                  {t("assert.hard")}
                  <Switch
                    checked={draft.hard ?? true}
                    onCheckedChange={(v) => patch({ hard: v })}
                  />
                </label>
              </div>
            </div>
          )}
        </div>
        <DialogFooter className="flex-shrink-0">
          <Button variant="outline" size="sm" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button size="sm" onClick={onSave}>
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ParamsForm({
  draft,
  patch,
  dataSources,
}: {
  draft: Assertion;
  patch: (p: Partial<Assertion>) => void;
  dataSources: { id: string; name: string; kind: string }[];
}) {
  const { t } = useT();
  switch (draft.type) {
    case "status":
      return (
        <Field label={t("assert.expectStatus")}>
          <Input
            type="number"
            className="h-7 w-28 text-xs"
            value={draft.value ?? 200}
            onChange={(e) => patch({ value: Number(e.target.value) })}
          />
        </Field>
      );
    case "jsonpath":
    case "jmespath":
    case "xpath":
      return (
        <div className="space-y-2">
          <Field
            label={
              draft.type === "jmespath"
                ? t("assert.jmesPathLabel")
                : draft.type === "xpath"
                  ? t("assert.xPathLabel")
                  : t("assert.jsonPathLabel")
            }
          >
            <Input
              className="h-7 w-full font-mono text-xs"
              value={draft.path ?? draft.expression ?? ""}
              onChange={(e) =>
                patch(
                  draft.type === "jmespath"
                    ? { expression: e.target.value }
                    : { path: e.target.value },
                )
              }
            />
          </Field>
          <ComparatorRow a={draft} patch={patch} />
        </div>
      );
    case "header":
      return (
        <div className="space-y-2">
          <Field label={t("assert.headerName")}>
            <Input
              className="h-7 w-full text-xs"
              placeholder={t("assert.headerNamePlaceholder")}
              value={draft.name ?? ""}
              onChange={(e) => patch({ name: e.target.value })}
            />
          </Field>
          <ComparatorRow a={draft} patch={patch} />
        </div>
      );
    case "css_selector":
      return (
        <div className="space-y-2">
          <Field label={t("assert.cssSelector")}>
            <Input
              className="h-7 w-full font-mono text-xs"
              value={draft.selector ?? ""}
              onChange={(e) => patch({ selector: e.target.value })}
            />
          </Field>
          <ComparatorRow a={draft} patch={patch} />
        </div>
      );
    case "regex":
      return (
        <Field label={t("assert.regexLabel")}>
          <Input
            className="h-7 w-full font-mono text-xs"
            value={draft.pattern ?? ""}
            onChange={(e) => patch({ pattern: e.target.value })}
          />
        </Field>
      );
    case "jsonschema":
      return (
        <Field label="JSON Schema">
          <Textarea
            className="min-h-28 font-mono text-xs"
            placeholder='{"type":"object",...}'
            value={draft.schema ?? ""}
            onChange={(e) => patch({ schema: e.target.value })}
          />
        </Field>
      );
    case "body_contains":
      return (
        <Field label={t("assert.containsText")}>
          <Input
            className="h-7 w-full text-xs"
            value={String(draft.value ?? "")}
            onChange={(e) => patch({ value: e.target.value })}
          />
        </Field>
      );
    case "duration_lt":
    case "size_lt":
      return (
        <Field
          label={
            draft.type === "duration_lt"
              ? t("assert.maxDuration")
              : t("assert.maxSize")
          }
        >
          <Input
            className="h-7 w-40 font-mono text-xs"
            value={String(draft.value ?? "")}
            onChange={(e) =>
              patch({
                value:
                  draft.type === "size_lt"
                    ? Number(e.target.value)
                    : e.target.value,
              })
            }
          />
        </Field>
      );
    case "db":
      return <DbForm draft={draft} patch={patch} dataSources={dataSources} />;
    case "redis":
      return (
        <RedisForm draft={draft} patch={patch} dataSources={dataSources} />
      );
    default:
      return null;
  }
}

function ComparatorRow({
  a,
  patch,
}: {
  a: Assertion;
  patch: (p: Partial<Assertion>) => void;
}) {
  const { t } = useT();
  return (
    <div className="flex items-center gap-2">
      <Select
        value={a.comparator ?? "equal"}
        onValueChange={(v) =>
          patch({ comparator: v as Assertion["comparator"] })
        }
      >
        <SelectTrigger className="h-7 w-32 text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {COMPARATORS.map(([k, key]) => (
            <SelectItem key={k} value={k}>
              {t(key)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Input
        className="h-7 flex-1 font-mono text-xs"
        placeholder={t("assert.expectedPlaceholder")}
        value={a.expected ?? ""}
        onChange={(e) => patch({ expected: e.target.value })}
      />
    </div>
  );
}

function DbForm({
  draft,
  patch,
  dataSources,
}: {
  draft: Assertion;
  patch: (p: Partial<Assertion>) => void;
  dataSources: DataSourceOption[];
}) {
  const { t } = useT();
  const [preview, setPreview] = useState<QueryPreview | null>(null);
  const run = async () => {
    try {
      setPreview(await runSqlPreview(draft.datasource ?? "", draft.sql));
    } catch (e) {
      setPreview({
        title: t("assert.previewFailed"),
        text: e instanceof Error ? e.message : String(e),
      });
    }
  };
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <Label className="w-12 shrink-0 text-xs text-muted-foreground">
          {t("action.db.datasource")}
        </Label>
        <DataSourceSelect
          value={draft.datasource ?? ""}
          onChange={(v) => patch({ datasource: v })}
          dataSources={dataSources}
          kinds={SQL_SOURCE_KINDS}
          emptyHint={t("assert.needSqlSource")}
        />
        <PreviewButton onClick={run} />
      </div>
      <Field label={t("assert.readonlySql")}>
        <Textarea
          className="min-h-20 font-mono text-xs"
          placeholder="SELECT status FROM t WHERE id = '${orderId}'"
          value={draft.sql ?? ""}
          onChange={(e) => patch({ sql: e.target.value })}
        />
      </Field>
      <DbTargetPicker
        value={draft.target}
        onChange={(target) => patch({ target })}
      />
      <ComparatorRow a={draft} patch={patch} />
      <PreviewBox preview={preview} />
    </div>
  );
}

function RedisForm({
  draft,
  patch,
  dataSources,
}: {
  draft: Assertion;
  patch: (p: Partial<Assertion>) => void;
  dataSources: DataSourceOption[];
}) {
  const { t } = useT();
  const [preview, setPreview] = useState<QueryPreview | null>(null);
  const run = async () => {
    try {
      setPreview(
        await runRedisPreview(
          draft.datasource ?? "",
          draft.command ?? "GET",
          draft.args ?? [],
        ),
      );
    } catch (e) {
      setPreview({
        title: t("assert.previewFailed"),
        text: e instanceof Error ? e.message : String(e),
      });
    }
  };
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <Label className="w-12 shrink-0 text-xs text-muted-foreground">
          {t("action.db.datasource")}
        </Label>
        <DataSourceSelect
          value={draft.datasource ?? ""}
          onChange={(v) => patch({ datasource: v })}
          dataSources={dataSources}
          kinds={["redis"]}
          emptyHint={t("assert.needRedisSource")}
        />
        <PreviewButton onClick={run} />
      </div>
      <RedisCommandRow
        command={draft.command ?? "GET"}
        args={draft.args ?? []}
        onCommandChange={(command) => patch({ command })}
        onArgsChange={(args) => patch({ args })}
      />
      <ComparatorRow a={draft} patch={patch} />
      <PreviewBox preview={preview} />
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

function Num({
  label,
  v,
  set,
}: {
  label: string;
  v?: number;
  set: (n: number) => void;
}) {
  const { t } = useT();
  return (
    <div className="space-y-1">
      <Label className="text-[10px] text-muted-foreground">{label}</Label>
      <Input
        type="number"
        min={0}
        className="h-7 text-xs"
        value={v ?? ""}
        placeholder={t("assert.notSet")}
        onChange={(e) => set(Number(e.target.value) || 0)}
      />
    </div>
  );
}
