import type { AuthConfig, HttpRequest } from "@/data/types";

export interface BuiltHeader {
  key: string;
  value: string;
}

function base64(s: string): string {
  if (typeof btoa === "function") {
    try {
      return btoa(s);
    } catch {
      /* fallthrough to Buffer */
    }
  }
  // Tauri / Node fallback
  return typeof Buffer !== "undefined"
    ? Buffer.from(s, "utf-8").toString("base64")
    : s;
}

/** Build the headers to add for the given auth config (variables are not resolved here; the caller resolves them centrally) */
export function buildAuthHeaders(auth: AuthConfig): BuiltHeader[] {
  if (auth.type === "none") return [];
  if (auth.type === "bearer" || auth.type === "oauth2") {
    const token = auth.token ?? "";
    return token ? [{ key: "Authorization", value: `Bearer ${token}` }] : [];
  }
  if (auth.type === "basic") {
    const up = `${auth.username ?? ""}:${auth.password ?? ""}`;
    return [{ key: "Authorization", value: `Basic ${base64(up)}` }];
  }
  if (auth.type === "apikey" && (auth.addTo ?? "header") === "header") {
    return auth.key ? [{ key: auth.key, value: auth.value ?? "" }] : [];
  }
  return [];
}

/** Default headers added automatically to a request (excluding user-configured headers and auth headers)
 *
 * Note: the UI already syncs these default headers into `req.headers` as `auto: true` entries,
 * so headers of the same name already present on the request are skipped here to avoid duplicates.
 * (Default headers the user disabled / deleted are skipped as well because their key is still in the list, respecting the user's action.) */
export function buildDefaultHeaders(req: HttpRequest): BuiltHeader[] {
  const existing = new Set(
    req.headers.filter((h) => h.key).map((h) => h.key.toLowerCase()),
  );
  const headers: BuiltHeader[] = [];
  const push = (key: string, value: string) => {
    if (existing.has(key.toLowerCase())) return;
    headers.push({ key, value });
  };
  push("Accept", "*/*");
  // Accept-Encoding is no longer added by default: it makes exported curl commands carry --compressed,
  // and some servers (especially older Java apps) do not support gzip/br and return an error/406.
  push("User-Agent", "Orbit/1.0");
  push("Connection", "keep-alive");
  push("Cache-Control", "no-cache");
  // Add Content-Type automatically when the body has a content type
  if (req.bodyMode !== "none" && req.contentType) {
    push("Content-Type", req.contentType);
  }
  // Enabled cookies are assembled into a Cookie header automatically
  const cookie = req.cookies
    .filter((c) => c.enabled && c.name)
    .map((c) => `${c.name}=${c.value}`)
    .join("; ");
  if (cookie) push("Cookie", cookie);
  return headers;
}

/** Parse the host (including port) from a URL, used to fill in the Host header */
export function hostFromUrl(url: string): string {
  try {
    if (url.startsWith("http")) return new URL(url).host;
    const slash = url.indexOf("/");
    const hostPart = slash >= 0 ? url.slice(0, slash) : url;
    return hostPart || "";
  } catch {
    return "";
  }
}

/** Compute the request body length in bytes (UTF-8) */
export function byteLength(s: string): number {
  if (typeof TextEncoder !== "undefined") {
    return new TextEncoder().encode(s).length;
  }
  return typeof Buffer !== "undefined"
    ? Buffer.byteLength(s, "utf-8")
    : s.length;
}
