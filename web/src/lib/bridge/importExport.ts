// Import (cURL / Postman / OpenAPI) and export (request code / collection docs / model validation).
// Import/export logic lives in orbit-config::exchange (Rust); this file only bridges channels.
import { apiPost, isTauri, tauriInvoke } from "./client";

// ─── Rich import types ─────────────────────────────
export interface ImportedEndpoint {
  name: string;
  method: string;
  url: string;
  headers: Record<string, string>;
  query_params: Record<string, string>;
  body: string;
  content_type: string;
  group: string;
  summary: string;
  model_ref: string;
  /** Auth type: none / bearer / basic / apikey / oauth2 */
  auth_type: string;
  /** Request parameter name for apiKey */
  auth_key_name: string;
  /** Where apiKey is injected: header / query */
  auth_add_to: string;
  /** Response examples (by status code) */
  responses: ImportedResponse[];
  /** Pre-request script (Postman event prerequest / OpenAPI x- extension) */
  pre_script?: string;
  /** Post-response script (Postman event test / OpenAPI x- extension) */
  post_script?: string;
}

export interface ImportedResponse {
  status: number;
  name: string;
  body: string;
  /** Raw schema (including field descriptions; the frontend renders example field comments) */
  schema?: any;
}

export interface ImportedSchema {
  name: string;
  schema_json: any;
}

export interface ImportParseResult {
  endpoints: ImportedEndpoint[];
  schemas: ImportedSchema[];
}

export async function parseImport(
  format: string,
  input: string,
): Promise<ImportParseResult> {
  if (isTauri()) {
    return tauriInvoke<ImportParseResult>("parse_import", { format, input });
  }
  return apiPost<ImportParseResult>("/api/import/parse", { format, input });
}

/** Import as an automation scenario (returns the full YAML) */
export async function importScenario(
  format: string,
  input: string,
): Promise<{ yaml: string }> {
  return apiPost<{ yaml: string }>("/api/import/scenario", { format, input });
}

// ─── Collection-level export (openapi / swagger / postman) ──────
// The backend data layer (Tauri DataService / Web server DataService) is the authoritative store;
// the frontend only passes the "export range", and the backend queries its own data to build the document (no snapshot upload anymore).

/**
 * Collection-level export; passing workspaceId (with an empty collectionId) exports every collection of that workspace.
 * @param format  document format (openapi / swagger / postman)
 * @param title   document title (request name / folder name / collection name / workspace name)
 * @param collectionId collection id (pass an empty string for a workspace-level export)
 * @param itemId  node id (omitted = the whole collection; folder = a folder; request = a single request)
 * @param workspaceId target workspace (for workspace-level exports)
 */
export async function exportCollection(
  format: string,
  title: string,
  collectionId: string,
  itemId?: string,
  workspaceId?: string,
): Promise<string> {
  if (isTauri()) {
    return tauriInvoke<string>("export_collection", {
      request: {
        format,
        title,
        collectionId,
        itemId,
        workspaceId: workspaceId ?? null,
      },
    });
  }
  const res = await apiPost<{ content: string }>("/api/export/collection", {
    format,
    title,
    collectionId,
    itemId,
    workspaceId: workspaceId ?? null,
  });
  return res.content;
}

export async function readTextFile(path: string): Promise<string> {
  if (isTauri()) {
    return tauriInvoke<string>("read_text_file", { path });
  }
  throw new Error("readTextFile only available in Tauri mode");
}

// ─── Export ──────────────────────────────────────────
export type ExportFormat =
  "curl" | "powershell" | "httpie" | "wget" | "fetch" | "python";

export async function exportRequest(
  request: {
    method: string;
    url: string;
    headers: Record<string, string>;
    body: string;
  },
  format: ExportFormat,
): Promise<string> {
  if (isTauri()) {
    return tauriInvoke<string>("export_request", { request, format });
  }
  return apiPost<string>(`/api/export/${format}`, request);
}

// ─── Response validation (models)──────────────────────────────
export interface ValidationResult {
  valid: boolean;
  errors: { path: string; message: string }[];
}

export async function validateResponseAgainstModel(
  responseBody: string,
  modelFields: string,
): Promise<ValidationResult> {
  if (isTauri()) {
    return tauriInvoke<ValidationResult>("validate_response_against_model", {
      responseBody,
      modelFields,
    });
  }
  return apiPost<ValidationResult>("/api/validate/model", {
    responseBody,
    modelFields,
  });
}
