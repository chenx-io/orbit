// DB action form: choose data source → read-only SQL / Redis command → extraction method → write to variable (single value + multi-column mapping).
// The data-source type decides the query shape (redis → command, others → SQL), avoiding yet another mode selector.
import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  DataSourceSelect,
  DbTargetPicker,
  PreviewBox,
  PreviewButton,
  RedisCommandRow,
} from "@/components/common/DbQueryForm";
import {
  isRedisSource,
  runRedisPreview,
  runSqlPreview,
  type DataSourceOption,
  type QueryPreview,
} from "@/lib/dbQuery";
import { useT } from "@/lib/i18n";
import type { DbActionItem, ActionColumnVar } from "@/data/types";

export function DbActionForm({
  value,
  onChange,
  dataSources,
}: {
  value: DbActionItem;
  onChange: (patch: Partial<DbActionItem>) => void;
  dataSources: DataSourceOption[];
}) {
  const { t } = useT();
  const [preview, setPreview] = useState<QueryPreview | null>(null);
  const [showRetry, setShowRetry] = useState(!!value.retry);
  const redis = isRedisSource(dataSources, value.datasource);
  const columns = value.columns ?? [];

  const run = async () => {
    try {
      setPreview(
        redis
          ? await runRedisPreview(
              value.datasource,
              value.command ?? "GET",
              value.args ?? [],
            )
          : await runSqlPreview(value.datasource, value.sql),
      );
    } catch (e) {
      setPreview({
        title: t("action.db.previewFailed"),
        text: e instanceof Error ? e.message : String(e),
      });
    }
  };

  const setColumns = (next: ActionColumnVar[]) => onChange({ columns: next });

  return (
    <div className="space-y-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Label className="w-12 shrink-0 text-xs text-muted-foreground">
          {t("action.db.datasource")}
        </Label>
        <DataSourceSelect
          value={value.datasource}
          onChange={(id) => onChange({ datasource: id })}
          dataSources={dataSources}
          className="w-56"
        />
        <PreviewButton onClick={run} />
      </div>

      {redis ? (
        <RedisCommandRow
          command={value.command ?? "GET"}
          args={value.args ?? []}
          onCommandChange={(command) => onChange({ command })}
          onArgsChange={(args) => onChange({ args })}
        />
      ) : (
        <Textarea
          className="min-h-16 font-mono text-xs"
          placeholder={t("action.db.sqlPlaceholder")}
          value={value.sql ?? ""}
          onChange={(e) => onChange({ sql: e.target.value })}
        />
      )}

      {!redis && (
        <DbTargetPicker
          value={value.target}
          onChange={(target) => onChange({ target })}
        />
      )}

      {/* Variable writing: single value + multi-column mapping */}
      <div className="space-y-2 rounded-md border border-border/70 p-2">
        <div className="flex flex-wrap items-center gap-2">
          <Label className="text-xs text-muted-foreground">
            {t("action.db.varSingle")}
          </Label>
          <Input
            className="h-7 w-56 font-mono text-xs"
            placeholder={t("action.db.varSinglePlaceholder")}
            value={value.extractVar ?? ""}
            onChange={(e) => onChange({ extractVar: e.target.value })}
          />
        </div>
        {!redis && (
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs text-muted-foreground">
                {t("action.db.multiColumns")}
              </span>
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  className="h-6 w-14 text-xs"
                  placeholder={t("action.db.row")}
                  value={value.row ?? 0}
                  onChange={(e) =>
                    onChange({ row: Number(e.target.value) || 0 })
                  }
                />
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 gap-1 px-1.5 text-xs"
                  onClick={() =>
                    setColumns([...columns, { column: "", var: "" }])
                  }
                >
                  <Plus className="h-3 w-3" /> {t("action.db.addColumn")}
                </Button>
              </div>
            </div>
            {columns.map((c, i) => (
              <div key={i} className="flex items-center gap-1.5">
                <Input
                  className="h-7 flex-1 font-mono text-xs"
                  placeholder={t("action.db.column")}
                  value={c.column}
                  onChange={(e) =>
                    setColumns(
                      columns.map((x, j) =>
                        j === i ? { ...x, column: e.target.value } : x,
                      ),
                    )
                  }
                />
                <span className="text-xs text-muted-foreground">→</span>
                <Input
                  className="h-7 flex-1 font-mono text-xs"
                  placeholder={t("action.db.var")}
                  value={c.var}
                  onChange={(e) =>
                    setColumns(
                      columns.map((x, j) =>
                        j === i ? { ...x, var: e.target.value } : x,
                      ),
                    )
                  }
                />
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 text-muted-foreground hover:text-rose-400"
                  onClick={() => setColumns(columns.filter((_, j) => j !== i))}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        )}
        {!value.extractVar?.trim() &&
          columns.filter((c) => c.var?.trim()).length === 0 && (
            <div className="text-xs text-amber-400/90">
              {t("action.db.noVarHint")}
            </div>
          )}
      </div>

      {/* Polling retry */}
      <div className="rounded-md border border-border/70 p-2">
        <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
          <input
            type="checkbox"
            className="h-3.5 w-3.5 accent-fuchsia-500"
            checked={showRetry}
            onChange={(e) => {
              setShowRetry(e.target.checked);
              if (!e.target.checked) onChange({ retry: undefined });
              else
                onChange({
                  retry: {
                    interval_ms: 200,
                    max_attempts: 5,
                    timeout_ms: null,
                  },
                });
            }}
          />
          {t("action.db.retryTitle")}
        </label>
        {showRetry && value.retry && (
          <div className="mt-2 grid grid-cols-3 gap-2">
            <LabeledNum
              label={t("action.db.retryInterval")}
              value={value.retry.interval_ms}
              onChange={(n) =>
                onChange({ retry: { ...value.retry, interval_ms: n } })
              }
            />
            <LabeledNum
              label={t("action.db.retryAttempts")}
              value={value.retry.max_attempts}
              onChange={(n) =>
                onChange({ retry: { ...value.retry, max_attempts: n } })
              }
            />
            <LabeledNum
              label={t("action.db.retryTimeout")}
              value={value.retry.timeout_ms ?? undefined}
              onChange={(n) =>
                onChange({ retry: { ...value.retry, timeout_ms: n || null } })
              }
            />
          </div>
        )}
      </div>

      <PreviewBox preview={preview} />
    </div>
  );
}

function LabeledNum({
  label,
  value,
  onChange,
}: {
  label: string;
  value?: number;
  onChange: (n: number) => void;
}) {
  return (
    <div className="space-y-1">
      <Label className="text-[10px] text-muted-foreground">{label}</Label>
      <Input
        type="number"
        min={0}
        className="h-7 text-xs"
        value={value ?? ""}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
      />
    </div>
  );
}
