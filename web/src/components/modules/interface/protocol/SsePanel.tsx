// SSE panel: Headers + max events to collect.
import { Input } from "@/components/ui/input";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { Field } from "./shared";
import { useT } from "@/lib/i18n";
import type { SseRequest } from "@/data/types";

export function SsePanel({
  req,
  set,
}: {
  req: SseRequest;
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  return (
    <>
      <Field label="Headers">
        <KeyValueEditor
          items={req.headers}
          enableDynamic
          onChange={(v) => set({ headers: v })}
        />
      </Field>
      <Field label={t("sse.maxEvents")}>
        <Input
          className="h-8 w-24 text-xs"
          type="number"
          value={req.maxEvents ?? 50}
          onChange={(e) => set({ maxEvents: parseInt(e.target.value) || 50 })}
        />
      </Field>
    </>
  );
}
