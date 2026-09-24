// Non-HTTP protocol request editor (shell): top connection bar (address / edit connection / send) + protocol panel dispatch + session response area.
// The session state machine is factored into the useSession hook; each protocol's config form lives in the protocol/ directory.
import { useMemo, useRef, useState } from "react";
import { ChevronDown, Copy, Save, Send, Settings2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import type { OpenSessionOptions } from "@/lib/bridge";
import type {
  ApiRequest,
  GrpcRequest,
  GraphqlRequest,
  HttpRequest,
  PayloadType,
  PluginRequest,
  SseRequest,
  TcpRequest,
  UdpRequest,
  WsRequest,
} from "@/data/types";
import { PROTOCOL_LABEL } from "@/lib/utils";
import { findConnectionCollection } from "@/components/layout/collection-tree/treeUtils";
import { ConnectionEditDialog } from "@/components/layout/collection-tree/ConnectionEditDialog";
import { useSession } from "@/hooks/useSession";
import { WebSocketPanel } from "./protocol/WebSocketPanel";
import { TcpPanel, UdpPanel } from "./protocol/TcpUdpPanel";
import { GrpcPanel } from "./protocol/GrpcPanel";
import { SsePanel } from "./protocol/SsePanel";
import { GraphqlPanel } from "./protocol/GraphqlPanel";
import { PluginPanel } from "./protocol/PluginPanel";
import { SessionResponsePanel } from "./protocol/SessionResponsePanel";
import { MessageComposer, type MsgFormat } from "./protocol/MessageComposer";

export function ProtocolRequestEditor({
  req,
}: {
  req: Exclude<ApiRequest, HttpRequest>;
}) {
  const updateDraft = useAppStore((s) => s.updateDraft);
  const saveDraft = useAppStore((s) => s.saveDraft);
  const saveDraftAs = useAppStore((s) => s.saveDraftAs);
  const isDirty = useAppStore((s) => !!s.dirty[req.id]);
  const globalVariables = useAppStore((s) => s.globalVariables);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const environments = useAppStore((s) => s.environments);
  const collections = useAppStore((s) => s.collections);
  const updateConnection = useAppStore((s) => s.updateConnection);
  const { t } = useT();
  const urlRef = useRef<HTMLInputElement>(null);
  const saveAsRef = useRef<HTMLInputElement>(null);
  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const [saveAsName, setSaveAsName] = useState("");
  const [connEditOpen, setConnEditOpen] = useState(false);
  const [sendPayload, setSendPayload] = useState("");
  const [sendFormat, setSendFormat] = useState<MsgFormat>("text");
  const [sendAuto, setSendAuto] = useState(false);
  const [sendInterval, setSendInterval] = useState(1000);

  const session = useSession();
  const set = (patch: Record<string, unknown>) => updateDraft(req.id, patch);
  const protocol = req.protocol;
  const envVars = {
    ...globalVariables,
    ...(environments.find((e) => e.id === activeEnvId)?.variables ?? {}),
  };

  // Messages opened inside a connection-typed collection: connection config (url/framing/codec etc.) is inherited from the collection
  const connCol = useMemo(
    () => findConnectionCollection(collections, req.id),
    [collections, req.id],
  );
  const effectiveUrl = connCol?.connection?.url ?? req.url;
  const connFraming = connCol?.connection?.framing;
  const connCloseAfter = connCol?.connection?.closeAfter;

  /** Build the session-open options (connection fields inherited from the collection connection, request-level overrides).
   * Note: req is Exclude<ApiRequest, HttpRequest> (includes PluginRequest, whose protocol is a string),
   * so the switch cannot narrow automatically and each case must assert explicitly. */
  const sessionOptions = (): OpenSessionOptions => {
    const base = { protocol, url: effectiveUrl, envVars };
    switch (protocol) {
      case "websocket": {
        const ws = req as WsRequest;
        return {
          ...base,
          closeAfter:
            (ws.closeAfter ?? connCloseAfter ?? 0) > 1
              ? (ws.closeAfter ?? connCloseAfter)
              : 0,
          headers: headersToRecord(ws.headers),
          preScript: ws.prereqScript,
          postScript: ws.postreqScript,
        };
      }
      case "tcp": {
        const tc = req as TcpRequest;
        return {
          ...base,
          framing:
            (tc.framing ?? connFraming)
              ? {
                  mode: (tc.framing ?? connFraming)!.mode,
                  delimiter: (tc.framing ?? connFraming)!.delimiter,
                  fixedLen: (tc.framing ?? connFraming)!.fixedLen,
                  bigEndian: (tc.framing ?? connFraming)!.bigEndian,
                }
              : null,
          preScript: tc.prereqScript,
          postScript: tc.postreqScript,
        };
      }
      case "udp": {
        const ud = req as UdpRequest;
        return {
          ...base,
          preScript: ud.prereqScript,
          postScript: ud.postreqScript,
        };
      }
      case "grpc": {
        const gr = req as GrpcRequest;
        return {
          ...base,
          service: gr.service,
          messageFormat: gr.messageFormat,
          streaming: gr.streaming ?? undefined,
          payload: gr.message,
          payloadType: "text",
          headers: headersToRecord(gr.headers),
          preScript: gr.prereqScript,
          postScript: gr.postreqScript,
        };
      }
      case "sse": {
        const ss = req as SseRequest;
        return {
          ...base,
          maxEvents: ss.maxEvents,
          postScript: ss.postreqScript,
        };
      }
      case "graphql": {
        const gq = req as GraphqlRequest;
        return {
          ...base,
          query: gq.query,
          variables: gq.variables,
          operationName: gq.operationName,
          headers: headersToRecord(gq.headers),
          postScript: gq.postreqScript,
        };
      }
      default:
        // Plugin protocols (native/wasm dynamic): connection config inherited from the collection (passed through to plugin execute),
        // message sequence is sent via the session by MessageComposer.
        return {
          ...base,
          connection: connCol?.connection ?? null,
          headers: headersToRecord((req as PluginRequest).headers ?? []),
          preScript: req.prereqScript,
          postScript: req.postreqScript,
        };
    }
  };

  const onConnect = async () => {
    if (session.connecting || !effectiveUrl.trim()) return;
    await session.openSession(sessionOptions());
  };

  // Plugin protocols (native/wasm dynamic): use the single-shot request model
  const isPluginProtocol = ![
    "websocket",
    "tcp",
    "udp",
    "grpc",
    "sse",
    "graphql",
  ].includes(protocol);

  const onSend = async () => {
    // Plugin protocol: prefer the send box for payload, otherwise fall back to the message param (requestConfigSchema) `sql`
    let payload = sendPayload;
    if (isPluginProtocol && !payload.trim()) {
      const opts = (req as PluginRequest).options as
        Record<string, unknown> | undefined;
      payload = (opts?.sql as string | undefined) ?? "";
    }
    if (!payload.trim()) return;
    // Map send format to payload type: json/text → text (UTF-8 bytes), base64/hex kept as-is
    const payloadType: PayloadType =
      sendFormat === "base64"
        ? "base64"
        : sendFormat === "hex"
          ? "hex"
          : "text";
    // Plugin protocol: auto-connect first (openSession) if not connected, then send SQL — completing the query in one shot
    // openSession updates sessionIdRef synchronously, so sendMessage can read it immediately after openSession returns
    if (isPluginProtocol && !session.connected) {
      await session.openSession(sessionOptions());
    }
    void session.sendMessage(payload, payloadType);
  };

  /** Insert a dynamic value / variable template at the URL cursor (writes to the collection connection config for connected collections) */
  const insertDynamic = (token: string) => {
    const el = urlRef.current;
    if (!el) {
      if (connCol) {
        updateConnection(connCol.id, {
          ...connCol.connection,
          url: (connCol.connection?.url ?? "") + token,
        });
      } else {
        set({ url: (req.url || "") + token });
      }
      return;
    }
    const start = el.selectionStart ?? effectiveUrl.length;
    const end = el.selectionEnd ?? effectiveUrl.length;
    const next = effectiveUrl.slice(0, start) + token + effectiveUrl.slice(end);
    if (connCol) {
      updateConnection(connCol.id, { ...connCol.connection, url: next });
    } else {
      set({ url: next });
    }
    requestAnimationFrame(() => {
      el.focus();
      const pos = start + token.length;
      el.setSelectionRange(pos, pos);
    });
  };

  const openSaveAs = () => {
    setSaveAsName(`${req.name} (${t("common.copy")})`);
    setSaveAsOpen(true);
    setTimeout(() => saveAsRef.current?.select(), 50);
  };

  return (
    <div className="flex h-full flex-col">
      {/* Top connection bar */}
      <div className="flex items-center gap-2 border-b border-border px-3 py-2">
        <div className="flex flex-1 items-center gap-1.5 rounded-md border border-input bg-background px-2">
          <span className="rounded bg-muted/60 px-2 py-0.5 text-xs font-bold text-muted-foreground">
            {PROTOCOL_LABEL[protocol] ?? protocol}
          </span>
          <span
            className={
              session.connected
                ? "text-xs text-emerald-500"
                : "text-xs text-muted-foreground"
            }
          >
            {session.connected
              ? t("session.connected")
              : t("session.disconnected")}
          </span>
          <Input
            ref={urlRef}
            className="h-8 flex-1 border-0 shadow-none focus-visible:ring-0"
            placeholder={
              connCol ? t("proto.connAddrCollection") : t("proto.connAddr")
            }
            value={effectiveUrl}
            onChange={(e) => {
              if (connCol) {
                updateConnection(connCol.id, {
                  ...connCol.connection,
                  url: e.target.value,
                });
              } else {
                set({ url: e.target.value });
              }
            }}
          />
          {connCol && (
            <Button
              variant="ghost"
              size="icon-sm"
              className="h-7 w-7 shrink-0"
              title={t("proto.editConnConfig")}
              onClick={() => setConnEditOpen(true)}
            >
              <Settings2 className="h-3.5 w-3.5" />
            </Button>
          )}
          <DynamicValuePicker
            onInsert={(token) => insertDynamic(`{{${token}}}`)}
          />
        </div>
        {session.connected ? (
          <Button
            size="sm"
            variant="destructive"
            className="gap-1.5"
            onClick={() => void session.closeSession()}
          >
            <X className="h-4 w-4" /> {t("session.close")}
          </Button>
        ) : (
          <Button
            size="sm"
            className="gap-1.5"
            onClick={() => void (isPluginProtocol ? onSend() : onConnect())}
            disabled={
              session.connecting ||
              (!isPluginProtocol && !effectiveUrl.trim()) ||
              (isPluginProtocol && !sendPayload.trim())
            }
          >
            <Send className="h-4 w-4" />{" "}
            {session.connecting ? t("session.connecting") : t("common.send")}
          </Button>
        )}
        <div className="flex items-center">
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 rounded-r-none"
            onClick={() => saveDraft(req.id)}
            disabled={!isDirty}
          >
            <Save className="h-4 w-4" /> {t("common.save")}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                className="rounded-l-none border-l-0 px-1.5"
              >
                <ChevronDown className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={openSaveAs}>
                <Copy className="h-4 w-4" /> {t("common.duplicate")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      {/* Interactive send area: shown after connecting for long-connection sessions; always shown for plugin protocols (single-shot) */}
      {(session.connected && session.canSend) || isPluginProtocol ? (
        <MessageComposer
          value={sendPayload}
          onChange={setSendPayload}
          format={sendFormat}
          onFormatChange={setSendFormat}
          onSend={onSend}
          sending={session.sending}
          placeholder={
            isPluginProtocol
              ? t("proto.sendPlaceholderPlugin")
              : t("proto.sendPlaceholderSession")
          }
          autoSend={sendAuto}
          onAutoSendChange={setSendAuto}
          intervalMs={sendInterval}
          onIntervalChange={setSendInterval}
        />
      ) : null}

      {/* Script / connection error banner */}
      {session.notice && (
        <div
          className={`border-b border-border px-3 py-1.5 text-xs ${
            session.notice.kind === "error"
              ? "text-rose-500"
              : "text-muted-foreground"
          }`}
        >
          {session.notice.kind === "error" ? "✗" : "ℹ"} {session.notice.text}
        </div>
      )}

      <PanelGroup direction="vertical" className="min-h-0 flex-1">
        <Panel defaultSize={56} minSize={20}>
          <div className="h-full space-y-3 overflow-y-auto p-3">
            {protocol === "websocket" && (
              <WebSocketPanel req={req as WsRequest} set={set} />
            )}
            {protocol === "tcp" && (
              <TcpPanel
                req={req as Extract<ApiRequest, { protocol: "tcp" }>}
                set={set}
              />
            )}
            {protocol === "udp" && (
              <UdpPanel req={req as UdpRequest} set={set} />
            )}
            {protocol === "grpc" && (
              <GrpcPanel
                req={req as GrpcRequest}
                set={set}
                connectUrl={effectiveUrl}
              />
            )}
            {protocol === "sse" && (
              <SsePanel req={req as SseRequest} set={set} />
            )}
            {protocol === "graphql" && (
              <GraphqlPanel req={req as GraphqlRequest} set={set} />
            )}
            {!["websocket", "tcp", "udp", "grpc", "sse", "graphql"].includes(
              protocol,
            ) && <PluginPanel req={req as PluginRequest} set={set} />}
          </div>
        </Panel>
        <PanelResizeHandle className="h-1 shrink-0 bg-border transition-colors hover:bg-primary" />
        <Panel defaultSize={44} minSize={15}>
          <SessionResponsePanel
            protocol={protocol}
            messages={session.messages}
            connected={session.connected}
            connecting={session.connecting}
            onClear={session.clearMessages}
          />
        </Panel>
      </PanelGroup>

      {/* Save-as-copy dialog */}
      <Dialog
        open={saveAsOpen}
        onOpenChange={(o) => {
          if (!o) setSaveAsOpen(false);
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("request.saveAsTitle")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <Label className="text-xs">{t("request.saveAsLabel")}</Label>
            <Input
              ref={saveAsRef}
              value={saveAsName}
              onChange={(e) => setSaveAsName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  const n = saveAsName.trim();
                  if (n) {
                    saveDraftAs(req.id, n);
                    setSaveAsOpen(false);
                  }
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setSaveAsOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => {
                const n = saveAsName.trim();
                if (!n) return;
                saveDraftAs(req.id, n);
                setSaveAsOpen(false);
              }}
            >
              {t("common.duplicate")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Connection-typed collection: edit connection config (collection-level, shared by messages) */}
      {connCol && (
        <ConnectionEditDialog
          open={connEditOpen}
          initial={connCol.connection}
          onOpenChange={setConnEditOpen}
          onConfirm={(config) => updateConnection(connCol.id, config)}
        />
      )}
    </div>
  );
}

function headersToRecord(
  headers: { enabled?: boolean; key: string; value: string }[],
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const h of headers) if (h.enabled && h.key) out[h.key] = h.value;
  return out;
}
