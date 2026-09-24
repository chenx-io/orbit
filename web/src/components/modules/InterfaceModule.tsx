// Main request-debugging module: the URL bar plus the request builder (RequestTabs) and the response panel (ResponsePanel).
// This file only orchestrates (state comes from the store plus a little local UI state); the concrete UI lives in the interface/ subdirectory.
import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Columns2,
  Copy,
  Rows2,
  Save,
  Send,
  Sparkles,
} from "lucide-react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { UrlInput, type UrlInputHandle } from "@/components/common/UrlInput";
import { useT } from "@/lib/i18n";
import { cn, methodColor } from "@/lib/utils";
import { useActiveRequest, useAppStore } from "@/store/useStore";
import { RequestTabs } from "./interface/RequestTabs";
import { ResponsePanel } from "./interface/ResponsePanel";
import { ProtocolRequestEditor } from "./interface/ProtocolRequestEditor";
import { GrpcPackageEditor } from "./interface/GrpcPackageEditor";
import { GrpcServiceEditor } from "./interface/GrpcServiceEditor";
import { GrpcRpcEditor } from "./interface/GrpcRpcEditor";
import { isHttpRequest, requestProtocol } from "@/data/types";

const METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

export function InterfaceModule() {
  const req = useActiveRequest();
  const activeGrpcNode = useAppStore((s) => s.activeGrpcNode);
  const activeTab = useAppStore((s) =>
    s.tabs.find((t) => t.id === s.activeTabId),
  );
  const save = useAppStore((s) => s.saveDraft);
  const saveAs = useAppStore((s) => s.saveDraftAs);
  const restoreDraft = useAppStore((s) => s.restoreDraft);
  const isDirty = useAppStore((s) => !!s.dirty[req?.id ?? ""]);
  const send = useAppStore((s) => s.sendRequest);
  const loading = useAppStore((s) => s.loading);
  const response = useAppStore((s) => s.response);
  const assertions = useAppStore((s) => s.assertionResults);
  const responseError = useAppStore((s) => s.responseError);
  const responseTab = useAppStore((s) => s.responseTab);
  const setResponseTab = useAppStore((s) => s.setResponseTab);
  const requestTab = useAppStore((s) => s.requestTab);
  const setRequestTab = useAppStore((s) => s.setRequestTab);
  const locale = useAppStore((s) => s.locale);
  const models = useAppStore((s) => s.models);
  const aiFocusEntity = useAppStore((s) => s.aiFocusEntity);
  const updateDraft = useAppStore((s) => s.updateDraft);
  const urlRef = useRef<UrlInputHandle>(null);
  const { t, format } = useT();
  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const [saveAsName, setSaveAsName] = useState("");
  const saveAsRef = useRef<HTMLInputElement>(null);
  // Request / response layout: vertical = stacked, horizontal = side by side (persisted to localStorage)
  const [layout, setLayout] = useState<"vertical" | "horizontal">(() => {
    try {
      const saved = localStorage.getItem("orbit:req-resp-layout");
      return saved === "horizontal" ? "horizontal" : "vertical";
    } catch {
      return "vertical";
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem("orbit:req-resp-layout", layout);
    } catch {
      /* ignore */
    }
  }, [layout]);

  // grpc package/service tabs (dedicated tabs, consistent with request tabs)
  const grpcNodeTab = activeTab?.grpcNode ?? null;
  if (grpcNodeTab) {
    return grpcNodeTab.type === "grpc-package" ? (
      <GrpcPackageEditor
        key={grpcNodeTab.nodeId}
        collectionId={grpcNodeTab.collectionId}
        id={grpcNodeTab.nodeId}
      />
    ) : (
      <GrpcServiceEditor
        key={grpcNodeTab.nodeId}
        collectionId={grpcNodeTab.collectionId}
        id={grpcNodeTab.nodeId}
      />
    );
  }

  // Selecting a package/service node of a grpc collection renders its editor (rpc goes through a request tab)
  if (activeGrpcNode) {
    return activeGrpcNode.type === "grpc-package" ? (
      <GrpcPackageEditor
        key={activeGrpcNode.id}
        collectionId={activeGrpcNode.collectionId}
        id={activeGrpcNode.id}
      />
    ) : (
      <GrpcServiceEditor
        key={activeGrpcNode.id}
        collectionId={activeGrpcNode.collectionId}
        id={activeGrpcNode.id}
      />
    );
  }

  if (!req) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        {t("request.selectRequest")}
      </div>
    );
  }

  // gRPC: a dedicated editor (URL bar + Method + Message/Metadata/Auth/script tabs + response panel, matching the HTTP layout)
  if (requestProtocol(req) === "grpc") {
    return (
      <GrpcRpcEditor
        key={req.id}
        req={req as import("@/data/types").GrpcRequest}
      />
    );
  }

  // Non-HTTP protocols: render the protocol-specific editor (HTTP uses the existing RequestTabs)
  if (!isHttpRequest(req)) {
    // key=req.id: force a fresh instance when switching requests, keeping each request's session and message list independent
    return <ProtocolRequestEditor key={req.id} req={req} />;
  }

  const set = (patch: Parameters<typeof updateDraft>[1]) =>
    updateDraft(req.id, patch);

  return (
    <div className="flex h-full flex-col">
      {/* URL bar */}
      <div className="flex items-center gap-2 border-b border-border px-3 py-2">
        <div className="flex flex-1 items-center gap-1.5 rounded-md border border-input bg-background px-2">
          <Select value={req.method} onValueChange={(v) => set({ method: v })}>
            <SelectTrigger className="h-7 w-24 border-0 px-1 shadow-none focus:ring-0">
              <SelectValue>
                <span className={cn("font-bold", methodColor(req.method))}>
                  {req.method}
                </span>
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              {METHODS.map((m) => (
                <SelectItem
                  key={m}
                  value={m}
                  className={cn("font-bold", methodColor(m))}
                >
                  {m}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <UrlInput
            ref={urlRef}
            value={req.url}
            onChange={(v) => set({ url: v })}
            locale={locale}
          />
          <DynamicValuePicker
            onInsert={(t) => urlRef.current?.insert(`{{${t}}}`)}
          />
        </div>
        <Button
          size="sm"
          className="gap-1.5"
          onClick={() => send()}
          disabled={loading}
        >
          <Send className="h-4 w-4" />{" "}
          {loading ? t("common.sending") : t("common.send")}
        </Button>
        <div className="flex items-center">
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 rounded-r-none"
            onClick={() => save(req.id)}
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
              <DropdownMenuItem
                onClick={() => {
                  setSaveAsName(format("request.copyName", req.name));
                  setSaveAsOpen(true);
                  setTimeout(() => saveAsRef.current?.select(), 50);
                }}
              >
                <Copy className="h-4 w-4" /> {t("common.duplicate")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* AI generate / optimize: open the drawer with the current request as context */}
        <Button
          variant="outline"
          size="icon"
          className="h-8 w-8 shrink-0"
          title={t("ai.editEntry")}
          aria-label={t("ai.editEntry")}
          onClick={() =>
            aiFocusEntity({
              kind: "request",
              id: req.id,
              name: req.name || t("ai.session.new"),
              detail: `${req.method} ${req.url}`.trim(),
            })
          }
        >
          <Sparkles className="h-4 w-4 text-violet-400" />
        </Button>

        {/* Layout switch: stacked / side by side */}
        <Button
          variant="outline"
          size="icon"
          className="h-8 w-8 shrink-0"
          title={
            layout === "vertical"
              ? t("layout.toHorizontal")
              : t("layout.toVertical")
          }
          aria-label={
            layout === "vertical"
              ? t("layout.toHorizontal")
              : t("layout.toVertical")
          }
          onClick={() =>
            setLayout((l) => (l === "vertical" ? "horizontal" : "vertical"))
          }
        >
          {layout === "vertical" ? (
            <Columns2 className="h-4 w-4" />
          ) : (
            <Rows2 className="h-4 w-4" />
          )}
        </Button>
      </div>

      <PanelGroup
        direction={layout === "vertical" ? "vertical" : "horizontal"}
        autoSaveId="interface-layout"
        className="flex-1"
      >
        {/* Request builder */}
        <Panel defaultSize={25} minSize={25}>
          <RequestTabs
            req={req}
            locale={locale}
            models={models}
            requestTab={requestTab}
            setRequestTab={setRequestTab}
          />
        </Panel>

        <PanelResizeHandle
          className={
            layout === "vertical"
              ? "h-1 shrink-0 bg-border transition-colors hover:bg-primary"
              : "w-1 shrink-0 bg-border transition-colors hover:bg-primary"
          }
        />

        {/* Response */}
        <Panel defaultSize={75} minSize={25}>
          <ResponsePanel
            req={req}
            response={response}
            responseError={responseError}
            assertions={assertions}
            responseTab={responseTab}
            setResponseTab={setResponseTab}
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
                    saveAs(req.id, n);
                    restoreDraft(req.id);
                    setSaveAsOpen(false);
                  }
                }
                if (e.key === "Escape") setSaveAsOpen(false);
              }}
              className="text-sm"
            />
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSaveAsOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              onClick={() => {
                const n = saveAsName.trim();
                if (n) {
                  saveAs(req.id, n);
                  restoreDraft(req.id);
                  setSaveAsOpen(false);
                }
              }}
              disabled={!saveAsName.trim()}
            >
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
