// WebSocket message editor panel: handshake Headers + message sequence (Saved Messages semantics).
import { Input } from "@/components/ui/input";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { Field, MessageListEditor } from "./shared";
import { useT } from "@/lib/i18n";
import type { WsRequest } from "@/data/types";

export function WebSocketPanel({
  req,
  set,
}: {
  req: WsRequest;
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  return (
    <>
      <Field label={t("ws.handshakeHeaders")}>
        <KeyValueEditor
          items={req.headers}
          enableDynamic
          onChange={(v) => set({ headers: v })}
        />
      </Field>
      <Field label={t("ws.closeAfter")}>
        <Input
          className="h-8 w-24 text-xs"
          type="number"
          value={req.closeAfter ?? 0}
          onChange={(e) => set({ closeAfter: parseInt(e.target.value) || 0 })}
        />
      </Field>
      <Field label={t("proto.messageSequence")}>
        <MessageListEditor
          messages={req.messages}
          onChange={(m) => set({ messages: m })}
          showFrameType
        />
      </Field>
    </>
  );
}
