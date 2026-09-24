// Credential display names: a credential in the AI config may be unnamed, and the UI still needs a sensible label,
// otherwise the model dialog's group headings degrade to something like "current OpenAI-compatible" that identifies nothing.
import type { AiProviderKind } from "@/data/aiTypes";

/** These hostname prefixes are generic words to skip when deriving a name (`api.deepseek.com` → `deepseek`). */
const GENERIC_HOST_PREFIXES = new Set([
  "api",
  "www",
  "llm",
  "openapi",
  "chat",
  "ai",
  "gateway",
]);

/** Base URL → host (keeping the port, so several local gateways stay distinguishable). */
export function baseUrlHost(baseUrl: string): string {
  const raw = (baseUrl ?? "").trim();
  if (!raw) return "";
  try {
    const url = new URL(raw.includes("://") ? raw : `https://${raw}`);
    return url.port ? `${url.hostname}:${url.port}` : url.hostname;
  } catch {
    return "";
  }
}

/**
 * Fallback name when no name is set.
 *
 * Prefer inferring the brand from the Base URL (`https://api.deepseek.com/v1` → `deepseek`),
 * falling back to the protocol alias (`OpenAI` / `Anthropic`) when that fails (an empty URL / a bare IP).
 */
export function defaultCredentialName(
  kind: AiProviderKind,
  baseUrl: string,
): string {
  const host = baseUrlHost(baseUrl);
  const hostname = host.split(":")[0];
  const port = host.includes(":") ? host.slice(host.indexOf(":")) : "";
  if (hostname) {
    const parts = hostname.split(".");
    // Bare IP: keep the whole thing (splitting the first segment would yield a meaningless name like "192")
    const isIp = parts.length === 4 && parts.every((p) => /^\d+$/.test(p));
    if (isIp) return host;
    const first = parts[0];
    const brand =
      parts.length > 1 && GENERIC_HOST_PREFIXES.has(first) ? parts[1] : first;
    if (brand) return `${brand}${port}`;
  }
  return kind === "anthropic" ? "Anthropic" : "OpenAI";
}

/**
 * Display name of a credential.
 *
 * Legacy data stores "unnamed" as a bare protocol value (`openai` / `anthropic`), which says nothing as a heading,
 * so it falls back to the Base-URL-derived name here. Once the user renames it in settings the value is frozen and never overwritten.
 */
export function credentialDisplayName(credential: {
  label?: string | null;
  kind: AiProviderKind;
  baseUrl: string;
}): string {
  const label = (credential.label ?? "").trim();
  if (!label) return defaultCredentialName(credential.kind, credential.baseUrl);
  const lower = label.toLowerCase();
  if (
    lower === "openai" ||
    lower === "anthropic" ||
    lower === credential.kind
  ) {
    return defaultCredentialName(credential.kind, credential.baseUrl);
  }
  return label;
}
