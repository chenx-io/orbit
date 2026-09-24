// Un-interpolated request template (isomorphic with the Rust `orbit_engine::request_build::RequestTemplate`).
//
// One-off debugging sends a "template-state" request to the engine: placeholders (`{{var}}` / `{{$...}}`) and structured params
// are **kept verbatim**, and the engine performs "pre-interpolation script → interpolation → encoding/assembly → post-interpolation script".
// This file only does pure structural conversion (no variable resolution, no encoding); the wire format is always snake_case.
import type { HttpRequest, KeyValue } from "@/data/types";
import { buildAuthHeaders, buildDefaultHeaders } from "@/lib/request";
import { getActiveBody, stripBodyComments } from "@/lib/requestBody";

/** Text body modes (matching Rust `TextBodyMode`) */
export type TextBodyFormat = "json" | "xml" | "raw";

/** Form field template (matching Rust `FormParam`) */
export interface WireFormParam {
  key: string;
  value?: string;
  /** Tauri path mode: the file's real absolute path; the engine reads it from disk */
  file_path?: string;
  /** Browser preview mode: the file content as base64 (the fallback when no real path is available) */
  base64?: string;
  file_type?: string;
  filename?: string;
}

/** Body template (`mode`-tagged, matching Rust `BodyTemplate`) */
export type WireBodyTemplate =
  | { mode: "none" }
  | {
      mode: "text";
      format: TextBodyFormat;
      /** Initial body (comments already stripped); `{{var}}` is kept verbatim */
      text: string;
      /** Effective in raw mode only (defaults to text/plain) */
      content_type?: string;
    }
  | { mode: "urlencoded"; params: [string, string][] }
  | { mode: "multipart"; params: WireFormParam[] }
  | { mode: "binary"; file_path?: string; base64?: string };

/** Un-interpolated request template (matching Rust `RequestTemplate`) */
export interface WireRequestTemplate {
  url: string;
  path_params: [string, string][];
  query_params: [string, string][];
  /** Automatic default headers (Accept / User-Agent / Connection / Cache-Control / Cookie ...) */
  default_headers: [string, string][];
  /** Auth headers (Bearer / Basic / apikey-in-header) */
  auth_headers: [string, string][];
  /** User-configured headers (highest priority; an explicit Content-Type is never overridden) */
  user_headers: [string, string][];
  cookies: [string, string][];
  body: WireBodyTemplate;
}

const kvPairs = (items: KeyValue[] | undefined): [string, string][] =>
  (items ?? [])
    .filter((p) => p.enabled && p.key)
    .map((p) => [p.key, p.value ?? ""] as [string, string]);

/** Build the body template for a bodyMode. */
function buildBodyTemplate(req: HttpRequest): WireBodyTemplate {
  switch (req.bodyMode) {
    case "none":
      return { mode: "none" };
    case "json":
    case "xml":
    case "raw": {
      // Strip comments before sending (JSON `//` and `/* */`, XML `<!-- -->`) so only valid data is sent;
      // stripping happens before interpolation so a `{{var}}` inside a comment is not misparsed.
      const raw = getActiveBody(req);
      const text = raw ? stripBodyComments(raw, req.bodyMode) : "";
      return {
        mode: "text",
        format: req.bodyMode,
        text,
        // The Content-Type of json / xml is derived strictly from the mode by the engine; only raw is customizable
        content_type:
          req.bodyMode === "raw" ? req.contentType || undefined : undefined,
      };
    }
    case "x-www-form-urlencoded":
      return { mode: "urlencoded", params: kvPairs(req.formParams) };
    case "form-data":
      return {
        mode: "multipart",
        params: (req.formParams ?? [])
          .filter((p) => p.enabled && p.key)
          .map((p) => {
            const file = p.file;
            const path = file?.path;
            return {
              key: p.key,
              value: file ? undefined : p.value,
              file_path: path || undefined,
              // Only send the file content when no real path is available (avoiding base64 over IPC for large files)
              base64: file && !path ? file.data : undefined,
              file_type: file?.type,
              filename: file?.name,
            };
          }),
      };
    case "binary": {
      const bin = req.binaryFile;
      return {
        mode: "binary",
        file_path: bin?.path || undefined,
        base64: bin && !bin.path ? bin.data : undefined,
      };
    }
  }
}

/**
 * `HttpRequest` → un-interpolated request template.
 *
 * Headers are sent grouped as "default → auth → user" (the engine merges them, later entries overriding earlier ones),
 * so the engine can tell whether the user explicitly set Content-Type (if so it does not override it by body mode).
 */
export function buildRequestTemplate(req: HttpRequest): WireRequestTemplate {
  return {
    url: req.url,
    path_params: kvPairs(req.pathParams),
    query_params: kvPairs(req.queryParams),
    default_headers: buildDefaultHeaders(req).map(
      (h) => [h.key, h.value] as [string, string],
    ),
    auth_headers: buildAuthHeaders(req.auth).map(
      (h) => [h.key, h.value] as [string, string],
    ),
    user_headers: kvPairs(req.headers),
    // Cookies were already assembled into a Cookie header by buildDefaultHeaders (and deduped against user headers), so they are not sent again
    cookies: [],
    body: buildBodyTemplate(req),
  };
}
