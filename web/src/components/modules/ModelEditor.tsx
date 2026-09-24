import { Plus, Trash2, Boxes } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import type { SchemaField, SchemaFieldType } from "@/data/types";
import { uid } from "@/data/seed";

const TYPES: SchemaFieldType[] = [
  "string",
  "integer",
  "number",
  "boolean",
  "object",
  "array",
  "null",
];

function newField(name = "field"): SchemaField {
  return { id: uid("f"), name, type: "string", required: false };
}

function FieldRow({
  field,
  onChange,
  onDelete,
  depth,
  t,
}: {
  field: SchemaField;
  onChange: (f: SchemaField) => void;
  onDelete: () => void;
  depth: number;
  t: (k: string) => string;
}) {
  const children = field.children ?? [];
  const setChild = (id: string, f: SchemaField) =>
    onChange({
      ...field,
      children: children.map((c) => (c.id === id ? f : c)),
    });
  const addChild = () =>
    onChange({ ...field, children: [...children, newField("item")] });
  const removeChild = (id: string) =>
    onChange({ ...field, children: children.filter((c) => c.id !== id) });

  return (
    <div style={{ marginLeft: depth * 16 }}>
      <div className="flex items-center gap-1.5 py-1">
        <Input
          value={field.name}
          onChange={(e) => onChange({ ...field, name: e.target.value })}
          className="h-7 w-32 text-xs"
          placeholder={t("model.fieldName")}
        />
        <Select
          value={field.type}
          onValueChange={(v) =>
            onChange({ ...field, type: v as SchemaFieldType })
          }
        >
          <SelectTrigger className="h-7 w-24">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {TYPES.map((ty) => (
              <SelectItem key={ty} value={ty}>
                {ty}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Checkbox
          checked={field.required}
          onCheckedChange={(c) => onChange({ ...field, required: !!c })}
        />
        <span className="text-xs text-muted-foreground">
          {t("model.required")}
        </span>
        <Input
          value={field.example ?? ""}
          onChange={(e) => onChange({ ...field, example: e.target.value })}
          className="h-7 w-36 shrink-0 text-xs"
          placeholder={t("model.example")}
        />
        <Input
          value={field.description ?? ""}
          onChange={(e) => onChange({ ...field, description: e.target.value })}
          className="h-7 min-w-0 flex-1 text-xs"
          placeholder={t("model.fieldDescription")}
        />
        {(field.type === "object" || field.type === "array") && (
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={addChild}
            title={t("model.addChild")}
          >
            <Plus className="h-3.5 w-3.5" />
          </Button>
        )}
        <Button variant="ghost" size="icon-sm" onClick={onDelete}>
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
      {children.map((c) => (
        <FieldRow
          key={c.id}
          field={c}
          depth={depth + 1}
          onChange={(f) => setChild(c.id, f)}
          onDelete={() => removeChild(c.id)}
          t={t}
        />
      ))}
    </div>
  );
}

export function ModelEditor() {
  const { t } = useT();
  const activeModelId = useAppStore((s) => s.activeModelId);
  const models = useAppStore((s) => s.models);
  const updateModel = useAppStore((s) => s.updateModel);
  const removeModel = useAppStore((s) => s.removeModel);

  const model = models.find((m) => m.id === activeModelId) ?? null;

  if (!model) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
        <Boxes className="h-8 w-8 opacity-50" />
        <div>{t("model.emptyHint")}</div>
      </div>
    );
  }

  const setFields = (fields: SchemaField[]) =>
    updateModel(model.id, { fields });
  const addField = () => setFields([...model.fields, newField()]);
  const updateField = (id: string, f: SchemaField) =>
    setFields(model.fields.map((x) => (x.id === id ? f : x)));
  const removeField = (id: string) =>
    setFields(model.fields.filter((x) => x.id !== id));

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-border px-4 py-2">
        <div className="flex items-center gap-2">
          <Boxes className="h-4 w-4 text-primary" />
          <span className="text-sm font-semibold">{t("model.title")}</span>
          <span className="text-xs text-muted-foreground">
            （{model.fields.length} {t("model.fieldsCount")}）
          </span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="text-destructive"
          onClick={() => removeModel(model.id)}
        >
          <Trash2 className="h-3.5 w-3.5" /> {t("model.delete")}
        </Button>
      </div>
      <ScrollArea className="flex-1">
        <div className="p-4">
          <Label className="text-xs">{t("model.name")}</Label>
          <Input
            value={model.name}
            onChange={(e) => updateModel(model.id, { name: e.target.value })}
            className="mt-1 h-8"
            placeholder={t("model.namePlaceholder")}
          />
          <Label className="mt-3 block text-xs">{t("model.description")}</Label>
          <Input
            value={model.description ?? ""}
            onChange={(e) =>
              updateModel(model.id, { description: e.target.value })
            }
            className="mt-1 h-8"
            placeholder={t("model.descriptionPlaceholder")}
          />
          <div className="mb-2 mt-4 flex items-center justify-between">
            <span className="text-sm font-medium">{t("model.fields")}</span>
            <Button variant="outline" size="sm" onClick={addField}>
              <Plus className="h-3.5 w-3.5" /> {t("model.addField")}
            </Button>
          </div>
          <div className="pr-2">
            {model.fields.map((f) => (
              <FieldRow
                key={f.id}
                field={f}
                depth={0}
                onChange={(nf) => updateField(f.id, nf)}
                onDelete={() => removeField(f.id)}
                t={t}
              />
            ))}
            {model.fields.length === 0 && (
              <div className="py-4 text-center text-xs text-muted-foreground">
                {t("model.noFields")}
              </div>
            )}
          </div>
        </div>
      </ScrollArea>
    </div>
  );
}
