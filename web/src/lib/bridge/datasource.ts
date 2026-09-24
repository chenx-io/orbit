// Data source command bridge (dual channel: Tauri desktop + orbit-server HTTP).
// Kept isomorphic with the backend commands/datasource.rs (Tauri) and /api/datasources/* (HTTP).
import type { DataSource } from "@/data/types";
import { apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

/** Test connection report (the backend DsTestReport) */
export interface DataSourceTestReport {
  id: string;
  name: string;
  kind: string;
  ok: boolean;
  latencyMs: number;
  /** Server info on success (e.g. the SQLite/PG/Redis version) */
  detail?: string | null;
  /** Failure reason */
  error?: string | null;
}

/** SQL dry-run result */
export interface SqlPreview {
  columns: string[];
  rows: string[][];
  rowsAffected: number;
  elapsedMs: number;
  error?: string;
}

/** Redis dry-run result */
export interface RedisPreview {
  value?: string;
  error?: string;
}

export async function listDataSources(): Promise<DataSource[]> {
  if (isTauri()) {
    return tauriInvoke<DataSource[]>("ds_list");
  }
  return apiGet<DataSource[]>("/api/datasources");
}

export async function upsertDataSource(cfg: DataSource): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("ds_upsert", { cfg });
    return;
  }
  await apiPost<any>("/api/datasources/upsert", cfg);
}

export async function removeDataSource(id: string): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("ds_remove", { id });
    return;
  }
  await apiPost<any>("/api/datasources/remove", { id });
}

export async function testDataSource(input: {
  id?: string;
  config?: DataSource;
}): Promise<DataSourceTestReport> {
  if (isTauri()) {
    return tauriInvoke<DataSourceTestReport>("ds_test", {
      req: { id: input.id ?? null, config: input.config ?? null },
    });
  }
  return apiPost<DataSourceTestReport>("/api/datasources/test", {
    id: input.id ?? null,
    config: input.config ?? null,
  });
}

/** Dry run: SQL (relational) or a Redis command. One of id or config. */
export async function previewQuery(input: {
  id?: string;
  config?: DataSource;
  sql?: string;
  redis?: string[];
}): Promise<SqlPreview | RedisPreview> {
  if (isTauri()) {
    return tauriInvoke<any>("ds_preview_query", {
      req: {
        id: input.id ?? null,
        config: input.config ?? null,
        sql: input.sql ?? null,
        redis: input.redis ?? null,
      },
    });
  }
  return apiPost<any>("/api/datasources/query", {
    id: input.id ?? null,
    config: input.config ?? null,
    sql: input.sql ?? null,
    redis: input.redis ?? null,
  });
}

export function isSqlResult(r: SqlPreview | RedisPreview): r is SqlPreview {
  return Array.isArray((r as SqlPreview).columns);
}
