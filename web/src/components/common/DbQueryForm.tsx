// Shared database query form pieces: assertion editing (AssertionEditor) and the pre/post "database action"
// (RequestActionsEditor) reuse one data source dropdown, value-read mode and dry-run preview.
//
// Only read-only query UI elements live here; comparators / expected values / variable writes are decided by each caller.
// Constants and dry-run helpers live in `@/lib/dbQuery` (avoiding mixed exports with this file's components).
import { Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import {
  dbTargetKinds,
  REDIS_COMMANDS,
  type DataSourceOption,
  type QueryPreview,
} from "@/lib/dbQuery";
import type { AssertionDbTarget, DbTargetKind } from "@/data/types";

/** Data source dropdown: filtered by `kinds` (e.g. relational kinds for DB, ["redis"] for Redis) */
export function DataSourceSelect({
  value,
  onChange,
  dataSources,
  kinds,
  emptyHint,
  className,
}: {
  value: string;
  onChange: (id: string) => void;
  dataSources: DataSourceOption[];
  /** Allowed data source kinds; omitted means no filtering */
  kinds?: string[];
  emptyHint?: string;
  className?: string;
}) {
  const { t } = useT();
  const list = kinds
    ? dataSources.filter((d) => kinds.includes(d.kind))
    : dataSources;
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className={cn("h-7 text-xs", className ?? "w-48")}>
        <SelectValue placeholder={t("dbQuery.selectSource")} />
      </SelectTrigger>
      <SelectContent>
        {list.length === 0 && (
          <div className="px-2 py-1 text-xs text-muted-foreground">
            {emptyHint ?? t("dbQuery.noSource")}
          </div>
        )}
        {list.map((d) => (
          <SelectItem key={d.id} value={d.id}>
            {d.name || d.id}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** Value-read picker (row_count / scalar / cell / row / json_path plus row / column / path) */
export function DbTargetPicker({
  value,
  onChange,
}: {
  value?: AssertionDbTarget;
  onChange: (next: AssertionDbTarget) => void;
}) {
  const { t } = useT();
  const kind = value?.type ?? "scalar";
  const patch = (p: Partial<AssertionDbTarget>) =>
    onChange({ type: kind, ...(value ?? {}), ...p });
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Label className="text-xs text-muted-foreground">
        {t("dbQuery.target")}
      </Label>
      <Select
        value={kind}
        onValueChange={(v) => patch({ type: v as DbTargetKind })}
      >
        <SelectTrigger className="h-7 w-36 text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {dbTargetKinds().map(([k, l]) => (
            <SelectItem key={k} value={k}>
              {l}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {kind !== "row_count" && kind !== "scalar" && (
        <Input
          type="number"
          className="h-7 w-16 text-xs"
          placeholder={t("dbQuery.rowPlaceholder")}
          value={value?.row ?? 0}
          onChange={(e) => patch({ row: Number(e.target.value) || 0 })}
        />
      )}
      {kind === "cell" && (
        <Input
          className="h-7 w-32 font-mono text-xs"
          placeholder={t("action.db.column")}
          value={value?.column ?? ""}
          onChange={(e) => patch({ column: e.target.value })}
        />
      )}
      {kind === "json_path" && (
        <Input
          className="h-7 w-44 font-mono text-xs"
          placeholder={t("dbQuery.pathPlaceholder")}
          value={value?.path ?? ""}
          onChange={(e) => patch({ path: e.target.value })}
        />
      )}
    </div>
  );
}

/** Read-only query dry-run button */
export function PreviewButton({ onClick }: { onClick: () => void }) {
  const { t } = useT();
  return (
    <Button
      variant="outline"
      size="sm"
      className="h-7 gap-1 text-xs"
      onClick={onClick}
    >
      <Play className="h-3 w-3" /> {t("dbQuery.dryRunBtn")}
    </Button>
  );
}

/** Dry-run result panel */
export function PreviewBox({ preview }: { preview: QueryPreview | null }) {
  if (!preview) return null;
  return (
    <div className="rounded-md border border-border bg-muted/30 p-2 text-xs">
      <div className="mb-1 font-medium">{preview.title}</div>
      <pre className="max-h-44 overflow-auto whitespace-pre-wrap font-mono">
        {preview.text}
      </pre>
    </div>
  );
}

/** Redis command plus arguments row (space-separated arguments, supports {{var}}) */
export function RedisCommandRow({
  command,
  args,
  onCommandChange,
  onArgsChange,
}: {
  command: string;
  args: string[];
  onCommandChange: (c: string) => void;
  onArgsChange: (a: string[]) => void;
}) {
  const { t } = useT();
  return (
    <div className="flex items-center gap-2">
      <Select value={command || "GET"} onValueChange={onCommandChange}>
        <SelectTrigger className="h-7 w-28 text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {REDIS_COMMANDS.map((c) => (
            <SelectItem key={c} value={c}>
              {c}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Input
        className="h-7 flex-1 font-mono text-xs"
        placeholder={t("dbQuery.argsPlaceholder")}
        value={args.join(" ")}
        onChange={(e) =>
          onArgsChange(
            e.target.value
              .split(/\s+/)
              .map((s) => s.trim())
              .filter(Boolean),
          )
        }
      />
    </div>
  );
}
