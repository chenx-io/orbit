// AI assistant panel (docked on the **right**; any module can open it and the current workspace/environment/references come along).
//
// Shape choice: docked rather than an overlay — it is a flex item in the `AppShell` main row (sharing a level with the sidebar,
// the request tree and the request/response panels) and **compresses the content on the left** instead of covering it:
// the request referenced in AI and the response being compared stay in view, so the drawer needs no toggling back and forth.
// The width persists to localStorage and is adjusted by dragging the left edge.
import { useCallback, useEffect, useRef, useState } from "react";
import { Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import {
  AI_DRAWER_WIDTH,
  AI_DRAWER_WIDTH_KEY,
  AI_EXAMPLE_KEYS,
} from "@/lib/ai/constants";
import { AiDrawerHeader } from "./AiDrawerHeader";
import { MessageList } from "./MessageList";
import { Composer } from "./Composer";
import { PlanCard } from "./PlanCard";
import { AiSettingsDialog } from "./AiSettingsDialog";

/** Empty state: starter examples (clicking one sends it). */
function EmptyState({ onPick }: { onPick: (text: string) => void }) {
  const { t } = useT();
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
      <Sparkles className="h-6 w-6 text-violet-400" />
      <div>
        <p className="text-sm font-semibold">{t("ai.empty.title")}</p>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("ai.empty.hint")}
        </p>
      </div>
      <div className="flex w-full flex-col gap-1.5">
        {AI_EXAMPLE_KEYS.map((key) => (
          <button
            key={key}
            type="button"
            onClick={() => onPick(t(key))}
            className="cursor-pointer rounded-md border border-border bg-card px-2.5 py-1.5 text-left text-xs transition-colors hover:bg-accent"
          >
            {t(key)}
          </button>
        ))}
      </div>
    </div>
  );
}

export function AiDrawer() {
  const { t } = useT();
  const open = useAppStore((s) => s.aiDrawerOpen);
  const session = useAppStore((s) => s.aiSession);
  const sessions = useAppStore((s) => s.aiSessions);
  const streaming = useAppStore((s) => s.aiStreaming);
  const submitting = useAppStore((s) => s.aiSubmitting);
  const streamText = useAppStore((s) => s.aiStreamText);
  const streamReasoning = useAppStore((s) => s.aiStreamReasoning);
  const toolCards = useAppStore((s) => s.aiToolCards);
  const steps = useAppStore((s) => s.aiSteps);
  const applied = useAppStore((s) => s.aiAppliedProposals);
  const plan = useAppStore((s) => s.aiPlan);
  const credentials = useAppStore((s) => s.aiCredentials);
  const mode = useAppStore((s) => s.aiSession?.mode ?? s.aiPrefs.mode);
  const setMode = useAppStore((s) => s.aiSetMode);
  const implementPlan = useAppStore((s) => s.aiImplementPlan);
  const error = useAppStore((s) => s.aiError);
  const workspaceName = useAppStore(
    (s) => s.workspaces.find((w) => w.id === s.activeWorkspaceId)?.name ?? null,
  );
  const environmentName = useAppStore(
    (s) => s.environments.find((e) => e.id === s.activeEnvId)?.name ?? null,
  );

  const send = useAppStore((s) => s.aiSend);
  const stop = useAppStore((s) => s.aiStop);
  const approve = useAppStore((s) => s.aiApprove);
  const loadConfig = useAppStore((s) => s.aiLoadConfig);
  const restoreRecentSession = useAppStore((s) => s.aiRestoreRecentSession);
  const openSession = useAppStore((s) => s.aiOpenSession);
  const newSession = useAppStore((s) => s.aiNewSession);
  const renameSession = useAppStore((s) => s.aiRenameSession);
  const deleteSession = useAppStore((s) => s.aiDeleteSession);
  const attachEvents = useAppStore((s) => s.aiAttachEvents);

  const [text, setText] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [width, setWidth] = useState<number>(() => {
    try {
      const saved = Number(localStorage.getItem(AI_DRAWER_WIDTH_KEY));
      return Number.isFinite(saved) && saved >= AI_DRAWER_WIDTH.min
        ? Math.min(saved, AI_DRAWER_WIDTH.max)
        : AI_DRAWER_WIDTH.default;
    } catch {
      return AI_DRAWER_WIDTH.default;
    }
  });

  // Subscribe to the event stream: the drawer is always mounted (subscribed even while hidden) so switching modules loses no events
  useEffect(() => {
    const unsubscribe = attachEvents();
    return () => unsubscribe();
  }, [attachEvents]);

  useEffect(() => {
    if (!open) return;
    void loadConfig();
    // Refresh the list and enter the most recent session on first open (it checks internally and does not switch when a session is already active)
    void restoreRecentSession();
  }, [open, loadConfig, restoreRecentSession]);

  useEffect(() => {
    try {
      localStorage.setItem(AI_DRAWER_WIDTH_KEY, String(width));
    } catch {
      /* ignore */
    }
  }, [width]);

  // Resize handle: pointer capture keeps tracking after the pointer leaves the handle
  const resizing = useRef<{ startX: number; startWidth: number } | null>(null);
  const onResizeDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    resizing.current = { startX: e.clientX, startWidth: width };
    e.currentTarget.setPointerCapture(e.pointerId);
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
  };
  const onResizeMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const r = resizing.current;
    if (!r) return;
    // The drawer is right-aligned: dragging left widens it
    const next = r.startWidth + (r.startX - e.clientX);
    setWidth(
      Math.min(Math.max(next, AI_DRAWER_WIDTH.min), AI_DRAWER_WIDTH.max),
    );
  };
  const onResizeUp = (e: React.PointerEvent<HTMLDivElement>) => {
    resizing.current = null;
    document.body.style.userSelect = "";
    document.body.style.cursor = "";
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  const handleSend = useCallback(
    (value?: string) => {
      const payload = (value ?? text).trim();
      if (!payload) return;
      setText("");
      void send(payload);
    },
    [send, text],
  );

  if (!open) return null;

  const noCredential = credentials.length === 0;
  const showEmpty = !session || session.messages.length === 0;

  return (
    <>
      {/* Docked panel: shrink-0 keeps its own width while the left siblings are compressed (a max-w safety net prevents squeezing the main area away).
          z-40 only overrides fixed overlays such as the load-test floating panel and does not affect its ordinary layout relationship with the main content */}
      <div
        style={{ width }}
        className="relative z-40 flex h-full max-w-[70vw] min-w-0 shrink-0"
      >
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={t("ai.resize")}
          onPointerDown={onResizeDown}
          onPointerMove={onResizeMove}
          onPointerUp={onResizeUp}
          className="w-1 shrink-0 cursor-col-resize bg-border transition-colors hover:bg-primary/70"
        />
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden border-l border-border bg-background">
          <AiDrawerHeader
            title={session?.title ?? t("ai.session.new")}
            sessions={sessions}
            activeSessionId={session?.id ?? null}
            workspaceName={workspaceName}
            environmentName={environmentName}
            onOpenSession={(id) => void openSession(id)}
            onNewSession={newSession}
            onRenameSession={(title) => void renameSession(title)}
            onDeleteSession={(id) => void deleteSession(id)}
            onOpenSettings={() => setSettingsOpen(true)}
          />

          {error && (
            <div className="shrink-0 border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
              {error}
            </div>
          )}

          {showEmpty && !streaming ? (
            <EmptyState onPick={(value) => handleSend(value)} />
          ) : (
            <MessageList
              session={session}
              streaming={streaming}
              streamText={streamText}
              streamReasoning={streamReasoning}
              toolCards={toolCards}
              steps={steps}
              applied={applied}
              onDecide={(callId, allow) => void approve(callId, allow)}
            />
          )}

          {/* The plan is pinned above the input: always visible in Plan mode and collapsed to one row after switching to Agent */}
          {plan && (
            <PlanCard
              plan={plan}
              mode={mode}
              busy={streaming || submitting}
              onImplement={() => void implementPlan()}
            />
          )}

          <Composer
            value={text}
            onChange={setText}
            onSend={() => handleSend()}
            onStop={() => void stop()}
            streaming={streaming || submitting}
            disabled={noCredential}
            mode={mode}
            onModeChange={(next) => void setMode(next)}
          />
        </div>
      </div>

      <AiSettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </>
  );
}

/** Drawer trigger button (reused by the menu bar and elsewhere). */
export function AiDrawerButton({ className }: { className?: string }) {
  const { t } = useT();
  const toggle = useAppStore((s) => s.aiToggleDrawer);
  const open = useAppStore((s) => s.aiDrawerOpen);
  return (
    <Button
      size="icon-sm"
      variant="ghost"
      className={className}
      title={t("ai.toggle")}
      onClick={toggle}
      aria-pressed={open}
    >
      <Sparkles className="h-3.5 w-3.5" />
    </Button>
  );
}
