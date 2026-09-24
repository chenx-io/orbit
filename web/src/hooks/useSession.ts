// Long-lived connection session state-machine hook (shared by interactive debugging of non-HTTP protocols):
// Wraps open/close/send and the session event stream (event subscription + history merge + message dedup + unmount cleanup).
import { useCallback, useEffect, useRef, useState } from "react";
import {
  connectSessionEvents,
  sessionClose,
  sessionMessages,
  sessionOpen,
  sessionSend,
  type OpenSessionOptions,
  type SessionEvent,
  type SessionMessage,
  type SessionStreamHandle,
} from "@/lib/bridge";
import type { PayloadType } from "@/data/types";

export interface SessionNotice {
  kind: "error" | "info";
  text: string;
}

export function useSession() {
  const streamRef = useRef<SessionStreamHandle | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [sending, setSending] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const [canSend, setCanSend] = useState(false);
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [notice, setNotice] = useState<SessionNotice | null>(null);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  // Close the session and event stream when the component unmounts
  useEffect(() => {
    return () => {
      streamRef.current?.unlisten();
      if (sessionIdRef.current) {
        void sessionClose(sessionIdRef.current).catch(() => {});
      }
    };
  }, []);

  const resetSession = useCallback(() => {
    streamRef.current?.unlisten();
    streamRef.current = null;
    setSessionId(null);
    setConnected(false);
    setCanSend(false);
  }, []);

  const handleEvent = useCallback(
    (ev: SessionEvent) => {
      switch (ev.type) {
        case "sent":
          // The sent message is already shown immediately by the sendMessage response; dedup by seq here
          setMessages((m) =>
            m.some((x) => x.seq === ev.seq)
              ? m
              : [
                  ...m,
                  {
                    seq: ev.seq,
                    direction: "send",
                    data: ev.data,
                    text: ev.text ?? null,
                    decoded: null,
                    sse: null,
                    error: null,
                    time: ev.time,
                  },
                ],
          );
          break;
        case "received":
          setMessages((m) =>
            m.some((x) => x.seq === ev.seq)
              ? m
              : [
                  ...m,
                  {
                    seq: ev.seq,
                    direction: "recv",
                    data: ev.data,
                    text: ev.text ?? null,
                    decoded: ev.decoded ?? null,
                    sse: ev.sse ?? null,
                    error: null,
                    time: ev.time,
                  },
                ],
          );
          break;
        case "error":
          setNotice({ kind: "error", text: ev.message });
          // Errors also enter the list as system messages, so they aren't ignored as just a narrow banner
          setMessages((m) => [
            ...m,
            {
              seq: -1 - m.length,
              direction: "recv",
              data: "",
              text: null,
              decoded: null,
              sse: null,
              error: ev.message,
              time: ev.time,
            },
          ]);
          break;
        case "closed":
          setNotice(
            ev.reason === "user closed"
              ? null
              : { kind: "info", text: `Connection closed: ${ev.reason}` },
          );
          resetSession();
          break;
        default:
          break;
      }
    },
    [resetSession],
  );

  const openSession = useCallback(
    async (options: OpenSessionOptions) => {
      if (connecting) return;
      setConnecting(true);
      setNotice(null);
      setMessages([]);
      try {
        const res = await sessionOpen(options);
        // Update the ref synchronously so send works right after openSession returns (the single-shot model depends on it)
        sessionIdRef.current = res.session_id;
        setSessionId(res.session_id);
        setConnected(true);
        setCanSend(res.can_send);
        // Subscribe to the event stream first, then merge history: avoids losing messages arriving early (before the subscription is established)
        streamRef.current = connectSessionEvents(res.session_id, handleEvent);
        await streamRef.current.ready;
        const hist = await sessionMessages(res.session_id).catch(() => []);
        if (hist.length) {
          setMessages((prev) => {
            const seen = new Set(prev.map((m) => m.seq));
            const merged = [...prev];
            for (const h of hist) {
              if (!seen.has(h.seq)) {
                seen.add(h.seq);
                merged.push(h);
              }
            }
            return merged.sort((a, b) => a.seq - b.seq);
          });
        }
      } catch (e) {
        setNotice({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setConnecting(false);
      }
    },
    [connecting, handleEvent],
  );

  const closeSession = useCallback(async () => {
    const id = sessionIdRef.current;
    if (!id) return;
    resetSession();
    setNotice(null);
    await sessionClose(id).catch(() => {});
  }, [resetSession]);

  const sendMessage = useCallback(
    async (payload: string, payloadType: PayloadType) => {
      const id = sessionIdRef.current;
      if (!id || sending) return;
      setSending(true);
      try {
        const data = payloadToBase64(payload, payloadType);
        const res = await sessionSend(id, data);
        // Show on screen immediately after a successful send (not relying on the event round-trip, so the user is guaranteed to see the send confirmation)
        const decoded = decodeMessage(data);
        setMessages((m) =>
          m.some((x) => x.seq === res.seq)
            ? m
            : [
                ...m,
                {
                  seq: res.seq,
                  direction: "send",
                  data,
                  text: decoded.text,
                  decoded: null,
                  sse: null,
                  error: null,
                  time: Date.now(),
                },
              ],
        );
      } catch (e) {
        setNotice({
          kind: "error",
          text: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setSending(false);
      }
    },
    [sending],
  );

  const clearMessages = useCallback(() => setMessages([]), []);

  return {
    connecting,
    sending,
    sessionId,
    connected,
    canSend,
    messages,
    notice,
    setNotice,
    openSession,
    closeSession,
    sendMessage,
    resetSession,
    clearMessages,
  };
}

// ─── Byte/encoding utilities (for session message display)─────────────────────────────

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function base64ToBytes(data: string): Uint8Array {
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(" ");
}

export function decodeMessage(data: string): {
  text: string | null;
  hex: string;
} {
  try {
    const bytes = base64ToBytes(data);
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return { text, hex: toHex(bytes) };
  } catch {
    const bytes = base64ToBytes(data);
    return { text: null, hex: toHex(bytes) };
  }
}

export function payloadToBase64(payload: string, type: PayloadType): string {
  if (type === "hex") return bytesToBase64(hexToBytes(payload));
  if (type === "base64") return payload;
  return bytesToBase64(new TextEncoder().encode(payload));
}

function hexToBytes(s: string): Uint8Array {
  const clean = s.replace(/\s+/g, "");
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.substr(i * 2, 2), 16) || 0;
  }
  return out;
}
