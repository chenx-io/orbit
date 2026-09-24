// TCP / UDP panel: framing + initial payload + message sequence.
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Field, MessageListEditor } from "./shared";
import { FRAMING_MODES, PAYLOAD_TYPES } from "./constants";
import { useT } from "@/lib/i18n";
import type { PayloadType, TcpFramingMode, UdpRequest } from "@/data/types";

export function TcpPanel({
  req,
  set,
}: {
  req: Extract<import("@/data/types").ApiRequest, { protocol: "tcp" }>;
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  return (
    <>
      <Field label={t("tcp.framing")}>
        <div className="flex items-center gap-1.5">
          <Select
            value={req.framing?.mode ?? "read_until_close"}
            onValueChange={(v) =>
              set({
                framing: { ...(req.framing ?? {}), mode: v as TcpFramingMode },
              })
            }
          >
            <SelectTrigger className="h-8 w-40 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FRAMING_MODES.map((m) => (
                <SelectItem key={m} value={m} className="text-xs">
                  {m}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {req.framing?.mode === "delimiter" && (
            <Input
              className="h-8 w-32 text-xs"
              placeholder={t("tcp.delimiter")}
              value={req.framing?.delimiter ?? "\\n"}
              onChange={(e) =>
                set({ framing: { ...req.framing, delimiter: e.target.value } })
              }
            />
          )}
          {(req.framing?.mode === "fixed" ||
            req.framing?.mode === "length_prefix") && (
            <Input
              className="h-8 w-24 text-xs"
              type="number"
              placeholder={t("tcp.lenBytes")}
              value={req.framing?.fixedLen ?? 4}
              onChange={(e) =>
                set({
                  framing: {
                    ...req.framing,
                    fixedLen: parseInt(e.target.value) || 4,
                  },
                })
              }
            />
          )}
        </div>
      </Field>
      <PayloadField req={req} set={set} />
      <Field label={t("proto.messageSequence")}>
        <MessageListEditor
          messages={req.messages ?? []}
          onChange={(m) => set({ messages: m })}
        />
      </Field>
    </>
  );
}

export function UdpPanel({
  req,
  set,
}: {
  req: UdpRequest;
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  return (
    <>
      <PayloadField req={req} set={set} />
      <Field label={t("proto.messageSequence")}>
        <MessageListEditor
          messages={req.messages ?? []}
          onChange={(m) => set({ messages: m })}
        />
      </Field>
    </>
  );
}

/** Initial payload (payload_type + payload) — shared by TCP/UDP */
function PayloadField({
  req,
  set,
}: {
  req: { payload?: string; payloadType?: PayloadType };
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  return (
    <Field label={t("tcp.payload")}>
      <div className="flex items-center gap-1.5">
        <Select
          value={req.payloadType ?? "text"}
          onValueChange={(v) => set({ payloadType: v as PayloadType })}
        >
          <SelectTrigger className="h-8 w-24 text-xs">
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
        <Input
          className="h-8 flex-1 text-xs"
          placeholder="payload"
          value={req.payload ?? ""}
          onChange={(e) => set({ payload: e.target.value })}
        />
      </div>
    </Field>
  );
}
