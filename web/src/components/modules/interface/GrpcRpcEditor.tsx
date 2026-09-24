// gRPC RPC editor: top address bar + Method (aligned with the HTTP layout), middle Tabs (Message/Metadata/Auth/scripts),
// bottom response area. Sending goes through the existing session long-connection path (GrpcClient supports unary / 4 streaming modes and JSON↔proto).
import { useEffect, useMemo, useRef, useState } from "react";
import { Cable, Link2, Save, Send, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { useAppStore, useEnvVarNames } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { getUiLocale } from "@/lib/localeDict";
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
import { buildAuthHeaders } from "@/lib/request";
import { resolveTemplate } from "@/lib/resolve";
import { messageSchema } from "@/lib/bridge/grpc";
import { generateGrpcBody } from "@/lib/grpcSchemaGen";
import { BodyEditor, type BodyEditorHandle } from "./BodyEditor";
import { AuthEditor } from "./AuthEditor";
import { RequestActionsEditor } from "./actions/RequestActionsEditor";
import { ResponseSchemaFields } from "./ResponseSchemaDialog";
import { ScriptPanel } from "./ScriptPanel";
import { ErrorBoundary } from "@/components/common/ErrorBoundary";
import {
  actionsToLegacyScript,
  getPostActions,
  getPreActions,
} from "@/lib/requestActions";
import type { DataSourceOption } from "@/lib/dbQuery";
import type { GrpcRequest, ScriptLog, TestResult } from "@/data/types";

function formatTime(time: number): string {
  const d = new Date(time);
  return `${d.toLocaleTimeString(getUiLocale(), { hour12: false })}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

function base64ToBytes(data: string): Uint8Array {
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(" ");
}

function decodeMessage(data: string): { text: string | null; hex: string } {
  try {
    const bytes = base64ToBytes(data);
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return { text, hex: toHex(bytes) };
  } catch {
    return { text: null, hex: toHex(base64ToBytes(data)) };
  }
}

export function GrpcRpcEditor({ req }: { req: GrpcRequest }) {
  const updateDraft = useAppStore((s) => s.updateDraft);
  const saveDraft = useAppStore((s) => s.saveDraft);
  const isDirty = useAppStore((s) => !!s.dirty[req.id]);
  const locale = useAppStore((s) => s.locale);
  const globalVariables = useAppStore((s) => s.globalVariables);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const environments = useAppStore((s) => s.environments);
  const collections = useAppStore((s) => s.collections);
  const envVarNames = useEnvVarNames();
  // Data-source options the action editor needs (the gRPC session path uses allowDb=false; only for signature compatibility)
  const dataSources = useAppStore((s) => s.dataSources) as DataSourceOption[];

  const urlRef = useRef<HTMLInputElement>(null);
  const msgRef = useRef<BodyEditorHandle>(null);
  const streamRef = useRef<SessionStreamHandle | null>(null);
  const [tab, setTab] = useState<
    "message" | "interface" | "metadata" | "auth" | "prescript" | "postscript"
  >("message");
  const [templateLoading, setTemplateLoading] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [sending, setSending] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [notice, setNotice] = useState<{
    kind: "error" | "info";
    text: string;
  } | null>(null);
  // The "interface definition" tab's input/output model schema
  const [ifaceSchema, setIfaceSchema] = useState<{
    input: { properties: Record<string, unknown> } | null;
    output: { properties: Record<string, unknown> } | null;
    loading: boolean;
  }>({ input: null, output: null, loading: false });
  // Bottom response-area tabs: messages / scripts (script logs)
  const [responseTab, setResponseTab] = useState<"messages" | "scripts">(
    "messages",
  );
  const { t, format } = useT();

  // Aggregate pre/post script logs from all messages (for the "scripts" tab)
  const scriptLogs = useMemo(() => {
    const pre: ScriptLog[] = [];
    const post: ScriptLog[] = [];
    const tests: TestResult[] = [];
    for (const m of messages) {
      if (m.pre_logs?.length) pre.push(...m.pre_logs);
      if (m.post_logs?.length) post.push(...m.post_logs);
    }
    return { pre, post, tests };
  }, [messages]);

  const set = (patch: Partial<GrpcRequest>) => updateDraft(req.id, patch);
  const envVars = {
    ...globalVariables,
    ...(environments.find((e) => e.id === activeEnvId)?.variables ?? {}),
  };

  // Full method path: /pkg.Service/Method
  const methodPath = `/${[req.packageName, req.serviceName].filter(Boolean).join(".")}/${req.method ?? ""}`;

  useEffect(() => {
    return () => {
      streamRef.current?.unlisten();
      if (sessionId) void sessionClose(sessionId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const resetSession = () => {
    streamRef.current?.unlisten();
    streamRef.current = null;
    setSessionId(null);
    setConnected(false);
  };

  const handleEvent = (ev: SessionEvent) => {
    switch (ev.type) {
      case "sent":
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
                  pre_logs: ev.pre_logs ?? [],
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
                  post_logs: ev.post_logs ?? [],
                  sse: ev.sse ?? null,
                  time: ev.time,
                },
              ],
        );
        break;
      case "error":
        setNotice({ kind: "error", text: ev.message });
        setMessages((m) => [
          ...m,
          {
            seq: -1 - m.length,
            direction: "recv",
            data: "",
            text: null,
            decoded: null,
            post_logs: [],
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
  };

  // metadata + auth → headers (gRPC metadata)
  const buildHeaders = (): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const h of req.metadata ?? [])
      if (h.enabled && h.key) out[h.key] = h.value;
    for (const a of buildAuthHeaders(req.auth ?? { type: "none" }))
      out[a.key] = a.value;
    return out;
  };

  // The owning gRPC collection (address fallback / descriptor source)
  const grpcCollection = useMemo(
    () =>
      collections.find(
        (c) =>
          c.kind === "grpc" &&
          c.items.some(
            (it) => it.type === "grpc-rpc" && it.requestId === req.id,
          ),
      ),
    [collections, req.id],
  );

  // Effective server address: falls back to the gRPC collection connection's configured address when the request has none (empty)
  const effectiveUrl = (
    req.url?.trim() ||
    grpcCollection?.connection?.url ||
    ""
  ).trim();

  const sessionOptions = (payloadOverride?: string): OpenSessionOptions => ({
    protocol: "grpc",
    url: effectiveUrl,
    service: methodPath,
    messageFormat: req.messageFormat ?? "json",
    streaming: req.streaming ?? undefined,
    payload: payloadOverride ?? req.message ?? "",
    payloadType: "text",
    headers: buildHeaders(),
    // The session path supports scripts only: multiple scripts are merged in order into a single script to send (DB actions unsupported)
    preScript: actionsToLegacyScript(getPreActions(req)),
    postScript: actionsToLegacyScript(getPostActions(req)),
    envVars,
  });

  // Resolve dynamic-value templates in the message: random values like {{$uuid}} + {{variable}}
  const resolvePayload = async (text: string) =>
    resolveTemplate(text, envVars, locale);

  // Streaming-mode detection
  const mode = req.streaming; // undefined | server_streaming | client_streaming | bidirectional
  const isUnary = !mode; // One-shot call: responds once after sending, then auto-ends
  // client_streaming / bidirectional keep sending messages after the connection is open
  const canStreamSend = mode === "client_streaming" || mode === "bidirectional";

  const handleSend = async () => {
    if (connecting || !effectiveUrl) {
      if (!effectiveUrl) setNotice({ kind: "error", text: t("grpc.rpcNoUrl") });
      return;
    }
    // Connected (client_stream / bidi): send one more message as a streaming request
    if (sessionId && connected && canStreamSend) {
      await sendCurrent();
      return;
    }
    // Not connected: open a session (unary runs immediately and auto-ends; streaming opens a persistent connection)
    setConnecting(true);
    setNotice(null);
    setMessages([]);
    try {
      // Resolve the initial message's dynamic-value template first, then send
      const resolvedPayload = await resolvePayload(req.message ?? "");
      const res = await sessionOpen(sessionOptions(resolvedPayload));
      setSessionId(res.session_id);
      // unary is one-shot: it never enters the "connected" state; once `closed` is received, resetSession resets it
      if (!isUnary) setConnected(true);
      streamRef.current = connectSessionEvents(res.session_id, handleEvent);
      await streamRef.current.ready;
      const hist = await sessionMessages(res.session_id).catch(() => []);
      if (hist.length) {
        setMessages((prev) => {
          const seen = new Set(prev.map((m) => m.seq));
          const merged = [...prev];
          for (const h of hist) if (!seen.has(h.seq)) merged.push(h);
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
  };

  const sendCurrent = async () => {
    if (!sessionId || sending) return;
    setSending(true);
    try {
      // Resolve dynamic-value templates before sending: random values like {{$uuid}}/{{$randomInt}} + {{variable}}
      const payload = await resolvePayload(req.message ?? "");
      // gRPC messages are JSON text, base64-encoded before going through the session
      const data = btoa(payload);
      await sessionSend(sessionId, data);
    } catch (e) {
      setNotice({
        kind: "error",
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setSending(false);
    }
  };

  const handleClose = async () => {
    if (!sessionId) return;
    const id = sessionId;
    resetSession();
    setNotice(null);
    await sessionClose(id).catch(() => {});
  };

  const insertDynamic = (token: string) => {
    // Insert based on the currently displayed address (including the collection fallback), then write back into the request's own url
    const base = effectiveUrl;
    const el = urlRef.current;
    if (!el) return set({ url: base + token });
    const start = el.selectionStart ?? base.length;
    const end = el.selectionEnd ?? base.length;
    set({ url: base.slice(0, start) + token + base.slice(end) });
  };

  // Find the gRPC collection containing this rpc and take its descriptorFiles (for message templates / schema)
  const grpcDescriptorFiles = (): string[] | null => {
    const col = collections.find((c) => {
      if (c.kind !== "grpc") return false;
      return c.items.some(
        (it) => it.type === "grpc-rpc" && it.requestId === req.id,
      );
    });
    const files = col?.grpc?.descriptorFiles;
    return files && files.length > 0 ? files : null;
  };

  // The owning gRPC collection (the "follow" button uses the address configured in the collection connection)
  const handleFollowUrl = () => {
    const url = grpcCollection?.connection?.url;
    if (!url || !url.trim()) {
      setNotice({
        kind: "error",
        text: t("grpc.rpcNoCollectionUrl"),
      });
      return;
    }
    set({ url });
  };

  // Auto-generate: build a JSON request body with random values via faker from the input model schema (mirrors HTTP auto-generate)
  const handleGenerateTemplate = async () => {
    if (!req.inputType) {
      setNotice({ kind: "error", text: t("grpc.rpcNoInputType") });
      return;
    }
    const files = grpcDescriptorFiles();
    if (!files) {
      setNotice({
        kind: "error",
        text: t("grpc.rpcNoDescriptor"),
      });
      return;
    }
    setTemplateLoading(true);
    try {
      const schema = await messageSchema(files, req.inputType);
      const json = await generateGrpcBody(schema);
      set({ message: json, messageTemplate: json });
    } catch (e) {
      setNotice({
        kind: "error",
        text: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setTemplateLoading(false);
    }
  };

  // Load the input/output model schema needed by the "interface" tab
  const loadInterfaceSchema = async () => {
    const files = grpcDescriptorFiles();
    if (!files) return;
    setIfaceSchema((s) => ({ ...s, loading: true }));
    const [input, output] = await Promise.all([
      req.inputType
        ? messageSchema(files, req.inputType).catch((e) => {
            // eslint-disable-next-line no-console
            console.error(
              "[grpc] failed to load input message schema",
              req.inputType,
              e,
            );
            return null;
          })
        : Promise.resolve(null),
      req.outputType
        ? messageSchema(files, req.outputType).catch((e) => {
            // eslint-disable-next-line no-console
            console.error(
              "[grpc] failed to load output message schema",
              req.outputType,
              e,
            );
            return null;
          })
        : Promise.resolve(null),
    ]);
    setIfaceSchema({ input, output, loading: false });
  };

  // Load schema when the "interface" tab opens (refresh once per entry)
  useEffect(() => {
    if (tab === "interface") void loadInterfaceSchema();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, req.id]);

  // Insert a dynamic value at the Message JSON editor's cursor
  const insertMsg = (text: string) => {
    const el = msgRef.current;
    if (el && typeof el.insertAtCursor === "function") {
      el.insertAtCursor(text);
      return;
    }
    set({ message: (req.message ?? "") + text });
  };

  return (
    <div className="flex h-full flex-col">
      {/* Top: row 1 = address | send | save; row 2 = Method */}
      <div className="border-b border-border px-3 py-2">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className="shrink-0 gap-1 text-fuchsia-400">
            <Cable className="h-3 w-3" /> gRPC
          </Badge>
          <div className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md border border-input bg-background px-2">
            <Input
              ref={urlRef}
              value={effectiveUrl}
              onChange={(e) => set({ url: e.target.value })}
              placeholder={t("grpc.serverAddrExample")}
              className="h-7 flex-1 border-0 px-1 shadow-none focus:ring-0 font-mono text-xs"
            />
            <Button
              variant="ghost"
              size="icon-sm"
              className="h-6 w-6 shrink-0"
              title={t("grpc.rpcFollow")}
              onClick={handleFollowUrl}
            >
              <Link2 className="h-3.5 w-3.5" />
            </Button>
            <DynamicValuePicker onInsert={(t) => insertDynamic(`{{${t}}}`)} />
          </div>
          {sessionId && connected ? (
            canStreamSend ? (
              <Button
                size="sm"
                className="shrink-0 gap-1.5"
                onClick={handleSend}
                disabled={sending}
              >
                <Send className="h-4 w-4" /> {t("grpc.rpcSend")}
              </Button>
            ) : null
          ) : (
            <Button
              size="sm"
              className="shrink-0 gap-1.5"
              onClick={handleSend}
              disabled={connecting}
            >
              <Send className="h-4 w-4" />{" "}
              {connecting ? t("grpc.rpcConnecting") : t("grpc.rpcSend")}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            className="shrink-0 gap-1.5"
            onClick={() => saveDraft(req.id)}
            disabled={!isDirty}
            title={t("grpc.rpcSaveTitle")}
          >
            <Save className="h-3.5 w-3.5" /> {t("grpc.rpcSave")}
          </Button>
          {sessionId && connected && (
            <Button
              variant="ghost"
              size="sm"
              className="shrink-0 gap-1.5"
              onClick={handleClose}
            >
              <X className="h-4 w-4" /> {t("session.close")}
            </Button>
          )}
        </div>
        {/* Row 2: Method */}
        <div className="mt-1.5 flex shrink-0 items-center gap-1.5 font-mono text-xs">
          <span className="text-muted-foreground">Method</span>
          <span className="text-primary">
            {methodPath || "/pkg.Service/Method"}
          </span>
        </div>
      </div>

      {notice && (
        <div
          className={`border-b px-3 py-1.5 text-xs ${
            notice.kind === "error"
              ? "border-destructive/30 bg-destructive/10 text-destructive"
              : "border-border text-muted-foreground"
          }`}
        >
          {notice.text}
        </div>
      )}

      {/* Middle: Tabs */}
      <div className="flex min-h-0 flex-1 flex-col overflow-auto border-b border-border px-3 pt-2">
        <Tabs value={tab} onValueChange={(v) => setTab(v as typeof tab)}>
          <TabsList>
            <TabsTrigger value="message">Message</TabsTrigger>
            <TabsTrigger value="interface">
              {t("grpc.rpcInterface")}
            </TabsTrigger>
            <TabsTrigger value="metadata">Metadata</TabsTrigger>
            <TabsTrigger value="auth">Auth</TabsTrigger>
            <TabsTrigger value="prescript">
              {t("request.prescript")}
            </TabsTrigger>
            <TabsTrigger value="postscript">
              {t("request.postscript")}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="message" className="mt-2 space-y-2">
            <div className="flex items-center gap-2">
              <DynamicValuePicker
                onInsert={(token) => insertMsg(`{{${token}}}`)}
              />
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={handleGenerateTemplate}
                disabled={templateLoading}
              >
                {templateLoading
                  ? t("grpc.rpcGenerating")
                  : t("request.autoGen")}
              </Button>
            </div>
            <BodyEditor
              ref={msgRef}
              value={req.message ?? ""}
              onChange={(v) => set({ message: v })}
              mode="json"
              variables={envVarNames}
              placeholder="{\n  // Request message (JSON), click Auto-generate to build from the input model\n}"
            />
          </TabsContent>

          <TabsContent value="interface" className="mt-2 space-y-4">
            <ErrorBoundary>
              {/* Interface: input/output models, UI consistent with the HTTP model definition */}
              <div className="rounded-md border border-border">
                <div className="flex items-center gap-2 border-b border-border bg-muted/40 px-3 py-1.5 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
                  <span className="text-primary">{t("grpc.rpcInput")}</span>
                  <span className="font-mono normal-case text-primary/70">
                    {req.inputType || "-"}
                  </span>
                </div>
                <div className="max-h-56 overflow-auto">
                  {ifaceSchema.loading && (
                    <div className="p-3 text-xs text-muted-foreground">
                      {t("grpc.loading")}
                    </div>
                  )}
                  {!ifaceSchema.loading && !ifaceSchema.input && (
                    <div className="p-3 text-xs text-muted-foreground">
                      {t("grpc.rpcNoInput")}
                    </div>
                  )}
                  {ifaceSchema.input && (
                    <ResponseSchemaFields
                      schema={ifaceSchema.input}
                      depth={0}
                      t={t}
                    />
                  )}
                </div>
              </div>

              <div className="rounded-md border border-border">
                <div className="flex items-center gap-2 border-b border-border bg-muted/40 px-3 py-1.5 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
                  <span className="text-emerald-400">
                    {t("grpc.rpcOutput")}
                  </span>
                  <span className="font-mono normal-case text-emerald-400/70">
                    {req.outputType || "-"}
                  </span>
                </div>
                <div className="max-h-56 overflow-auto">
                  {ifaceSchema.loading && (
                    <div className="p-3 text-xs text-muted-foreground">
                      {t("grpc.loading")}
                    </div>
                  )}
                  {!ifaceSchema.loading && !ifaceSchema.output && (
                    <div className="p-3 text-xs text-muted-foreground">
                      {t("grpc.rpcNoOutput")}
                    </div>
                  )}
                  {ifaceSchema.output && (
                    <ResponseSchemaFields
                      schema={ifaceSchema.output}
                      depth={0}
                      t={t}
                    />
                  )}
                </div>
              </div>
            </ErrorBoundary>
          </TabsContent>

          <TabsContent value="metadata" className="mt-2">
            <KeyValueEditor
              items={req.metadata ?? []}
              onChange={(m) => set({ metadata: m })}
              enableDynamic
            />
          </TabsContent>

          <TabsContent value="auth" className="mt-2">
            {/* Reuse AuthEditor: build an HttpRequest shape, forwarding only the auth field */}
            <GrpcAuthEditor
              req={req}
              set={(patch) => set({ auth: patch.auth })}
            />
          </TabsContent>

          <TabsContent value="prescript" className="mt-2 space-y-2 pb-3">
            <RequestActionsEditor
              value={getPreActions(req)}
              onChange={(next) => set({ preActions: next, prereqScript: "" })}
              kind="pre"
              variables={envVarNames}
              allowDb={false}
              dataSources={dataSources}
            />
          </TabsContent>
          <TabsContent value="postscript" className="mt-2 space-y-2 pb-3">
            <RequestActionsEditor
              value={getPostActions(req)}
              onChange={(next) => set({ postActions: next, postreqScript: "" })}
              kind="post"
              variables={envVarNames}
              allowDb={false}
              dataSources={dataSources}
            />
          </TabsContent>
        </Tabs>
      </div>

      {/* Bottom: response area */}
      <div className="flex min-h-0 flex-1 flex-col">
        <Tabs
          value={responseTab}
          onValueChange={(v) => setResponseTab(v as typeof responseTab)}
          className="flex flex-1 min-h-0 flex-col"
        >
          <div className="flex items-center gap-2 border-b border-border px-2 py-1">
            <span className="text-xs font-medium">{t("session.response")}</span>
            {connected && (
              <span className="text-xs text-emerald-500">
                {t("session.connected")}
              </span>
            )}
            <TabsList className="ml-2 h-7">
              <TabsTrigger value="messages" className="px-2 text-xs">
                {format("grpc.rpcMsgCount", messages.length)}
              </TabsTrigger>
              <TabsTrigger value="scripts" className="px-2 text-xs">
                {t("grpc.rpcScripts")}
              </TabsTrigger>
            </TabsList>
            <Button
              variant="ghost"
              size="icon-sm"
              className="ml-auto h-6 w-6"
              title={t("session.clear")}
              onClick={() => setMessages([])}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
          <div className="flex-1 min-h-0">
            <TabsContent value="messages" className="m-0 h-full">
              <div className="h-full space-y-1 overflow-y-auto p-2">
                {messages.length === 0 && (
                  <div className="py-8 text-center text-xs text-muted-foreground">
                    {t("grpc.rpcEmptyHint")}
                  </div>
                )}
                {[...messages].reverse().map((m) => {
                  const { text, hex } = decodeMessage(m.data);
                  const content =
                    m.error != null && m.error !== ""
                      ? m.error
                      : m.decoded != null && m.decoded !== ""
                        ? format("grpc.rpcDecoded", m.decoded)
                        : (text ?? hex);
                  return (
                    <div
                      key={m.seq}
                      className="rounded-md border border-border bg-card/50 px-2 py-1.5 text-xs"
                    >
                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                        <span
                          className={
                            m.error
                              ? "text-rose-400"
                              : m.direction === "send"
                                ? "text-blue-400"
                                : "text-emerald-400"
                          }
                        >
                          {m.error
                            ? t("grpc.rpcErr")
                            : m.direction === "send"
                              ? t("grpc.rpcSent")
                              : t("grpc.rpcRecv")}
                        </span>
                        <span>{formatTime(m.time)}</span>
                        <span className="ml-auto font-mono">
                          {hex.slice(0, 80)}
                          {hex.length > 80 ? "…" : ""}
                        </span>
                      </div>
                      <div className="mt-0.5 break-all font-mono">
                        {content}
                      </div>
                    </div>
                  );
                })}
              </div>
            </TabsContent>
            <TabsContent value="scripts" className="m-0 h-full overflow-auto">
              <ScriptPanel
                preLogs={scriptLogs.pre}
                postLogs={scriptLogs.post}
                postTests={scriptLogs.tests}
              />
            </TabsContent>
          </div>
        </Tabs>
      </div>
    </div>
  );
}

/** Auth editing: reuse AuthEditor's fields, but adapt to GrpcRequest (only the auth field) */
function GrpcAuthEditor({
  req,
  set,
}: {
  req: GrpcRequest;
  set: (p: { auth: GrpcRequest["auth"] }) => void;
}) {
  const locale = useAppStore((s) => s.locale);
  const authReq = {
    ...req,
    protocol: "http" as const,
    method: "GET" as const,
    headers: [],
    queryParams: [],
    body: "",
    bodyMode: "none" as const,
    contentType: "",
    formParams: [],
    binaryFile: null,
    cookies: [],
  };
  return <AuthEditor req={authReq as any} set={set as any} locale={locale} />;
}
