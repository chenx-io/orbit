// GraphQL panel: Headers + Query + Variables + Operation Name.
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { Field } from "./shared";
import type { GraphqlRequest } from "@/data/types";

export function GraphqlPanel({
  req,
  set,
}: {
  req: GraphqlRequest;
  set: (patch: Record<string, unknown>) => void;
}) {
  return (
    <>
      <Field label="Headers">
        <KeyValueEditor
          items={req.headers}
          enableDynamic
          onChange={(v) => set({ headers: v })}
        />
      </Field>
      <Field label="Query">
        <Textarea
          className="h-24 text-xs font-mono"
          value={req.query ?? ""}
          onChange={(e) => set({ query: e.target.value })}
        />
      </Field>
      <Field label="Variables (JSON)">
        <Textarea
          className="h-16 text-xs font-mono"
          value={req.variables ?? ""}
          onChange={(e) => set({ variables: e.target.value })}
        />
      </Field>
      <Field label="Operation Name">
        <Input
          className="h-8 text-xs"
          value={req.operationName ?? ""}
          onChange={(e) => set({ operationName: e.target.value })}
        />
      </Field>
    </>
  );
}
