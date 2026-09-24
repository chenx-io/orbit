// Pure helpers for references (the context boundary): file-type filters and size formatting.
// They live under lib because files in components/** may only export components.
import type { AiReferenceKind } from "@/data/aiTypes";
import { t } from "@/lib/localeDict";

/** File-picker filters for external API-definition files (kept in sync with the Rust `REFERENCE_EXTS` allowlist). */
export function definitionFileFilters(): {
  name: string;
  extensions: string[];
}[] {
  return [
    {
      name: t("ai.reference.fileFilter"),
      extensions: [
        "json",
        "yaml",
        "yml",
        "har",
        "http",
        "rest",
        "graphql",
        "gql",
        "proto",
        "md",
        "txt",
      ],
    },
  ];
}

/** Bytes → human-readable size (shown on the reference chips). */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

/** i18n key per reference kind (for the chips' aria/tooltip copy). */
export function referenceKindLabelKey(kind: AiReferenceKind): string {
  return `ai.reference.kind.${kind}`;
}

/**
 * Loose kind string → reference kind.
 *
 * `AiSelection.kind` is a free-form string (historically `request` / `scenario` / `loadTest` / `collection`),
 * while reference kinds are a closed set; unknown values are treated as requests (the request editor is currently the only caller).
 */
export function toReferenceKind(kind: string): AiReferenceKind {
  switch (kind) {
    case "workspace":
    case "collection":
    case "scenario":
    case "file":
      return kind;
    default:
      return "request";
  }
}

/** Request summary (HTTP shows `POST /path`; other protocols show only the protocol name). */
export function describeRequest(req: {
  method?: string;
  url?: string;
  protocol?: string;
}): string {
  const method = typeof req.method === "string" ? req.method : "";
  const url = typeof req.url === "string" ? req.url : "";
  if (method || url) return `${method} ${url}`.trim();
  return req.protocol ?? "";
}

/** `@`-mention candidates (workspace/collection/request/scenario; files use the separate "pick a file" flow). */
export interface ReferenceCandidate {
  kind: AiReferenceKind;
  id: string;
  name: string;
  detail?: string;
}

/** Unique key of a candidate (for `@` list dedup and React keys). */
export function candidateKey(item: ReferenceCandidate): string {
  return `${item.kind}:${item.id}`;
}

/**
 * Build the `@`-mention candidate list from workspace data.
 *
 * Only **referenceable entities** are included (collections/requests/scenarios); the workspace and external files have their own fixed entries,
 * so a "global item" does not sneak into the list and make filtering awkward.
 */
export function buildReferenceCandidates(input: {
  collections: { id: string; name: string }[];
  requests: Record<
    string,
    {
      id: string;
      name: string;
      method?: string;
      url?: string;
      protocol?: string;
    }
  >;
  scenarios: { id: string; name: string; steps: unknown[] }[];
  limit?: number;
}): ReferenceCandidate[] {
  const limit = input.limit ?? 500;
  const out: ReferenceCandidate[] = [];
  for (const c of input.collections) {
    out.push({ kind: "collection", id: c.id, name: c.name });
  }
  for (const r of Object.values(input.requests)) {
    out.push({
      kind: "request",
      id: r.id,
      name: r.name || r.id,
      detail: describeRequest(r),
    });
  }
  for (const s of input.scenarios) {
    out.push({ kind: "scenario", id: s.id, name: s.name });
  }
  return out.slice(0, limit);
}

/** Filter candidates by keyword (an empty keyword returns everything). */
export function filterReferenceCandidates(
  candidates: ReferenceCandidate[],
  query: string,
  limit = 50,
): ReferenceCandidate[] {
  const q = query.trim().toLowerCase();
  const matched = q
    ? candidates.filter(
        (c) =>
          c.name.toLowerCase().includes(q) ||
          (c.detail ?? "").toLowerCase().includes(q),
      )
    : candidates;
  return matched.slice(0, limit);
}
