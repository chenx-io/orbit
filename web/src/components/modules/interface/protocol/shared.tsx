// Shared components for the protocol editors (used by every protocol panel).
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { uid } from "@/data/seed";
import { useT } from "@/lib/i18n";
import type { PayloadType, WsFrameType, WsMessageSpec } from "@/data/types";
import { PAYLOAD_TYPES } from "./constants";

export function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      {children}
    </div>
  );
}

/** Long-connection message-sequence editor (WS/TCP/UDP) */
export function MessageListEditor({
  messages,
  onChange,
  showFrameType,
}: {
  messages: WsMessageSpec[];
  onChange: (m: WsMessageSpec[]) => void;
  showFrameType?: boolean;
}) {
  const { t, format } = useT();
  const patch = (id: string, p: Partial<WsMessageSpec>) =>
    onChange(messages.map((m) => (m.id === id ? { ...m, ...p } : m)));
  return (
    <div className="space-y-2">
      {messages.map((m, i) => (
        <div
          key={m.id}
          className="space-y-1.5 rounded-md border border-border p-2"
        >
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-muted-foreground">
              {format("proto.messageN", i + 1)}
            </span>
            <Button
              variant="ghost"
              size="icon-sm"
              className="h-5 w-5"
              onClick={() => onChange(messages.filter((x) => x.id !== m.id))}
            >
              <Trash2 className="h-3 w-3 text-rose-400" />
            </Button>
          </div>
          <div className="flex items-center gap-1.5">
            <Select
              value={m.payloadType ?? "text"}
              onValueChange={(v) =>
                patch(m.id, { payloadType: v as PayloadType })
              }
            >
              <SelectTrigger className="h-7 w-24 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PAYLOAD_TYPES.map((p) => (
                  <SelectItem key={p} value={p} className="text-xs">
                    {p}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {showFrameType && (
              <Select
                value={m.messageType ?? "text"}
                onValueChange={(v) =>
                  patch(m.id, { messageType: v as WsFrameType })
                }
              >
                <SelectTrigger className="h-7 w-20 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="text" className="text-xs">
                    text
                  </SelectItem>
                  <SelectItem value="binary" className="text-xs">
                    binary
                  </SelectItem>
                </SelectContent>
              </Select>
            )}
            <Input
              className="h-7 flex-1 text-xs"
              placeholder="payload"
              value={m.payload}
              onChange={(e) => patch(m.id, { payload: e.target.value })}
            />
            <Input
              className="h-7 w-20 text-xs"
              placeholder="wait ms"
              type="number"
              value={m.waitMs ?? 0}
              onChange={(e) =>
                patch(m.id, { waitMs: parseInt(e.target.value) || 0 })
              }
            />
          </div>
          <Textarea
            className="h-14 text-xs"
            placeholder={t("proto.preScriptPlaceholder")}
            value={m.preScript ?? ""}
            onChange={(e) => patch(m.id, { preScript: e.target.value })}
          />
          <Textarea
            className="h-14 text-xs"
            placeholder={t("proto.postScriptPlaceholder")}
            value={m.postScript ?? ""}
            onChange={(e) => patch(m.id, { postScript: e.target.value })}
          />
        </div>
      ))}
      <Button
        variant="outline"
        size="sm"
        className="w-full gap-1 text-xs"
        onClick={() =>
          onChange([
            ...messages,
            {
              id: uid("msg"),
              payload: "",
              payloadType: "text",
              preScript: "",
              postScript: "",
              waitMs: 0,
            },
          ])
        }
      >
        <Plus className="h-3.5 w-3.5" /> {t("proto.addMessage")}
      </Button>
    </div>
  );
}
