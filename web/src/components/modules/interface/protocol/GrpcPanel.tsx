// gRPC panel: service discovery (reflection dropdown) + service method + request message + Metadata + format/streaming.
import { useState } from "react";
import { RefreshCw, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Field } from "./shared";
import { STREAM_MODES } from "./constants";
import { grpcReflect, type GrpcReflectService } from "@/lib/bridge";
import { useT } from "@/lib/i18n";
import type { GrpcRequest } from "@/data/types";

export function GrpcPanel({
  req,
  set,
  connectUrl,
}: {
  req: GrpcRequest;
  set: (patch: Record<string, unknown>) => void;
  /** Connect address (inherited from the collection connection; used for service discovery) */
  connectUrl: string;
}) {
  const [services, setServices] = useState<GrpcReflectService[] | null>(null);
  const [svcSel, setSvcSel] = useState("");
  const [methodSel, setMethodSel] = useState("");
  const [discovering, setDiscovering] = useState(false);
  const [reflectErr, setReflectErr] = useState("");
  const { t, format } = useT();

  const discover = async () => {
    if (!connectUrl.trim()) {
      setReflectErr(t("grpc.fillConnectUrl"));
      return;
    }
    setDiscovering(true);
    setReflectErr("");
    try {
      const svcs = await grpcReflect(connectUrl);
      setServices(svcs);
      if (svcs.length > 0) {
        setSvcSel(svcs[0].name);
        setMethodSel(svcs[0].methods[0]?.name ?? "");
      }
    } catch (e) {
      setReflectErr(e instanceof Error ? e.message : String(e));
      setServices([]);
    } finally {
      setDiscovering(false);
    }
  };

  const selectedSvc = services?.find((s) => s.name === svcSel);
  const methodInfo = selectedSvc?.methods.find((m) => m.name === methodSel);

  return (
    <>
      {/* Service discovery (Server Reflection) */}
      <Field label={t("grpc.discovery")}>
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <Input
              className="h-8 flex-1 text-xs font-mono"
              placeholder="grpc://host:port"
              value={connectUrl}
              readOnly
            />
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={() => void discover()}
              disabled={discovering}
            >
              <Search className="h-3.5 w-3.5" />
              {discovering ? t("grpc.discovering") : t("grpc.discover")}
            </Button>
          </div>
          {reflectErr && (
            <div className="text-xs text-rose-400">✗ {reflectErr}</div>
          )}
          {services && services.length > 0 && (
            <div className="flex items-center gap-1.5">
              <Select
                value={svcSel}
                onValueChange={(v) => {
                  setSvcSel(v);
                  setMethodSel("");
                }}
              >
                <SelectTrigger className="h-8 flex-1 text-xs">
                  <SelectValue placeholder={t("grpc.selectService")} />
                </SelectTrigger>
                <SelectContent>
                  {services.map((s) => (
                    <SelectItem key={s.name} value={s.name} className="text-xs">
                      {s.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                value={methodSel}
                onValueChange={(v) => {
                  setMethodSel(v);
                  const m = selectedSvc?.methods.find((x) => x.name === v);
                  if (m) {
                    set({ service: `${svcSel}/${m.name}` });
                    if (!req.message) set({ message: "{}" });
                  }
                }}
              >
                <SelectTrigger className="h-8 flex-1 text-xs">
                  <SelectValue placeholder={t("grpc.selectMethod")} />
                </SelectTrigger>
                <SelectContent>
                  {(selectedSvc?.methods ?? []).map((m) => (
                    <SelectItem key={m.name} value={m.name} className="text-xs">
                      {m.name}
                      {m.serverStreaming || m.clientStreaming
                        ? " (stream)"
                        : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                variant="ghost"
                size="icon-sm"
                className="h-8 w-8 shrink-0"
                title={t("grpc.rediscover")}
                onClick={() => void discover()}
              >
                <RefreshCw className="h-3.5 w-3.5" />
              </Button>
            </div>
          )}
          {methodInfo && (
            <div className="text-xs text-muted-foreground">
              {format(
                "grpc.ioTypes",
                methodInfo.inputType,
                methodInfo.outputType,
              )}
              {methodInfo.serverStreaming && ` · ${t("grpc.serverStream")}`}
              {methodInfo.clientStreaming && ` · ${t("grpc.clientStream")}`}
            </div>
          )}
        </div>
      </Field>

      <Field label={t("grpc.serviceMethod")}>
        <Input
          className="h-8 text-xs font-mono"
          value={req.service}
          onChange={(e) => set({ service: e.target.value })}
          placeholder="/greet.Greeter/SayHello"
        />
      </Field>
      <Field label={t("grpc.requestMsg")}>
        <Textarea
          className="h-20 text-xs font-mono"
          value={req.message ?? ""}
          onChange={(e) => set({ message: e.target.value })}
        />
      </Field>
      <Field label="Metadata (Headers)">
        <KeyValueEditor
          items={req.headers}
          enableDynamic
          onChange={(v) => set({ headers: v })}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label={t("grpc.msgFormat")}>
          <Select
            value={req.messageFormat ?? "json"}
            onValueChange={(v) => set({ messageFormat: v })}
          >
            <SelectTrigger className="h-8 w-32 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="json" className="text-xs">
                json
              </SelectItem>
              <SelectItem value="protobuf" className="text-xs">
                protobuf
              </SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <Field label={t("grpc.streamMode")}>
          <Select
            value={req.streaming ?? "unary"}
            onValueChange={(v) =>
              set({
                streaming:
                  v === "unary" ? undefined : (v as GrpcRequest["streaming"]),
              })
            }
          >
            <SelectTrigger className="h-8 w-40 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="unary" className="text-xs">
                unary
              </SelectItem>
              {STREAM_MODES.map((m) => (
                <SelectItem key={m} value={m} className="text-xs">
                  {m}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>
    </>
  );
}
