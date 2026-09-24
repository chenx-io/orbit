// Shared type definitions of the export module.
import type { AuthConfig, ResponseDef } from "@/data/types";

export type ExportSpec = "openapi" | "swagger" | "postman";
export type ExportFileFormat = "json" | "yaml";

/** An endpoint to export: the request itself plus its breadcrumb in the collection tree (collection name → folder names). */
export interface ExportRequest {
  request: import("@/data/types").HttpRequest;
  breadcrumb: string[];
}

/** A normalized request (shared by OpenAPI / Swagger). */
export interface NormReq {
  method: string;
  base: string;
  path: string;
  query: { key: string; value: string }[];
  headers: { key: string; value: string }[];
  pathParams: { key: string; value: string }[];
  /** Request body: mediaType + schema (OpenAPI style). Swagger reuses the schema. */
  bodyMedia: string | null;
  bodySchema: unknown | null;
  auth: AuthConfig;
  responses: ResponseDef[];
}

/** Postman collection item (a recursive structure). */
export interface PMItem {
  name: string;
  item?: PMItem[];
  request?: Record<string, unknown>;
  response?: unknown[];
  description?: string;
}
