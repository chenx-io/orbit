// AI message stream: history (user bubbles / assistant text / tool results) plus the current turn's streaming text,
// tool cards, change cards and long-task progress.
//
// Assistant prose is Markdown-rendered; user input and tool results stay plain text (the former is the user's own words and
// the latter is high-fidelity content such as JSON, where Markdown would just swallow symbols like `*` and `_`).
import { useEffect, useRef, useState } from "react";
import { ArrowDown, Loader2, Wrench } from "lucide-react";
import type {
  AiMessage,
  AiProposal,
  AiSession,
  AiToolCard as AiToolCardData,
} from "@/data/aiTypes";
import { useT } from "@/lib/i18n";
import { toolLabelKey, toolSubject } from "@/lib/ai/proposal";
import { ToolCallCard } from "./ToolCallCard";
import { ProposalCard } from "./ProposalCard";
import { Markdown } from "./Markdown";

/** How many pixels from the bottom still counts as "at the bottom" (below it the view auto-follows). */
const NEAR_BOTTOM_PX = 48;

function UserBubble({ text }: { text: string }) {
  return (
    <div className="flex justify-end">
      <div className="max-w-[85%] whitespace-pre-wrap break-words rounded-md bg-primary px-2.5 py-1.5 text-xs text-primary-foreground">
        {text}
      </div>
    </div>
  );
}

function AssistantText({ text }: { text: string }) {
  // Assistant replies are rendered as Markdown (the model almost always emits code blocks/lists/tables)
  return <Markdown text={text} />;
}

/** Tool results in history: collapsed into one row (the same shape as "thinking"), expanding shows the result body. */
function HistoryToolRow({ message }: { message: AiMessage }) {
  const { t } = useT();
  const name = message.toolName ?? "tool";
  const ok = message.toolOk !== false;
  return (
    <details className="rounded-md bg-muted/30 px-2 py-1">
      <summary className="cursor-pointer text-xs text-muted-foreground">
        <Wrench className="mr-1 inline h-3 w-3 align-[-1px]" />
        {t(toolLabelKey(name), name)}
        {" · "}
        <span className={ok ? "text-success" : "text-destructive"}>
          {t(ok ? "ai.status.completed" : "ai.status.failed")}
        </span>
      </summary>
      {message.text && (
        <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-muted-foreground">
          {message.text}
        </pre>
      )}
    </details>
  );
}

export interface MessageListProps {
  session: AiSession | null;
  streaming: boolean;
  streamText: string;
  streamReasoning: string;
  toolCards: AiToolCardData[];
  steps: string[];
  /** Changes persisted in this turn (the Agent mode write replay) */
  applied: AiProposal[];
  /** Authorization decisions on tool cards (only execution-type tools use them) */
  onDecide: (callId: string, allow: boolean) => void;
}

export function MessageList({
  session,
  streaming,
  streamText,
  streamReasoning,
  toolCards,
  steps,
  applied,
  onDecide,
}: MessageListProps) {
  const { t } = useT();
  const scrollerRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);

  const messages = session?.messages ?? [];
  const lastMsg = messages[messages.length - 1];

  // Follow new content automatically while still at the bottom (without interrupting a user scrolling up through history)
  useEffect(() => {
    if (!follow) return;
    const el = scrollerRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [
    follow,
    messages.length,
    streamText,
    toolCards.length,
    applied.length,
    lastMsg?.text,
  ]);

  const onScroll = () => {
    const el = scrollerRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setFollow(distance <= NEAR_BOTTOM_PX);
  };

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scrollerRef}
        onScroll={onScroll}
        className="h-full space-y-2.5 overflow-y-auto px-3 py-3"
      >
        {messages.map((m, i) => {
          if (m.role === "system") return null;
          if (m.role === "user") return <UserBubble key={i} text={m.text} />;
          if (m.role === "tool") return <HistoryToolRow key={i} message={m} />;
          return (
            <div key={i} className="space-y-1.5">
              {m.reasoning && (
                <details className="rounded-md border border-border bg-muted/30 px-2.5 py-1.5">
                  <summary className="cursor-pointer text-xs text-muted-foreground">
                    {t("ai.reasoning")}
                  </summary>
                  <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-muted-foreground">
                    {m.reasoning}
                  </pre>
                </details>
              )}
              {m.text && <AssistantText text={m.text} />}
              {/* Tool calls in history: collapsed into one row (tool name · entity name), expanding on demand shows the argument JSON.
                  This used to be a **permanent JSON block** — create-request / load-test arguments are large and filled the conversation within a few turns,
                  while what users need in the conversation is "what was done", not the raw arguments. */}
              {m.toolCalls?.map((call) => {
                const subject = toolSubject(call.arguments);
                return (
                  <details
                    key={call.id}
                    className="rounded-md bg-muted/30 px-2 py-1"
                  >
                    <summary className="cursor-pointer text-xs text-muted-foreground">
                      <Wrench className="mr-1 inline h-3 w-3 align-[-1px]" />
                      {t(toolLabelKey(call.name), call.name)}
                      {subject && (
                        <span className="text-foreground/70"> · {subject}</span>
                      )}
                    </summary>
                    <pre className="mt-1 max-h-40 overflow-auto rounded bg-background/70 p-1.5 font-mono text-xs">
                      {JSON.stringify(call.arguments, null, 2)}
                    </pre>
                  </details>
                );
              })}
            </div>
          );
        })}

        {/* ── Current turn (the part not yet persisted) ── */}
        {streamReasoning && (
          <details
            open
            className="rounded-md border border-border bg-muted/30 px-2.5 py-1.5"
          >
            <summary className="cursor-pointer text-xs text-muted-foreground">
              {t("ai.reasoning")}
            </summary>
            <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-muted-foreground">
              {streamReasoning}
            </pre>
          </details>
        )}

        {/* Prose before tool cards: within a turn the model always speaks first and then makes tool calls */}
        {streamText && <AssistantText text={streamText} />}

        {toolCards.map((card) => (
          <ToolCallCard
            key={card.callId}
            card={card}
            onDecide={
              card.status === "pendingApproval"
                ? (allow) => onDecide(card.callId, allow)
                : undefined
            }
          />
        ))}

        {applied.map((p) => (
          <ProposalCard key={p.id} proposal={p} />
        ))}

        {streaming && !streamText && (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            {t("ai.thinking")}
          </div>
        )}

        {steps.length > 0 && (
          <details className="rounded-md border border-border bg-muted/30 px-2.5 py-1.5">
            <summary className="cursor-pointer text-xs text-muted-foreground">
              {t("ai.steps")} ({steps.length})
            </summary>
            <ul className="mt-1 space-y-0.5">
              {steps.map((s, i) => (
                <li
                  key={i}
                  className="truncate font-mono text-xs text-muted-foreground"
                >
                  {s}
                </li>
              ))}
            </ul>
          </details>
        )}
      </div>

      {!follow && (
        <button
          type="button"
          onClick={() => {
            setFollow(true);
            const el = scrollerRef.current;
            if (el) el.scrollTop = el.scrollHeight;
          }}
          className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-popover px-2.5 py-1 text-xs text-muted-foreground shadow-md hover:text-foreground"
        >
          <ArrowDown className="h-3.5 w-3.5" />
          {t("ai.scrollToBottom")}
        </button>
      )}
    </div>
  );
}
