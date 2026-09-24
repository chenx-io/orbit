// AI assistant domain: drawer state, BYOK config, sessions, work modes, the turn event stream and change replay.
//
// Key rules (matching the contract with the Rust-side host):
// 1. Call `flushSave()` before sending a message to push frontend drafts to the backend — otherwise the AI reads stale data;
// 2. After a Rust-side write tool persists, it emits `dataChanged`; here we call `reloadFromBackend()` to refill
//    the store (otherwise stale UI data would overwrite the AI's changes on the next autosave);
// 3. After a turn ends (`turnFinished`), reload the session and treat the messages and plan persisted on the Rust side as authoritative,
//    clearing the local streaming buffer to avoid duplicate rendering.
//
// Mode (Ask / Agent / Plan) is **decided on the Rust side** (tool visibility + execution admission);
// the frontend only selects and displays it, passing the mode along with every turn, and never pretends to block anything locally.
import type { StateCreator } from "zustand";
import type {
  AiContextSummary,
  AiCredentialInput,
  AiCredentialView,
  AiEvent,
  AiMode,
  AiPlanArtifact,
  AiPrefs,
  AiProposal,
  AiReference,
  AiReferenceKind,
  AiSelection,
  AiSession,
  AiSessionSummary,
  AiTestConnectionResult,
  AiToolCard,
} from "@/data/aiTypes";
import { DEFAULT_AI_PREFS } from "@/data/aiTypes";
import {
  aiApproveTool,
  aiAbortTurn,
  aiConfigGet,
  aiCredentialList,
  aiCredentialRemove,
  aiCredentialSave,
  aiListModels as bridgeListModels,
  aiReadDefinitionFile,
  aiSessionDelete,
  aiSessionList,
  aiSessionLoad,
  aiSessionSave,
  aiStartTurn,
  aiTestConnection,
  connectAiEvents,
  isTauri,
} from "@/lib/bridge";
import { pickTauriFile } from "@/lib/filePicker";
import { t, tFormat } from "@/lib/localeDict";
import {
  definitionFileFilters,
  formatFileSize,
  toReferenceKind,
} from "@/lib/ai/references";
import { getPersistenceService, saveNow } from "@/lib/persistence";
import { getPostActions, getPreActions } from "@/lib/requestActions";
import type { AppState } from "../types";

export interface AiSlice {
  // ── Drawer and references (context boundary) ──
  aiDrawerOpen: boolean;
  /**
   * Objects the user explicitly references: workspace / collection / request / scenario / external definition file.
   *
   * These are not just "background information" but are rendered as **constraints** in the prompt ("operate only within this collection").
   * References are a session-level draft (not part of the snapshot, not written to the session file): switching or creating a session clears them,
   * so the previous turn's file body does not mysteriously follow into the next one.
   */
  aiReferences: AiReference[];
  aiSetDrawerOpen: (open: boolean) => void;
  aiToggleDrawer: () => void;
  /** Open the drawer and pin an entity as a reference (the "AI optimize" entry in the request editor) */
  aiFocusEntity: (selection: AiSelection) => void;
  /** Add a reference (deduped by kind+id) */
  aiAddReference: (reference: AiReference) => void;
  aiRemoveReference: (kind: AiReferenceKind, id: string) => void;
  aiClearReferences: () => void;
  /** Pick an external API-definition file and read its content as a reference */
  aiAttachDefinitionFile: () => Promise<void>;

  // ── BYOK config ──
  aiPrefs: AiPrefs;
  aiCredentials: AiCredentialView[];
  aiConfigLoading: boolean;
  aiLoadConfig: () => Promise<void>;
  aiSavePrefs: (prefs: AiPrefs) => Promise<void>;
  aiSaveCredential: (input: AiCredentialInput) => Promise<string>;
  aiRemoveCredential: (id: string) => Promise<void>;
  aiTestConnection: (
    providerId: string,
    model?: string | null,
  ) => Promise<AiTestConnectionResult>;
  /** Fetch the model list available for this credential (not cached in state; the caller holds it) */
  aiListModels: (providerId: string) => Promise<string[]>;

  // ── Sessions ──
  aiSessions: AiSessionSummary[];
  aiSession: AiSession | null;
  aiSessionLoading: boolean;
  aiLoadSessions: () => Promise<void>;
  aiOpenSession: (id: string) => Promise<void>;
  aiNewSession: () => void;
  aiRenameSession: (title: string) => Promise<void>;
  aiDeleteSession: (id: string) => Promise<void>;
  /**
   * Called when the drawer opens: refreshes the session list and, when there is **no active session**, opens the most recently updated one.
   *
   * "Most recent" is defined by the backend (`ai_session_list` is already sorted by `updatedAt` descending and filtered by the current workspace),
   * so the frontend does not re-sort, avoiding drift between the two definitions.
   */
  aiRestoreRecentSession: () => Promise<void>;
  /**
   * Whether auto-restore already happened within this run (**a runtime flag; never persisted, never part of `aiPrefs`**).
   *
   * It is also set once the user clicks "New chat": otherwise "just created a new chat → close and reopen the drawer"
   * would jump back to the old session, contradicting the user's intent.
   */
  aiAutoRestoreDone: boolean;
  /**
   * Switch the "credential + model" used by this conversation.
   *
   * Written back to both the session (used again next time it opens) and the preferences (the default for new sessions),
   * so one selection in the drawer applies from then on.
   */
  aiSetSessionModel: (providerId: string, model: string) => Promise<void>;

  // ── Turns ──
  aiTurnId: string | null;
  aiStreaming: boolean;
  aiSubmitting: boolean;
  /** Streaming body of the current turn (cleared when it ends, after which session messages are rendered) */
  aiStreamText: string;
  /** Reasoning chain of the current turn */
  aiStreamReasoning: string;
  aiToolCards: AiToolCard[];
  /** Long-task progress (scenario runs etc.), for display only */
  aiSteps: string[];
  /** Changes persisted in this turn/session (the write-operation replay in Agent mode, view only) */
  aiAppliedProposals: AiProposal[];
  /** Most recently produced plan (Plan mode; the one persisted in the session is authoritative) */
  aiPlan: AiPlanArtifact | null;
  aiError: string | null;
  /** Switch the work mode (Ask / Agent / Plan), writing back to both the session and the preferences */
  aiSetMode: (mode: AiMode) => Promise<void>;
  /** "Start implementing": switch to Agent mode and continue from the confirmed plan */
  aiImplementPlan: () => Promise<void>;
  aiSend: (text: string) => Promise<void>;
  aiStop: () => Promise<void>;
  /** Authorization decision for execution-type tools (write operations do not go through here) */
  aiApprove: (callId: string, allow: boolean) => Promise<void>;
  /** Subscribe to the event stream (returns an unsubscribe function) */
  aiAttachEvents: () => () => void;
}

/** Maximum number of step entries kept in the event stream (keeps long scenario runs from flooding the UI). */
const MAX_STEPS = 200;

export const createAiSlice: StateCreator<AppState, [], [], AiSlice> = (
  set,
  get,
) => ({
  aiDrawerOpen: false,
  aiReferences: [],
  aiSetDrawerOpen: (open) => set({ aiDrawerOpen: open }),
  aiToggleDrawer: () => set((s) => ({ aiDrawerOpen: !s.aiDrawerOpen })),
  // The "AI optimize" entry pins the current request as a reference instead of opening a second "selection" state —
  // the user sees the context in exactly one place (the chips above the input box), so two sources can never contradict each other
  aiFocusEntity: (selection) => {
    get().aiAddReference({
      kind: toReferenceKind(selection.kind),
      id: selection.id,
      name: selection.name,
      detail: selection.detail,
    });
    set({ aiDrawerOpen: true });
  },

  aiPrefs: { ...DEFAULT_AI_PREFS },
  aiCredentials: [],
  aiConfigLoading: false,
  aiLoadConfig: async () => {
    if (!isTauri()) return;
    set({ aiConfigLoading: true });
    try {
      const [prefs, credentials] = await Promise.all([
        aiConfigGet(),
        aiCredentialList(),
      ]);
      set({ aiPrefs: prefs, aiCredentials: credentials });
    } catch (e) {
      console.error("[ai] failed to load config", e);
      set({ aiError: toMessage(e) });
    } finally {
      set({ aiConfigLoading: false });
    }
  },
  aiSavePrefs: async (prefs) => {
    // Preferences persist with the snapshot (same path as locale/theme): the store is the only writer,
    // and does not write directly via Rust, avoiding the backend's savedAt moving forward and the frontend's later pushes being rejected by the optimistic lock.
    set({ aiPrefs: prefs });
    saveNow();
  },
  aiSaveCredential: async (input) => {
    const id = await aiCredentialSave(input);
    set({ aiCredentials: await aiCredentialList() });
    return id;
  },
  aiRemoveCredential: async (id) => {
    await aiCredentialRemove(id);
    set({ aiCredentials: await aiCredentialList() });
  },
  aiTestConnection: (providerId, model) => aiTestConnection(providerId, model),
  aiListModels: (providerId) => bridgeListModels(providerId),

  aiSessions: [],
  aiSession: null,
  aiSessionLoading: false,
  aiAutoRestoreDone: false,
  aiLoadSessions: async () => {
    if (!isTauri()) return;
    try {
      const rows = await aiSessionList(get().activeWorkspaceId);
      set({ aiSessions: rows });
    } catch (e) {
      console.error("[ai] failed to load the session list", e);
    }
  },
  aiRestoreRecentSession: async () => {
    if (!isTauri()) return;
    // The flag is **set before awaiting**: React strict mode or quickly toggling the drawer triggers this repeatedly,
    // and setting it first prevents two concurrent restores from overwriting each other (if the second runs after the first completes,
    // it would replace the session the user just opened manually).
    const shouldRestore = !get().aiAutoRestoreDone;
    if (shouldRestore) set({ aiAutoRestoreDone: true });
    let rows: AiSessionSummary[] = [];
    try {
      // The list is refreshed every time (keeping titles and order in the dropdown fresh), but restore happens once
      rows = await aiSessionList(get().activeWorkspaceId);
      set({ aiSessions: rows });
    } catch (e) {
      console.error("[ai] failed to load the session list", e);
      if (shouldRestore) {
        // A failed read does not count as "restored": try again the next time the drawer opens
        set({ aiAutoRestoreDone: false });
      }
      return;
    }
    // An active session already exists (opened/chatted in this run) or there are no historical sessions → keep the current state
    if (!shouldRestore || get().aiSession || rows.length === 0) return;
    await get().aiOpenSession(rows[0].id);
  },
  aiOpenSession: async (id) => {
    set({ aiSessionLoading: true, aiError: null });
    try {
      const session = await aiSessionLoad(id);
      // The plan travels with the session: reopening an old session still shows the previously confirmed plan
      set({
        aiSession: session,
        aiReferences: [],
        aiToolCards: [],
        aiSteps: [],
        aiAppliedProposals: [],
        aiPlan: session.plan ?? null,
        aiStreamText: "",
        aiStreamReasoning: "",
      });
    } catch (e) {
      set({ aiError: toMessage(e) });
    } finally {
      set({ aiSessionLoading: false });
    }
  },
  aiNewSession: () =>
    set({
      aiSession: null,
      aiReferences: [],
      aiStreamText: "",
      aiStreamReasoning: "",
      aiToolCards: [],
      aiSteps: [],
      aiAppliedProposals: [],
      aiPlan: null,
      aiError: null,
      // The user explicitly wants a new session: no auto-jump back to an old one for the rest of this run
      aiAutoRestoreDone: true,
    }),
  aiRenameSession: async (title) => {
    const session = get().aiSession;
    if (!session) return;
    const next = { ...session, title };
    set({ aiSession: next });
    try {
      await aiSessionSave(next);
      await get().aiLoadSessions();
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },
  aiDeleteSession: async (id) => {
    try {
      await aiSessionDelete(id);
      if (get().aiSession?.id === id) get().aiNewSession();
      await get().aiLoadSessions();
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },
  aiSetSessionModel: async (providerId, model) => {
    const prefs = { ...get().aiPrefs, providerId, model };
    set({ aiPrefs: prefs });
    saveNow();
    const session = get().aiSession;
    if (!session) return;
    const next = { ...session, providerId, model };
    set({ aiSession: next });
    try {
      await aiSessionSave(next);
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },

  aiTurnId: null,
  aiStreaming: false,
  aiSubmitting: false,
  aiStreamText: "",
  aiStreamReasoning: "",
  aiToolCards: [],
  aiSteps: [],
  aiAppliedProposals: [],
  aiPlan: null,
  aiError: null,

  aiAddReference: (reference) =>
    set((s) => {
      const exists = s.aiReferences.some(
        (r) => r.kind === reference.kind && r.id === reference.id,
      );
      return exists
        ? { aiReferences: s.aiReferences }
        : { aiReferences: [...s.aiReferences, reference] };
    }),
  aiRemoveReference: (kind, id) =>
    set((s) => ({
      aiReferences: s.aiReferences.filter(
        (r) => !(r.kind === kind && r.id === id),
      ),
    })),
  aiClearReferences: () => set({ aiReferences: [] }),
  aiAttachDefinitionFile: async () => {
    if (!isTauri()) {
      set({ aiError: t("ai.error.fileDesktopOnly") });
      return;
    }
    try {
      const picked = await pickTauriFile(definitionFileFilters());
      if (!picked?.path) return;
      const file = await aiReadDefinitionFile(picked.path);
      get().aiAddReference({
        kind: "file",
        id: file.path,
        name: file.name,
        detail: formatFileSize(file.bytes),
        content: file.text,
        truncated: file.truncated,
      });
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },

  aiSetMode: async (mode) => {
    // The mode in the preferences determines the initial mode of a **new session**; the mode on the session determines **that session**.
    // Write both: changing only the session makes new sessions fall back to the old default, and changing only the preferences resets the current session.
    set({ aiPrefs: { ...get().aiPrefs, mode } });
    saveNow();
    const session = get().aiSession;
    if (!session || session.mode === mode) return;
    const next = { ...session, mode };
    set({ aiSession: next });
    try {
      await aiSessionSave(next);
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },

  aiImplementPlan: async () => {
    const plan = get().aiPlan ?? get().aiSession?.plan ?? null;
    if (!plan) return;
    // The plan body is already in the session messages (the present_plan tool result); here we only need a "start" signal
    await get().aiSetMode("agent");
    await get().aiSend(tFormat("ai.plan.startMessage", plan.title));
  },

  aiSend: async (text) => {
    const trimmed = text.trim();
    if (!trimmed || get().aiSubmitting) return;
    if (!isTauri()) {
      set({ aiError: t("ai.error.desktopOnly") });
      return;
    }
    if (get().aiCredentials.length === 0) {
      set({
        aiError: t("ai.needCredential"),
      });
      return;
    }
    set({
      aiSubmitting: true,
      aiError: null,
      aiStreamText: "",
      aiStreamReasoning: "",
      aiToolCards: [],
      aiSteps: [],
      aiAppliedProposals: [],
    });
    try {
      // Push frontend drafts to the backend first: the AI reads the backend's authoritative snapshot
      await getPersistenceService()?.saveNow();
      const session = get().aiSession;
      const res = await aiStartTurn({
        sessionId: session?.id ?? "",
        text: trimmed,
        context: buildContext(get()),
        // The session's mode wins; new sessions use the default from the preferences
        mode: session?.mode ?? get().aiPrefs.mode,
        model: session?.model ?? null,
        providerId: get().aiPrefs.providerId ?? null,
      });
      // Optimistically show the user message (the session is persisted by Rust and reloaded when the turn ends)
      const current = get().aiSession;
      set({
        aiTurnId: res.turnId,
        aiStreaming: true,
        aiSession: current
          ? {
              ...current,
              messages: [
                ...current.messages,
                { role: "user", text: trimmed, at: Date.now() },
              ],
            }
          : {
              id: res.sessionId,
              title: trimmed.slice(0, 30),
              workspaceId: get().activeWorkspaceId,
              mode: get().aiPrefs.mode,
              plan: null,
              messages: [{ role: "user", text: trimmed, at: Date.now() }],
              createdAt: Date.now(),
              updatedAt: Date.now(),
            },
      });
    } catch (e) {
      set({ aiError: toMessage(e), aiStreaming: false });
    } finally {
      set({ aiSubmitting: false });
    }
  },

  aiStop: async () => {
    const turnId = get().aiTurnId;
    if (!turnId) return;
    try {
      await aiAbortTurn(turnId);
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },

  aiApprove: async (callId, allow) => {
    try {
      await aiApproveTool(callId, allow);
      if (allow) {
        set({
          aiToolCards: get().aiToolCards.map((c) =>
            c.callId === callId ? { ...c, status: "running" } : c,
          ),
        });
      }
    } catch (e) {
      set({ aiError: toMessage(e) });
    }
  },

  aiAttachEvents: () =>
    connectAiEvents(
      (events) => handleEvents(set, get, events),
      (e) => console.error("[ai] failed to poll events", e),
      // No polling when the drawer is closed and no turn is in progress (saving idle IPC)
      () => get().aiDrawerOpen || get().aiStreaming,
    ),
});

// ─── Event handling ───────────────────────────────────────────

function handleEvents(
  set: (partial: Partial<AppState>) => void,
  get: () => AppState,
  events: AiEvent[],
): void {
  let needReloadData = false;
  for (const ev of events) {
    switch (ev.type) {
      case "turnStarted":
        set({ aiStreaming: true });
        break;
      case "textDelta":
        set({ aiStreamText: get().aiStreamText + ev.delta });
        break;
      case "reasoningDelta":
        set({ aiStreamReasoning: get().aiStreamReasoning + ev.delta });
        break;
      case "toolCall":
        set({ aiToolCards: upsertToolCard(get().aiToolCards, ev) });
        break;
      case "toolResult":
        set({ aiToolCards: applyToolResult(get().aiToolCards, ev) });
        break;
      case "proposalReady":
        // Agent-mode writes are already persisted; here we only render the "change replay" (before/after diff)
        set({
          aiAppliedProposals: [...get().aiAppliedProposals, ev.proposal],
        });
        break;
      case "planReady":
        // The plan is persisted with the session; mirror it locally first and defer to session.plan once the turn ends
        set({ aiPlan: ev.plan });
        break;
      case "progress":
        set({
          aiSteps: [...get().aiSteps, ev.message].slice(-MAX_STEPS),
        });
        break;
      case "dataChanged":
        needReloadData = true;
        break;
      case "turnFinished": {
        set({ aiStreaming: false, aiTurnId: null });
        void finishTurn(get, set);
        break;
      }
      case "error":
        set({ aiError: ev.message, aiStreaming: false });
        break;
      default:
        break;
    }
  }
  if (needReloadData) {
    // The AI changed authoritative data on the Rust side: re-fetch the snapshot and drop stale references from the local draft cache
    void getPersistenceService()?.reloadFromBackend();
  }
}

/** Turn ended: the messages persisted by Rust are authoritative; clear the streaming buffers. */
async function finishTurn(
  get: () => AppState,
  set: (partial: Partial<AppState>) => void,
): Promise<void> {
  const sessionId = get().aiSession?.id;
  set({ aiStreamText: "", aiStreamReasoning: "" });
  if (!sessionId) return;
  try {
    const session = await aiSessionLoad(sessionId);
    // The session is authoritative: messages and the plan follow what was persisted (the plan may have just been rewritten in a tool call)
    set({
      aiSession: session,
      aiToolCards: [],
      aiSteps: [],
      aiPlan: session.plan ?? get().aiPlan,
    });
    await get().aiLoadSessions();
  } catch (e) {
    console.error("[ai] failed to reload the session", e);
  }
}

function upsertToolCard(
  cards: AiToolCard[],
  ev: Extract<AiEvent, { type: "toolCall" }>,
): AiToolCard[] {
  const next: AiToolCard = {
    callId: ev.callId,
    name: ev.name,
    kind: ev.kind,
    status: ev.status,
    arguments: ev.arguments,
    note: ev.note ?? null,
  };
  const idx = cards.findIndex((c) => c.callId === ev.callId);
  if (idx === -1) return [...cards, next];
  const copy = [...cards];
  copy[idx] = { ...copy[idx], ...next };
  return copy;
}

function applyToolResult(
  cards: AiToolCard[],
  ev: Extract<AiEvent, { type: "toolResult" }>,
): AiToolCard[] {
  const idx = cards.findIndex((c) => c.callId === ev.callId);
  const patch: Partial<AiToolCard> = {
    status: ev.ok ? "completed" : "failed",
    summary: ev.summary,
    payload: ev.payload,
    elapsedMs: ev.elapsedMs,
  };
  if (idx === -1) {
    return [
      ...cards,
      {
        callId: ev.callId,
        name: "",
        kind: "read",
        arguments: {},
        ...patch,
      } as AiToolCard,
    ];
  }
  const copy = [...cards];
  copy[idx] = { ...copy[idx], ...patch };
  return copy;
}

/** Assemble the context injected into the prompt (names only; secret values are never sent out). */
function buildContext(state: AppState): AiContextSummary {
  const env = state.environments.find((e) => e.id === state.activeEnvId);
  const ws = state.workspaces.find((w) => w.id === state.activeWorkspaceId);
  const selection = (() => {
    const tab = state.tabs.find((t) => t.id === state.activeTabId);
    if (!tab) return null;
    // Prefer drafts (content the user is editing but has not saved yet)
    const req = state.drafts[tab.requestId] ?? state.requests[tab.requestId];
    if (!req) return null;
    const url = "url" in req ? String(req.url) : "";
    const method = "method" in req ? String(req.method) : "";
    // Action list summary: lets the model immediately see "this request has pre-request DB actions / a few scripts"
    // without probing first. Only counts and shapes are given, **never content** — content and indices always follow
    // the latest get_request response (two copies would inevitably drift).
    const pre = getPreActions(req).filter(
      (a) => a.kind !== "interpolate",
    ).length;
    const post = getPostActions(req).length;
    const actions = [
      pre > 0 ? tFormat("ai.context.preActions", pre) : "",
      post > 0 ? tFormat("ai.context.postActions", post) : "",
    ]
      .filter(Boolean)
      .join(" · ");
    return {
      kind: "request",
      id: req.id,
      name: req.name,
      detail: [method ? `${method} ${url}` : url, actions]
        .filter(Boolean)
        .join(" · "),
    } satisfies AiSelection;
  })();
  // If the currently selected request is already explicitly referenced, do not repeat it as a "selection"
  // (writing it twice would show the model the same constraint twice and waste tokens)
  const selectionDuplicated = state.aiReferences.some(
    (r) => selection !== null && r.kind === "request" && r.id === selection.id,
  );
  return {
    workspaceId: state.activeWorkspaceId,
    workspaceName: ws?.name ?? null,
    environmentId: state.activeEnvId,
    environmentName: env?.name ?? null,
    variableNames: Object.keys(state.globalVariables ?? {}).concat(
      Object.keys(env?.variables ?? {}),
    ),
    secretNames: Object.keys(env?.secrets ?? {}),
    collections: state.collections.map((c) => `${c.id} | ${c.name}`),
    requestCount: Object.keys(state.requests).length,
    scenarioCount: state.scenarios.length,
    suiteCount: state.scenarioSuites.length,
    modelNames: state.models.map((m) => m.name),
    dataSourceNames: state.dataSources.map((d) => d.name),
    selection: selectionDuplicated ? null : selection,
    // Explicit references (including external definition file bodies) — the boundary condition of this task
    references: state.aiReferences,
  };
}

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
