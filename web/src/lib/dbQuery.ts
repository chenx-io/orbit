// Shared constants and dry-run helpers for database queries (the form component lives in components/common/DbQueryForm.tsx).
//
// Moved into lib to separate constants/pure functions from the React component: this avoids the fast-refresh warning
// and lets assertion editing and pre/post database actions share one set of semantics.
import { isSqlResult, previewQuery } from "@/lib/bridge";
import { t, tFormat } from "@/lib/localeDict";
import type { DbTargetKind } from "@/data/types";

/** Data source dropdown option */
export interface DataSourceOption {
  id: string;
  name: string;
  kind: string;
}

/** Dry-run result (title + displayable text) */
export interface QueryPreview {
  title: string;
  text: string;
}

/** Read-only Redis command allowlist (matching the backend's read-only protection allowlist) */
export const REDIS_COMMANDS = [
  "GET",
  "HGET",
  "EXISTS",
  "TTL",
  "LLEN",
  "SCARD",
  "ZSCORE",
  "TYPE",
  "DBSIZE",
];

/**
 * DB value-read options (aligned with the backend DbTarget), with labels resolved for the
 * active UI locale.
 */
export function dbTargetKinds(): [DbTargetKind, string][] {
  return [
    ["row_count", t("dbQuery.target.row_count")],
    ["scalar", t("dbQuery.target.scalar")],
    ["cell", t("dbQuery.target.cell")],
    ["row", t("dbQuery.target.row")],
    ["json_path", t("dbQuery.target.json_path")],
  ];
}

/** Relational data source kinds (selectable for DB actions/assertions) */
export const SQL_SOURCE_KINDS = ["mysql", "postgres", "sqlite"];

/** Whether a data source is Redis (decides the command vs SQL form and the value-read mode) */
export function isRedisSource(
  dataSources: DataSourceOption[],
  id: string,
): boolean {
  return dataSources.find((d) => d.id === id)?.kind === "redis";
}

/** Run a read-only SQL dry run and return a displayable result */
export async function runSqlPreview(
  datasource: string,
  sql?: string,
): Promise<QueryPreview> {
  const r = await previewQuery({ id: datasource, sql });
  return {
    title: tFormat("dbQuery.dryRun", sql ?? ""),
    text: isSqlResult(r)
      ? [r.columns.join(" | "), ...r.rows.map((x) => x.join(" | "))].join("\n")
      : ((r as { error?: string }).error ?? JSON.stringify(r)),
  };
}

/** Run a read-only Redis dry run and return a displayable result */
export async function runRedisPreview(
  datasource: string,
  command: string,
  args: string[] = [],
): Promise<QueryPreview> {
  const r = (await previewQuery({
    id: datasource,
    redis: [command, ...args],
  })) as {
    value?: string;
    error?: string;
  };
  return {
    title: tFormat("dbQuery.dryRun", command),
    text: r.error ?? String(r.value),
  };
}
