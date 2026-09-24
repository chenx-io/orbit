// AI assistant bridge: desktop (Tauri) only (BYOK keys are stored locally under `<app_data_dir>/ai/`;
// the browser / orbit-server form does not offer AI capabilities yet).
//
// Events travel via **polling drain** rather than Tauri event pushes: a background-thread emit contends with the window
// message loop for the webview lock (tauri#9453, already hit twice in this project), so the Rust side writes agent events
// into a ring buffer and this side pulls every 120ms (the same pattern as the load-test `load_progress`).
import type {
  AiCredentialInput,
  AiCredentialView,
  AiEvent,
  AiReferenceFile,
  AiPrefs,
  AiSession,
  AiSessionSummary,
  AiStartTurnRequest,
  AiStartTurnResponse,
  AiTestConnectionResult,
} from "@/data/aiTypes";
import { t } from "@/lib/localeDict";
import { isTauri, tauriInvoke } from "./client";

/**
 * Shared "desktop only" error message used by every entry point.
 *
 * A function rather than a constant so the message follows the active UI locale instead of
 * being frozen at module load time.
 */
export function aiDesktopOnlyError(): string {
  return t("ai.error.desktopOnly");
}

function ensureDesktop(): void {
  if (!isTauri()) throw new Error(aiDesktopOnlyError());
}

/** Read AI preferences (persisted into the snapshot, app-level). */
export async function aiConfigGet(): Promise<AiPrefs> {
  ensureDesktop();
  return tauriInvoke<AiPrefs>("ai_config_get");
}

/** Overwrite AI preferences. */
export async function aiConfigSave(prefs: AiPrefs): Promise<void> {
  ensureDesktop();
  await tauriInvoke("ai_config_save", { prefs });
}

/** Credential list (masked). */
export async function aiCredentialList(): Promise<AiCredentialView[]> {
  ensureDesktop();
  const rows = await tauriInvoke<AiCredentialView[]>("ai_credential_list");
  return rows ?? [];
}

/** Create / update a credential and return its id (omitting `apiKey` keeps the existing value). */
export async function aiCredentialSave(
  input: AiCredentialInput,
): Promise<string> {
  ensureDesktop();
  return tauriInvoke<string>("ai_credential_save", { input });
}

/** Delete a credential. */
export async function aiCredentialRemove(id: string): Promise<boolean> {
  ensureDesktop();
  return tauriInvoke<boolean>("ai_credential_remove", { id });
}

/**
 * Fetch the model list available for this credential (`GET {base}/models`).
 *
 * A few self-hosted gateways lack this endpoint → a readable error is thrown and the user can still type the model name.
 */
export async function aiListModels(providerId: string): Promise<string[]> {
  ensureDesktop();
  const rows = await tauriInvoke<string[]>("ai_list_models", { providerId });
  return rows ?? [];
}

/** Test connectivity (send a minimal request). */
export async function aiTestConnection(
  providerId: string,
  model?: string | null,
): Promise<AiTestConnectionResult> {
  ensureDesktop();
  return tauriInvoke<AiTestConnectionResult>("ai_test_connection", {
    request: { providerId, model: model ?? null },
  });
}

/** Session list (filterable by workspace). */
export async function aiSessionList(
  workspaceId?: string | null,
): Promise<AiSessionSummary[]> {
  ensureDesktop();
  const rows = await tauriInvoke<AiSessionSummary[]>("ai_session_list", {
    workspaceId: workspaceId ?? null,
  });
  return rows ?? [];
}

/** Load a session. */
export async function aiSessionLoad(id: string): Promise<AiSession> {
  ensureDesktop();
  return tauriInvoke<AiSession>("ai_session_load", { id });
}

/** Save a session (rename / toggle auto-apply). */
export async function aiSessionSave(session: AiSession): Promise<void> {
  ensureDesktop();
  await tauriInvoke("ai_session_save", { session });
}

/** Delete a session. */
export async function aiSessionDelete(id: string): Promise<boolean> {
  ensureDesktop();
  return tauriInvoke<boolean>("ai_session_delete", { id });
}

/** Start a turn (non-blocking; returns turnId immediately). */
export async function aiStartTurn(
  request: AiStartTurnRequest,
): Promise<AiStartTurnResponse> {
  ensureDesktop();
  return tauriInvoke<AiStartTurnResponse>("ai_start_turn", { request });
}

/** Abort a turn. */
export async function aiAbortTurn(turnId: string): Promise<boolean> {
  ensureDesktop();
  return tauriInvoke<boolean>("ai_abort_turn", { turnId });
}

/**
 * Decide on a pending tool call.
 *
 * Today only execution-type tools (run_request / run_scenario / run_load_test) reach this point:
 * writes persist directly in Agent mode, and Ask / Plan modes have no write tools at all.
 */
export async function aiApproveTool(
  callId: string,
  allow: boolean,
  note?: string,
): Promise<boolean> {
  ensureDesktop();
  return tauriInvoke<boolean>("ai_approve_tool", {
    callId,
    allow,
    note: note ?? null,
  });
}

/**
 * Read an external API-definition file (for reference chips).
 *
 * Read-only and bounded by an extension allowlist and a 4MB cap; the returned text is inlined into the prompt by the frontend,
 * so the model never gains arbitrary file-read capability.
 */
export async function aiReadDefinitionFile(
  path: string,
): Promise<AiReferenceFile> {
  ensureDesktop();
  return tauriInvoke<AiReferenceFile>("ai_read_definition_file", { path });
}

/** Drain and clear the event buffer. */
export async function aiDrainEvents(): Promise<AiEvent[]> {
  if (!isTauri()) return [];
  const rows = await tauriInvoke<AiEvent[]>("ai_drain_events");
  return rows ?? [];
}

/** Event polling interval (ms): responsive enough without saturating the webview. */
const POLL_INTERVAL_MS = 120;

/**
 * Subscribe to the agent event stream (polling implementation).
 *
 * `isActive` is the "should we pull?" predicate: when idle (drawer closed and no turn in progress) IPC is skipped,
 * avoiding a pointless round trip every 120ms; events stay in the backend ring buffer and are still retrievable afterwards.
 *
 * Returns an unsubscribe function; non-desktop returns a no-op (callers need no branching).
 */
export function connectAiEvents(
  onEvents: (events: AiEvent[]) => void,
  onError?: (error: unknown) => void,
  isActive?: () => boolean,
): () => void {
  if (!isTauri()) return () => undefined;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const tick = async () => {
    if (stopped) return;
    if (isActive && !isActive()) {
      timer = setTimeout(() => void tick(), POLL_INTERVAL_MS);
      return;
    }
    try {
      const events = await aiDrainEvents();
      if (events.length > 0 && !stopped) onEvents(events);
    } catch (e) {
      onError?.(e);
    }
    if (!stopped) {
      timer = setTimeout(() => void tick(), POLL_INTERVAL_MS);
    }
  };
  timer = setTimeout(() => void tick(), POLL_INTERVAL_MS);

  return () => {
    stopped = true;
    if (timer) clearTimeout(timer);
  };
}
