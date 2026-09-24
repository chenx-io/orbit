// AI assistant frontend types: one-to-one with the Rust side (crates/orbit-ai + web/src-tauri/src/commands/ai*.rs)
// camelCase JSON. Changing a field on either side requires updating both.
//
// Note: the model-list constants at the end of this file are derived from `lib/ai/providers.ts` (type-only dependency,
// so there is no runtime cycle: providers.ts only `import type`s AiProviderKind from this file).
import { PROVIDER_PRESETS } from "@/lib/ai/providers";

/** Protocol shape: determines the request body and the streaming parse strategy */
export type AiProviderKind = "openai" | "anthropic";

/**
 * Auth header style (values match the Rust `orbit_ai::auth`).
 *
 * - `apiKey`: vendor-standard header — Anthropic's official `x-api-key`, Azure-style `api-key`;
 * - `bearer`: `Authorization: Bearer <key>` (Anthropic's OAuth auth token, plus relays that accept only Bearer).
 */
export type AiAuthStyle = "apiKey" | "bearer";

/** Token usage */
export interface AiUsage {
  inputTokens: number;
  outputTokens: number;
}

/** Tool permission tier (plan = planning tools, visible in Plan mode only) */
export type AiToolKind = "read" | "write" | "execute" | "plan";

/** Tool call status */
export type AiToolStatus =
  "pendingApproval" | "running" | "completed" | "failed" | "denied";

/** Chat message role */
export type AiRole = "system" | "user" | "assistant" | "tool";

/** Tool call initiated by the model */
export interface AiToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

/** A single chat message (aligned with the Rust `orbit_ai::ChatMessage`) */
export interface AiMessage {
  role: AiRole;
  text: string;
  toolCalls?: AiToolCall[];
  toolCallId?: string;
  toolName?: string;
  /** Reasoning chain (DeepSeek / Anthropic) */
  reasoning?: string;
  toolOk?: boolean;
  at: number;
}

/** Work mode (literals match the Rust `orbit_ai::AiMode`) */
export type AiMode = "ask" | "agent" | "plan";

/** AI assistant preferences (persisted into the snapshot, app-level; no secrets) */
export interface AiPrefs {
  providerId?: string | null;
  provider: AiProviderKind;
  model: string;
  baseUrl: string;
  maxTokens: number;
  language: "zh" | "en";
  /** Default mode for new sessions (each session also remembers the mode it was using) */
  mode: AiMode;
  maxRounds: number;
}

/** Custom request headers (vendor/gateway differences, e.g. OpenRouter's HTTP-Referer) */
export interface AiHeaderPair {
  key: string;
  value: string;
}

/** Redacted view of a credential (the plaintext key is never exposed) */
export interface AiCredentialView {
  id: string;
  label: string;
  kind: AiProviderKind;
  baseUrl: string;
  defaultModel?: string | null;
  /** Auth header style (normalized by the backend; legacy credential files fall back to the protocol default) */
  authStyle: AiAuthStyle;
  /** Models the user chose to follow (from "fetch model list") */
  models: string[];
  /** Custom request headers */
  headers: AiHeaderPair[];
  hasKey: boolean;
  keyHint: string;
  updatedAt: number;
}

/**
 * Create / update credential input.
 *
 * Omitting `apiKey` / `models` / `headers` / `authStyle` (`undefined`) keeps the existing values;
 * passing an empty array clears them (omitting `authStyle` on create = use the protocol default).
 */
export interface AiCredentialInput {
  id?: string | null;
  label: string;
  kind: AiProviderKind;
  baseUrl: string;
  apiKey?: string | null;
  defaultModel?: string | null;
  authStyle?: AiAuthStyle | null;
  models?: string[] | null;
  headers?: AiHeaderPair[] | null;
}

/** Session summary (for lists) */
export interface AiSessionSummary {
  id: string;
  title: string;
  workspaceId?: string | null;
  messageCount: number;
  updatedAt: number;
}

/** A single step of a plan */
export interface AiPlanStep {
  title: string;
  detail?: string | null;
}

/** Plan produced in Plan mode (persisted with the session; revision increments on each amendment) */
export interface AiPlanArtifact {
  title: string;
  summary: string;
  steps: AiPlanStep[];
  notes: string[];
  revision: number;
  createdAt: number;
  updatedAt: number;
}

/** An AI session */
export interface AiSession {
  id: string;
  title: string;
  workspaceId?: string | null;
  providerId?: string | null;
  model?: string | null;
  /** The session's current work mode */
  mode: AiMode;
  /** The most recently produced plan (Plan mode) */
  plan?: AiPlanArtifact | null;
  messages: AiMessage[];
  createdAt: number;
  updatedAt: number;
}

/** Field diff kind */
export type AiDiffKind = "added" | "removed" | "changed";

/** A single field diff */
export interface AiFieldDiff {
  path: string;
  before?: unknown;
  after?: unknown;
  kind: AiDiffKind;
}

/** Proposal action (the host persists based on this) */
export type AiProposalAction =
  | { type: "createRequest"; collectionId: string; parentId?: string | null }
  | { type: "updateRequest"; requestId: string }
  | { type: "createCollection" }
  | { type: "createScenarioFolder" }
  | { type: "createScenario"; folderId?: string | null }
  | { type: "updateScenario"; scenarioId: string }
  | { type: "createSuite" }
  | { type: "createDataSet" }
  /** Create / update a script library item (omitting `templateId` = create) */
  | { type: "saveActionTemplate"; templateId?: string | null }
  /** Delete a script library item (requests referencing it become dangling references) */
  | { type: "deleteActionTemplate"; templateId: string };

/** A persisted change proposal */
export interface AiProposal {
  id: string;
  tool: string;
  title: string;
  target: string;
  action: AiProposalAction;
  before?: unknown;
  after: unknown;
  diffs: AiFieldDiff[];
  createdAt: number;
}

/** The entity selected in the context */
export interface AiSelection {
  kind: string;
  id: string;
  name: string;
  detail?: string;
}

/** Kind of object the user explicitly referenced */
export type AiReferenceKind =
  "workspace" | "collection" | "request" | "scenario" | "file";

/**
 * An object the user explicitly referenced (reference chips above the input box).
 *
 * Unlike the "current selection", this is a **boundary condition** the user actively pinned — "only add requests to this collection";
 * it is rendered as a constraint in the prompt (`Reference`), not merely as background information.
 */
export interface AiReference {
  kind: AiReferenceKind;
  /** Entity id; for `file` this is the file's absolute path */
  id: string;
  /** Display name */
  name: string;
  /** Supplementary description (e.g. `POST /api/login`, `2.1 KB`) */
  detail?: string;
  /** file only: the text read (possibly truncated), inlined into the prompt with the request */
  content?: string | null;
  /** file only: whether it was truncated for being too long (surfaced to the user in the UI) */
  truncated?: boolean;
}

/** Result of reading an external definition file */
export interface AiReferenceFile {
  name: string;
  path: string;
  bytes: number;
  text: string;
  truncated: boolean;
}

/** Context summary injected into the prompt (names only, no secret values) */
export interface AiContextSummary {
  workspaceId?: string | null;
  workspaceName?: string | null;
  environmentId?: string | null;
  environmentName?: string | null;
  variableNames?: string[];
  secretNames?: string[];
  collections?: string[];
  requestCount?: number;
  scenarioCount?: number;
  suiteCount?: number;
  modelNames?: string[];
  dataSourceNames?: string[];
  selection?: AiSelection | null;
  /** Objects the user explicitly referenced (boundary conditions) */
  references?: AiReference[];
}

/** Agent event (obtained by polling `ai_drain_events`) */
export type AiEvent =
  | { type: "turnStarted"; turnId: string }
  | { type: "textDelta"; turnId: string; delta: string }
  | { type: "reasoningDelta"; turnId: string; delta: string }
  | {
      type: "toolCall";
      turnId: string;
      callId: string;
      name: string;
      kind: AiToolKind;
      arguments: Record<string, unknown>;
      status: AiToolStatus;
      note?: string | null;
    }
  | {
      type: "toolResult";
      turnId: string;
      callId: string;
      ok: boolean;
      summary: string;
      payload: unknown;
      elapsedMs: number;
    }
  | {
      type: "proposalReady";
      turnId: string;
      callId: string;
      proposal: AiProposal;
    }
  | { type: "planReady"; turnId: string; callId: string; plan: AiPlanArtifact }
  | {
      type: "turnFinished";
      turnId: string;
      stopReason: "endTurn" | "toolUse" | "maxTokens" | "other";
      usage: AiUsage;
      truncated: boolean;
    }
  | { type: "dataChanged"; scope: string }
  | { type: "progress"; message: string }
  | {
      type: "error";
      turnId?: string | null;
      message: string;
      retryable: boolean;
    };

/** Input for starting a conversation turn */
export interface AiStartTurnRequest {
  sessionId: string;
  text: string;
  context: AiContextSummary;
  mode: AiMode;
  model?: string | null;
  providerId?: string | null;
}

/** Start result */
export interface AiStartTurnResponse {
  turnId: string;
  sessionId: string;
}

/** Test connection result */
export interface AiTestConnectionResult {
  ok: boolean;
  status: number;
  latencyMs: number;
  model: string;
  message: string;
}

/** A tool call card in the drawer (accumulated from the event stream) */
export interface AiToolCard {
  callId: string;
  name: string;
  kind: AiToolKind;
  status: AiToolStatus;
  arguments: Record<string, unknown>;
  note?: string | null;
  summary?: string;
  payload?: unknown;
  elapsedMs?: number;
}

/** Default preferences (aligned with the Rust `AiPrefs::default()`) */
export const DEFAULT_AI_PREFS: AiPrefs = {
  providerId: null,
  provider: "openai",
  model: "",
  baseUrl: "",
  maxTokens: 4096,
  language: "zh",
  mode: "agent",
  maxRounds: 8,
};

/**
 * Built-in model presets (a fallback list aggregated per protocol shape).
 *
 * **Derived from the provider presets in `lib/ai/providers.ts`** rather than hand-copied: this file used to carry
 * its own `gpt-4o` / `claude-3.5` list that drifted away from `PROVIDER_PRESETS`, leaving the user's dropdown
 * full of long-retired models. To add or update models, change only `providers.ts`.
 */
export const AI_MODEL_PRESETS: Record<AiProviderKind, string[]> = {
  openai: modelsOfKind("openai"),
  anthropic: modelsOfKind("anthropic"),
};

/** Default Base URL per provider (taken from the first provider preset with an address in this protocol shape). */
export const AI_DEFAULT_BASE_URL: Record<AiProviderKind, string> = {
  openai: baseUrlOfKind("openai"),
  anthropic: baseUrlOfKind("anthropic"),
};

/** Default model per provider (also taken from provider presets, to avoid drifting from `providers.ts`). */
export const AI_DEFAULT_MODEL: Record<AiProviderKind, string> = {
  openai: modelOfKind("openai"),
  anthropic: modelOfKind("anthropic"),
};

/** Models of every provider in this protocol shape (deduped in preset order, empty entries skipped). */
function modelsOfKind(kind: AiProviderKind): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const preset of PROVIDER_PRESETS) {
    if (preset.kind !== kind) continue;
    for (const model of preset.models) {
      if (!model || seen.has(model)) continue;
      seen.add(model);
      out.push(model);
    }
  }
  return out;
}

/** Base URL of the first provider preset with an address in this protocol shape. */
function baseUrlOfKind(kind: AiProviderKind): string {
  return (
    PROVIDER_PRESETS.find((p) => p.kind === kind && p.baseUrl !== "")
      ?.baseUrl ?? ""
  );
}

/** Default model of the first provider preset with a default model in this protocol shape. */
function modelOfKind(kind: AiProviderKind): string {
  return (
    PROVIDER_PRESETS.find((p) => p.kind === kind && p.defaultModel !== "")
      ?.defaultModel ?? ""
  );
}
