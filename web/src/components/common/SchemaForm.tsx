// SchemaForm: a lightweight JSON Schema → shadcn/ui dynamic form.
// Supports string (enum→Select, format=textarea for multi-line) / number / integer / boolean / array / object;
// each field reads title/description/default/required, with the value two-way bound to the parent state (controlled).
// Used to render plugin protocols' connectionConfigSchema / requestConfigSchema dynamically.

import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

/** The subset of JSON Schema that is allowed */
export interface SchemaNode {
  type?: string;
  title?: string;
  description?: string;
  default?: unknown;
  enum?: unknown[];
  format?: string;
  properties?: Record<string, SchemaNode>;
  items?: SchemaNode;
  required?: string[];
}

type SchemaValue =
  | string
  | number
  | boolean
  | Record<string, unknown>
  | unknown[]
  | null
  | undefined;

export interface SchemaFormProps {
  /** Root schema (type: object, with properties) */
  schema?: SchemaNode | null;
  /** Current value (an object) */
  value: Record<string, unknown>;
  /** Value change callback */
  onChange: (v: Record<string, unknown>) => void;
  /** Field label prefix (optional) */
  titlePrefix?: string;
  /** Whether to render required fields only (optional; used for connection parameters) */
  requiredOnly?: boolean;
}

function scalarToString(v: SchemaValue): string {
  if (v === null || v === undefined) return "";
  return String(v);
}

function stringToScalar(type: string, s: string): SchemaValue {
  if (s === "") return undefined;
  if (type === "number" || type === "integer") {
    const n = type === "integer" ? Math.trunc(Number(s)) : Number(s);
    return Number.isFinite(n) ? n : s;
  }
  return s;
}

function renderField(
  key: string,
  node: SchemaNode,
  current: SchemaValue,
  set: (v: SchemaValue) => void,
  required: boolean,
) {
  const label = node.title ?? key;
  const desc = node.description;
  const isRequired = required;

  const labelEl = (
    <Label className="text-xs text-muted-foreground">
      {label}
      {isRequired && <span className="ml-1 text-destructive">*</span>}
    </Label>
  );

  // ── boolean → Switch
  if (node.type === "boolean") {
    return (
      <div key={key} className="space-y-1">
        <div className="flex items-center justify-between">
          <div>
            {labelEl}
            {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
          </div>
          <Switch checked={!!current} onCheckedChange={(v) => set(v)} />
        </div>
      </div>
    );
  }

  // ── enum → Select
  if (Array.isArray(node.enum) && node.enum.length > 0) {
    const cur =
      current === undefined || current === null ? "" : String(current);
    return (
      <div key={key} className="space-y-1">
        {labelEl}
        <Select value={cur} onValueChange={(v) => set(v)}>
          <SelectTrigger className="h-8 text-xs">
            <SelectValue placeholder={desc ?? label} />
          </SelectTrigger>
          <SelectContent>
            {node.enum.map((e) => (
              <SelectItem key={String(e)} value={String(e)}>
                {String(e)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
      </div>
    );
  }

  // ── array of string → a comma-separated inline input (simplified: newline-separated)
  if (node.type === "array") {
    const items = Array.isArray(current) ? current : [];
    const text = items.map((v) => scalarToString(v as SchemaValue)).join("\n");
    return (
      <div key={key} className="space-y-1">
        {labelEl}
        <Textarea
          className="h-16 text-xs font-mono"
          value={text}
          placeholder={desc ?? label}
          onChange={(e) => {
            const lines = e.target.value
              .split("\n")
              .map((s) => s.trim())
              .filter(Boolean);
            set(lines);
          }}
        />
        {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
      </div>
    );
  }

  // ── textarea（format=textarea / multiline）
  if (node.type === "string" && node.format === "textarea") {
    return (
      <div key={key} className="space-y-1">
        {labelEl}
        <Textarea
          className="h-20 text-xs font-mono"
          value={scalarToString(current)}
          placeholder={desc ?? label}
          onChange={(e) => set(e.target.value)}
        />
        {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
      </div>
    );
  }

  // ── number / integer
  if (node.type === "number" || node.type === "integer") {
    const raw =
      current === undefined || current === null ? "" : String(current);
    return (
      <div key={key} className="space-y-1">
        {labelEl}
        <Input
          type="number"
          className="h-8 text-xs"
          value={raw}
          placeholder={desc ?? label}
          onChange={(e) =>
            set(stringToScalar(node.type ?? "string", e.target.value))
          }
        />
        {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
      </div>
    );
  }

  // ── object → recursively render child properties
  if (node.type === "object" && node.properties) {
    const obj = (
      current && typeof current === "object" && !Array.isArray(current)
        ? current
        : {}
    ) as Record<string, unknown>;
    const requiredSet = new Set(node.required ?? []);
    return (
      <div key={key} className="space-y-1.5">
        {labelEl}
        <div className="space-y-1.5 rounded-md border border-border/60 bg-background/40 p-2">
          {Object.entries(node.properties).map(([k, sub]) =>
            renderField(
              k,
              sub,
              obj[k] as SchemaValue,
              (v) => {
                const next = { ...obj };
                if (v === undefined || v === null || v === "") delete next[k];
                else next[k] = v;
                set(next);
              },
              requiredSet.has(k),
            ),
          )}
        </div>
      </div>
    );
  }

  // ── default: a single-line string
  return (
    <div key={key} className="space-y-1">
      {labelEl}
      <Input
        className="h-8 text-xs"
        value={scalarToString(current)}
        placeholder={desc ?? label}
        onChange={(e) => set(e.target.value)}
      />
      {desc && <p className="text-xs text-muted-foreground/70">{desc}</p>}
    </div>
  );
}

export function SchemaForm({
  schema,
  value,
  onChange,
  titlePrefix,
  requiredOnly,
}: SchemaFormProps) {
  if (!schema || schema.type !== "object" || !schema.properties) {
    return null;
  }
  const requiredSet = new Set(schema.required ?? []);
  const entries = Object.entries(schema.properties);
  const visibleEntries = requiredOnly
    ? entries.filter(([k]) => requiredSet.has(k))
    : entries;

  return (
    <div className="space-y-2">
      {titlePrefix && (
        <div className="text-xs font-medium text-foreground">{titlePrefix}</div>
      )}
      {visibleEntries.map(([key, node]) => {
        const cur = value[key] as SchemaValue;
        // Default to schema.default
        const effectiveCur =
          cur === undefined && node.default !== undefined ? node.default : cur;
        return renderField(
          key,
          node,
          effectiveCur as SchemaValue,
          (v) => {
            const next = { ...value };
            if (v === undefined || v === null || v === "") delete next[key];
            else next[key] = v;
            onChange(next);
          },
          requiredSet.has(key),
        );
      })}
    </div>
  );
}
