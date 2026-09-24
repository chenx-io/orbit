// CSV parsing (an RFC4180 subset): BOM stripping, CRLF/CR normalization, quoted fields and "" escaping, and newlines inside fields.
// Pure functions with no dependencies, reused by CSV-driven test data.
import type { DataSetMode } from "@/data/types";
import { t } from "@/lib/localeDict";

export interface ParsedCsv {
  columns: string[];
  /** Data rows (header excluded), each aligned with the columns order */
  rows: string[][];
  rowCount: number;
}

function stripBom(s: string): string {
  return s.charCodeAt(0) === 0xfeff ? s.slice(1) : s;
}

/**
 * Parse CSV text. The first row is the header (column names).
 * Empty content / header-only input throws, and the caller surfaces it to the user (avoiding a silent false success of "ran 0 rows").
 */
export function parseCsvText(text: string): ParsedCsv {
  const src = stripBom(text ?? "");
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let inQuotes = false;
  let sawContent = false;
  let i = 0;

  while (i < src.length) {
    const ch = src[i];
    if (inQuotes) {
      if (ch === '"') {
        if (src[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        inQuotes = false;
        i += 1;
        continue;
      }
      field += ch;
      i += 1;
      continue;
    }
    if (ch === '"' && field === "") {
      inQuotes = true;
      sawContent = true;
      i += 1;
      continue;
    }
    if (ch === ",") {
      row.push(field);
      field = "";
      sawContent = true;
      i += 1;
      continue;
    }
    if (ch === "\r") {
      // Normalize CRLF / CR: a bare CR ends the line, CRLF is finished off by the following \n
      if (src[i + 1] === "\n") {
        i += 1; // let the \n branch handle it
        continue;
      }
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
      sawContent = true;
      i += 1;
      continue;
    }
    if (ch === "\n") {
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
      sawContent = true;
      i += 1;
      continue;
    }
    field += ch;
    sawContent = true;
    i += 1;
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }

  // Drop trailing blank lines (empty records produced by the trailing newline)
  while (
    rows.length > 0 &&
    rows[rows.length - 1].every((v) => v.trim() === "")
  ) {
    rows.pop();
  }

  if (!sawContent || rows.length === 0) {
    throw new Error(t("csv.empty"));
  }
  const columns = rows[0].map((c, idx) => c.trim() || `col${idx + 1}`);
  const dataRows = rows
    .slice(1)
    .filter((r) => r.some((v) => v.trim() !== ""))
    .map((r) => columns.map((_, idx) => r[idx] ?? ""));
  if (dataRows.length === 0) {
    throw new Error(t("csv.headerOnly"));
  }
  return { columns, rows: dataRows, rowCount: dataRows.length };
}

/** Row → variable dictionary (keyed by the header column names) */
export function csvRowToVars(
  columns: string[],
  row: string[],
): Record<string, string> {
  const vars: Record<string, string> = {};
  columns.forEach((c, idx) => {
    vars[c] = row[idx] ?? "";
  });
  return vars;
}

/**
 * Build the row execution order (returns an array of row indices):
 * - sequential: original order
 * - shuffle: shuffled traversal (every row exactly once)
 * - random: pick a random row each time (may repeat)
 */
export function orderRowIndexes(rowCount: number, mode: DataSetMode): number[] {
  if (rowCount <= 0) return [];
  const idx = Array.from({ length: rowCount }, (_, i) => i);
  if (mode === "shuffle") {
    for (let i = idx.length - 1; i > 0; i -= 1) {
      const j = Math.floor(Math.random() * (i + 1));
      [idx[i], idx[j]] = [idx[j], idx[i]];
    }
    return idx;
  }
  if (mode === "random") {
    return idx.map(() => Math.floor(Math.random() * rowCount));
  }
  return idx;
}

/** For previews: take only the first `limit` rows (avoids rendering stalls on large tables) */
export function previewRows(rows: string[][], limit = 50): string[][] {
  return rows.slice(0, limit);
}

/** Text size warning threshold (warned before writing) */
export const DATASET_SIZE_WARN_BYTES = 500 * 1024;
