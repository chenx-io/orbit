// Data-model view dialog: shows the schema as a read-only field list (shared by request-body model / response examples).
import { Braces } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

/** Recursively render schema fields (same structure as ModelEditor's field rows, read-only): field name / type / required / example / description */
export function ResponseSchemaFields({
  schema,
  depth,
  t,
}: {
  schema: any;
  depth: number;
  t: (k: string) => string;
}) {
  // Depth guard: prevent abnormal schemas (e.g. extreme nesting / circular refs) from recursing too deep and blanking the screen
  if (depth > 12) return null;
  if (!schema || typeof schema !== "object") return null;
  const props = schema.properties;
  if (!props || typeof props !== "object") return null;
  const entries = Object.entries(props) as [string, any][];
  if (entries.length === 0) return null;
  return (
    <div className="text-xs">
      {/* Table header */}
      <div className="flex items-center gap-1.5 border-b border-border px-2 py-1.5 text-xs font-medium text-muted-foreground">
        <span className="w-32 shrink-0">{t("model.fieldName")}</span>
        <span className="w-24 shrink-0">{t("model.type")}</span>
        <span className="w-10 shrink-0">{t("model.required")}</span>
        <span className="w-36 shrink-0">{t("model.example")}</span>
        <span className="min-w-0 flex-1">{t("model.fieldDescription")}</span>
      </div>
      {entries.map(([name, field]) => {
        const type = field?.type ?? "string";
        const required = field?.required === true;
        const rawExample = field?.example ?? field?.default ?? "";
        // Example values may be objects/arrays (e.g. nested message `{}`); React cannot render objects directly, so serialize to a string
        const example =
          rawExample && typeof rawExample === "object"
            ? JSON.stringify(rawExample)
            : rawExample;
        const desc = field?.description ?? "";
        // Nested content: the backend message_schema structure is
        //   singular message → field.properties = { fieldName: schema }
        //   repeated        → field.items = { fieldName: schema } (element field map)
        //   map value       → field.additionalProperties = { fieldName: schema } (value field map)
        // Normalize all of these into { properties: ... } for the next recursion level.
        let childProps = null;
        if (
          field?.properties &&
          typeof field.properties === "object" &&
          !Array.isArray(field.properties)
        ) {
          childProps = field.properties;
        } else if (
          field?.items &&
          typeof field.items === "object" &&
          !Array.isArray(field.items)
        ) {
          childProps = field.items;
        } else if (
          field?.additionalProperties &&
          typeof field.additionalProperties === "object" &&
          !Array.isArray(field.additionalProperties)
        ) {
          childProps = field.additionalProperties;
        }
        const childSchema = childProps ? { properties: childProps } : null;
        return (
          <div key={name}>
            <div
              className="flex items-center gap-1.5 border-b border-border/40 px-2 py-1.5"
              style={{ marginLeft: depth * 16 }}
            >
              <span className="w-32 shrink-0 truncate font-medium text-primary">
                {name}
              </span>
              <span className="w-24 shrink-0 truncate text-muted-foreground">
                {type}
                {childSchema ? " ▾" : ""}
              </span>
              <span className="w-10 shrink-0">
                {required && (
                  <span className="text-xs text-amber-400">
                    {t("model.required")}
                  </span>
                )}
              </span>
              <span className="w-36 shrink-0 truncate font-mono text-muted-foreground">
                {example}
              </span>
              <span className="min-w-0 flex-1 truncate text-muted-foreground">
                {desc}
              </span>
            </div>
            {childSchema && (
              <ResponseSchemaFields
                schema={childSchema}
                depth={depth + 1}
                t={t}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

export function ResponseSchemaDialog({
  title,
  schema,
  open,
  onOpenChange,
  t,
}: {
  title: string;
  schema: any;
  open: boolean;
  onOpenChange: (v: boolean) => void;
  t: (k: string) => string;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-sm">
            <Braces className="h-4 w-4 text-primary" />
            {title}
            <span className="text-xs font-normal text-muted-foreground">
              {t("request.viewSchema")}
            </span>
          </DialogTitle>
        </DialogHeader>
        <div className="max-h-[70vh] overflow-auto">
          <ResponseSchemaFields schema={schema} depth={0} t={t} />
        </div>
      </DialogContent>
    </Dialog>
  );
}
