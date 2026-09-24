import { useMemo, useState } from "react";
import { Plus, Trash2, Braces } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { DynamicValuePicker } from "./DynamicValuePicker";
import { useT } from "@/lib/i18n";
import type { KeyValue } from "@/data/types";
import { uid } from "@/data/seed";

export interface CommonKey {
  key: string;
  value?: string;
}

interface Props {
  items: KeyValue[];
  onChange: (items: KeyValue[]) => void;
  enableDynamic?: boolean;
  /** Offer common key suggestions for the key input (e.g. request headers). Selecting one fills both the key and a common value. */
  commonKeys?: CommonKey[];
}

export function KeyValueEditor({
  items,
  onChange,
  enableDynamic,
  commonKeys,
}: Props) {
  const { t } = useT();

  const update = (id: string, patch: Partial<KeyValue>) =>
    onChange(items.map((it) => (it.id === id ? { ...it, ...patch } : it)));
  const setBoth = (id: string, key: string, value: string) =>
    onChange(items.map((it) => (it.id === id ? { ...it, key, value } : it)));
  const remove = (id: string) => onChange(items.filter((it) => it.id !== id));
  const add = () =>
    onChange([...items, { id: uid("kv"), key: "", value: "", enabled: true }]);

  const insertDynamic = (id: string, token: string) => {
    const it = items.find((x) => x.id === id);
    if (!it) return;
    update(id, {
      value: it.value ? `${it.value}{{${token}}}` : `{{${token}}}`,
    });
  };

  return (
    <div className="space-y-1.5">
      {items.map((it) => (
        <div key={it.id} className="flex items-center gap-1.5">
          <Checkbox
            checked={it.enabled}
            onCheckedChange={(c) => update(it.id, { enabled: !!c })}
          />
          <KeyCell
            value={it.key}
            onChange={(v) => update(it.id, { key: v })}
            onBoth={(k, v) => setBoth(it.id, k, v)}
            commonKeys={commonKeys}
          />
          <div className="flex flex-1 items-center gap-1">
            <Input
              value={it.value}
              placeholder={t("kv.valuePlaceholder") as string}
              className="h-8 flex-1 font-mono text-xs"
              onChange={(e) => update(it.id, { value: e.target.value })}
            />
            {enableDynamic && (
              <DynamicValuePicker
                onInsert={(token) => insertDynamic(it.id, token)}
              />
            )}
          </div>
          <Button variant="ghost" size="icon-sm" onClick={() => remove(it.id)}>
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        className="text-muted-foreground"
        onClick={add}
      >
        <Plus className="h-3.5 w-3.5" /> {t("kv.add")}
      </Button>
    </div>
  );
}

function KeyCell({
  value,
  onChange,
  onBoth,
  commonKeys,
}: {
  value: string;
  onChange: (v: string) => void;
  onBoth: (key: string, value: string) => void;
  commonKeys?: CommonKey[];
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    if (!commonKeys) return [];
    const q = query.toLowerCase();
    return commonKeys.filter((c) => c.key.toLowerCase().includes(q));
  }, [commonKeys, query]);

  return (
    <div className="relative flex-1">
      <Input
        value={value}
        placeholder="Key"
        className="h-8 w-full font-mono text-xs"
        onChange={(e) => {
          onChange(e.target.value);
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => {
          setQuery(value);
          setOpen(true);
        }}
        onBlur={() => window.setTimeout(() => setOpen(false), 150)}
      />
      {open && commonKeys && filtered.length > 0 && (
        <div className="absolute z-50 mt-1 max-h-56 w-full overflow-auto rounded-md border border-border bg-popover p-1 shadow-md">
          {filtered.map((c) => (
            <button
              key={c.key}
              type="button"
              onMouseDown={(e) => {
                e.preventDefault();
                onBoth(c.key, c.value ?? "");
                setOpen(false);
              }}
              className="flex w-full flex-col items-start rounded px-2 py-1 text-left hover:bg-accent/15"
            >
              <span className="font-mono text-xs text-foreground">{c.key}</span>
              {c.value && (
                <span className="truncate font-mono text-xs text-muted-foreground">
                  {c.value}
                </span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function EmptyHint() {
  const { t } = useT();
  return (
    <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <Braces className="h-3.5 w-3.5" /> {t("common.emptyData")}
    </div>
  );
}
