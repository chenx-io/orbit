// Mainstream provider presets: picking a row fills in "protocol kind + endpoint + default model" automatically.
//
// Why not just "pick a protocol": the question users actually need to answer is "which vendor am I on", not
// "is it OpenAI-compatible" — the latter is an **implementation detail** (DeepSeek, Qwen, Kimi and Ollama are all
// OpenAI-compatible, yet the endpoint and model names differ, so making users copy them by hand is needlessly cruel).
//
// The protocol kind is still kept: it decides the body shape and auth headers and is the boundary of the adapter layer.
//
// ⚠ **Model list verified on 2026-09-21** (checked against each vendor's official docs; `defaultModel` is each
// vendor's "general/balanced" tier, with the flagship and cheap tiers listed in `models` for switching). Models iterate fast; when updating later:
// 1. Trust each vendor's official docs (a wrong model name leads straight to a 404 / model not found);
// 2. **keep `default_model` in `crates/orbit-ai/src/provider/mod.rs` in sync** —
//    that is the backend's fallback when a credential has no default model;
// 3. the other frontend model lists (`AI_MODEL_PRESETS` / `AI_DEFAULT_MODEL` in `data/aiTypes.ts`)
//    all derive from this file — never copy a second version (that is exactly how it drifted into two stale lists before).
import type { AiAuthStyle, AiProviderKind } from "@/data/aiTypes";

export interface ProviderPreset {
  /** Preset id (for local matching, never persisted). */
  id: string;
  /** Display name (brand names are generally not translated). */
  label: string;
  /** i18n key overriding `label` when localization is needed. */
  labelKey?: string;
  /** Which protocol implementation it uses. */
  kind: AiProviderKind;
  /** Endpoint (empty = the user fills it in). */
  baseUrl: string;
  /** Default model. */
  defaultModel: string;
  /** Preset model list (the initial value of the "watch list", later fetchable / checkable). */
  models: string[];
  /** i18n key for notes such as extra required headers. */
  hintKey?: string;
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: "openai",
    label: "OpenAI",
    kind: "openai",
    baseUrl: "https://api.openai.com/v1",
    // Balanced tier (per the vendor: use terra to balance intelligence and cost)
    defaultModel: "gpt-5.6-terra",
    models: ["gpt-6-astra", "gpt-5.6", "gpt-5.6-terra", "gpt-5.6-luna"],
  },
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    kind: "anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    // The vendor recommends Opus 5 by default; Sonnet 5 is used here as the "best mix of speed and intelligence", with the flagship switchable
    defaultModel: "claude-sonnet-5",
    models: [
      "claude-opus-5",
      "claude-sonnet-5",
      "claude-fable-5-1",
      "claude-haiku-4-5-20251001",
    ],
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    kind: "openai",
    baseUrl: "https://api.deepseek.com/v1",
    defaultModel: "deepseek-v4-pro",
    models: ["deepseek-v4-pro", "deepseek-flash"],
  },
  {
    id: "qwen",
    label: "Qwen",
    labelKey: "ai.settings.preset.qwen",
    kind: "openai",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    defaultModel: "qwen3.7-plus",
    models: ["qwen3.8-max", "qwen3.7-plus", "qwen3.8-flash"],
  },
  {
    id: "kimi",
    label: "Kimi (Moonshot)",
    kind: "openai",
    baseUrl: "https://api.moonshot.cn/v1",
    defaultModel: "kimi-k3",
    models: ["kimi-k3", "kimi-k2.6", "kimi-k2.7-code"],
  },
  {
    id: "zhipu",
    label: "Zhipu GLM",
    labelKey: "ai.settings.preset.zhipu",
    kind: "openai",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    defaultModel: "glm-5.2",
    models: ["glm-5.3", "glm-5.2", "glm-5", "glm-4.7", "glm-4.7-flash"],
  },
  {
    id: "minimax",
    label: "MiniMax",
    kind: "openai",
    baseUrl: "https://api.minimax.cn/v1",
    defaultModel: "MiniMax-M3",
    models: [
      "MiniMax-M3",
      "MiniMax-M2.7",
      "MiniMax-M2.7-highspeed",
      "MiniMax-M2.5-highspeed",
    ],
  },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    labelKey: "ai.settings.preset.siliconflow",
    kind: "openai",
    baseUrl: "https://api.siliconflow.cn/v1",
    defaultModel: "deepseek-ai/DeepSeek-V4-Pro",
    models: [
      "deepseek-ai/DeepSeek-V4-Pro",
      "deepseek-ai/DeepSeek-V4-Flash",
      "zai-org/GLM-5.3",
      "Qwen/Qwen3.8-27B",
      "moonshotai/Kimi-K2.7-Code",
    ],
  },
  {
    id: "gemini",
    label: "Google Gemini",
    kind: "openai",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    // 3.8 Flash is the strongest 3.x stable model (pushed by the official banner); 2.0 is retired and can no longer be the default
    defaultModel: "gemini-3.8-flash",
    models: [
      "gemini-3.8-flash",
      "gemini-3.7-flash",
      "gemini-3.5-flash-lite",
      "gemini-3.1-pro-preview",
    ],
    hintKey: "ai.settings.preset.geminiHint",
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    kind: "openai",
    baseUrl: "https://openrouter.ai/api/v1",
    defaultModel: "anthropic/claude-sonnet-5",
    models: [
      "anthropic/claude-opus-5",
      "anthropic/claude-sonnet-5",
      "openai/gpt-6-astra",
      "deepseek/deepseek-v4-pro-0813",
      "deepseek/deepseek-v4.1-flash",
    ],
    hintKey: "ai.settings.preset.openrouterHint",
  },
  {
    id: "ollama",
    label: "Ollama (local)",
    labelKey: "ai.settings.preset.ollama",
    kind: "openai",
    baseUrl: "http://localhost:11434/v1",
    // The local default is a general tier that is recently updated and supports tool calling; the old llama3.1 stays in the list as an alternative
    defaultModel: "qwen3.5",
    models: ["qwen3.5", "gemma4", "deepseek-v4.1-flash", "gpt-oss", "llama3.1"],
    hintKey: "ai.settings.preset.ollamaHint",
  },
  {
    id: "openai-compatible",
    label: "OpenAI Compatible",
    labelKey: "ai.settings.preset.openaiCompatible",
    kind: "openai",
    baseUrl: "",
    defaultModel: "",
    models: [],
  },
  {
    id: "anthropic-compatible",
    label: "Anthropic Compatible",
    labelKey: "ai.settings.preset.anthropicCompatible",
    kind: "anthropic",
    baseUrl: "",
    defaultModel: "",
    models: [],
  },
];

/** Normalize a URL for comparison (trim, drop trailing slashes, lowercase). */
function normalizeUrl(url: string): string {
  return url.trim().replace(/\/+$/, "").toLowerCase();
}

/**
 * Look up a preset by "protocol kind + endpoint".
 *
 * Presets with an empty endpoint (hand-filled ones such as "OpenAI Compatible") do not participate, otherwise any
 * credential without an endpoint would be identified as it. No match means a private gateway → the UI shows "custom".
 */
export function matchPreset(
  kind: AiProviderKind,
  baseUrl: string,
): ProviderPreset | null {
  const target = normalizeUrl(baseUrl);
  if (!target) return null;
  return (
    PROVIDER_PRESETS.find(
      (p) =>
        p.baseUrl !== "" &&
        p.kind === kind &&
        normalizeUrl(p.baseUrl) === target,
    ) ?? null
  );
}

/** Preset display name (brand names are emitted directly; generic entries go through i18n). */
export function presetLabel(
  preset: ProviderPreset,
  t: (key: string) => string,
): string {
  return preset.labelKey ? t(preset.labelKey) : preset.label;
}

// ── Auth styles (aligned with Rust `orbit_ai::auth`)──────────────────

/**
 * Default auth style for a protocol kind.
 *
 * The official Anthropic API key uses `x-api-key` (only OAuth auth tokens use Bearer);
 * the OpenAI-compatible ecosystem always uses `Authorization: Bearer`.
 */
export function defaultAuthStyle(kind: AiProviderKind): AiAuthStyle {
  return kind === "anthropic" ? "apiKey" : "bearer";
}

/** Normalize an auth style (illegal values fall back to the protocol default), aligned with Rust `normalize_auth_style`. */
export function normalizeAuthStyle(
  kind: AiProviderKind,
  style?: string | null,
): AiAuthStyle {
  const s = (style ?? "").trim().toLowerCase();
  if (s === "apikey") return "apiKey";
  if (s === "bearer") return "bearer";
  return defaultAuthStyle(kind);
}

/** The auth header name actually sent when not using Bearer (shown in the UI so users need not guess). */
export function standardAuthHeaderName(kind: AiProviderKind): string {
  return kind === "anthropic" ? "x-api-key" : "api-key";
}
