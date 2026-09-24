// Generic panel for plugin protocols (native/wasm, dynamic) — single-shot request model.
// - Connection params are defined by the plugin manifest's connectionConfigSchema and filled in when creating the collection (stored on the collection's connection); here they are shown as read-only hints.
// - Message params (requestConfigSchema) are configured as request params and rendered dynamically via SchemaForm.
// - Single-shot: type SQL/request content in the send box above, click "Send" to auto-connect and run one query; results appear in the response panel.

import { Info } from "lucide-react";
import { KeyValueEditor } from "@/components/common/KeyValueEditor";
import { SchemaForm, type SchemaNode } from "@/components/common/SchemaForm";
import { Field } from "./shared";
import { useT } from "@/lib/i18n";
import { useAppStore } from "@/store/useStore";
import type { PluginRequest } from "@/data/types";

export function PluginPanel({
  req,
  set,
}: {
  req: PluginRequest;
  set: (patch: Record<string, unknown>) => void;
}) {
  const { t } = useT();
  const protocolCatalog = useAppStore((s) => s.protocolCatalog);
  // The dynamic form schema for the current plugin
  const entry = protocolCatalog.find((p) => p.id === req.protocol);
  const reqSchema =
    (entry?.requestConfigSchema as SchemaNode | undefined) ?? null;

  const options = req.options ?? {};

  return (
    <>
      <Field label={t("plugins.headers")}>
        <KeyValueEditor
          items={req.headers}
          enableDynamic
          onChange={(v) => set({ headers: v })}
        />
      </Field>

      {/* Connection params: filled in when creating the collection; read-only hints here */}
      <div className="flex items-start gap-1.5 rounded-md border border-dashed border-border bg-muted/30 px-2.5 py-2 text-xs text-muted-foreground">
        <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground/70" />
        <span>{t("plugins.connInCollection")}</span>
      </div>

      {/* Message params: requestConfigSchema dynamic form (e.g. SQL) */}
      {reqSchema && reqSchema.properties ? (
        <Field label={t("plugins.requestArgs")}>
          <div className="rounded-md border border-border bg-muted/30 p-2">
            <SchemaForm
              schema={reqSchema}
              value={options as Record<string, unknown>}
              onChange={(v) => set({ options: v })}
            />
          </div>
        </Field>
      ) : (
        <Field label={t("plugins.requestArgs")}>
          <div className="rounded-md border border-dashed border-border bg-muted/30 px-2.5 py-2 text-xs text-muted-foreground">
            {t("plugins.sendInComposer")}
          </div>
        </Field>
      )}
    </>
  );
}
