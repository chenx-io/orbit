// Tauri native file picking: obtains the real absolute path without pre-reading the file.
// This is the core of the "send by path" approach — large files no longer round-trip through base64; at send time
// Rust reads them straight from disk, which is more efficient in both memory and transfer.
import { isTauri } from "@/lib/bridge";

export interface PickedFile {
  /** File name (without the directory) */
  name: string;
  /** MIME type */
  type: string;
  /** Tauri desktop: the file's real absolute path; always empty in the browser */
  path?: string;
  /** Browser: base64 (without the `data:` prefix); always empty on Tauri */
  data?: string;
}

/** Extract the file name from a path (handles both / and \\ separators) */
function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

/** Common extension → MIME; falls back to octet-stream when unknown */
const MIME_BY_EXT: Record<string, string> = {
  txt: "text/plain",
  html: "text/html",
  htm: "text/html",
  css: "text/css",
  js: "application/javascript",
  mjs: "application/javascript",
  json: "application/json",
  xml: "application/xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  avif: "image/avif",
  ico: "image/x-icon",
  pdf: "application/pdf",
  mp4: "video/mp4",
  webm: "video/webm",
  mp3: "audio/mpeg",
  wav: "audio/wav",
  zip: "application/zip",
  gz: "application/gzip",
  tar: "application/x-tar",
  csv: "text/csv",
  tsv: "text/tab-separated-values",
  doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
};

export function mimeFromName(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return MIME_BY_EXT[ext] ?? "application/octet-stream";
}

/** Dialog file-type filters (isomorphic with `@tauri-apps/plugin-dialog` filters). */
export interface FileFilter {
  name: string;
  extensions: string[];
}

/**
 * Tauri desktop: pick a single file with the native dialog and return its real path (no content pre-read).
 * The browser has no native path capability and returns null, leaving the caller to read base64 via `<input type=file>`.
 *
 * `filters` is optional: when omitted it means "all files" (e.g. picking a binary request body).
 */
export async function pickTauriFile(
  filters?: FileFilter[],
): Promise<PickedFile | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: false,
    directory: false,
    ...(filters && filters.length > 0 ? { filters } : {}),
  });
  if (!selected || Array.isArray(selected)) return null;
  return {
    name: basename(selected),
    type: mimeFromName(selected),
    path: selected,
  };
}
