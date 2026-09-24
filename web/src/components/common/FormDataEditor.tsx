import { useRef } from "react";
import { Plus, Trash2, Braces, FileUp, Paperclip, X, Type } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { DynamicValuePicker } from "./DynamicValuePicker";
import { useT } from "@/lib/i18n";
import type { KeyValue } from "@/data/types";
import { uid } from "@/data/seed";
import { readFileAsBase64 } from "@/lib/requestBody";
import { isTauri } from "@/lib/bridge";
import { pickTauriFile } from "@/lib/filePicker";

interface Props {
  items: KeyValue[];
  onChange: (items: KeyValue[]) => void;
  /** form-data supports file uploads; urlencoded is text only */
  mode: "form-data" | "urlencoded";
  enableDynamic?: boolean;
}

export function FormDataEditor({
  items,
  onChange,
  mode,
  enableDynamic,
}: Props) {
  const { t } = useT();

  const update = (id: string, patch: Partial<KeyValue>) =>
    onChange(items.map((it) => (it.id === id ? { ...it, ...patch } : it)));
  const remove = (id: string) => onChange(items.filter((it) => it.id !== id));
  const add = () =>
    onChange([
      ...items,
      { id: uid("kv"), key: "", value: "", enabled: true, type: "text" },
    ]);

  // A text row switches back to text mode
  const toText = (id: string) =>
    update(id, { type: "text", file: null, value: "" });

  const insertDynamic = (id: string, token: string) => {
    const it = items.find((x) => x.id === id);
    if (!it) return;
    update(id, {
      value: it.value ? `${it.value}{{${token}}}` : `{{${token}}}`,
    });
  };

  const fileInputRefs = useRef<Record<string, HTMLInputElement | null>>({});

  // Picking a file: Tauri uses the native dialog for the real path (no content pre-read); the browser reads base64 via <input>.
  // Both enter file mode (type:"file"); the only difference is file.path (Tauri) versus file.data (Web).
  const pickFileForRow = async (id: string) => {
    if (isTauri()) {
      const picked = await pickTauriFile();
      if (picked) {
        update(id, {
          type: "file",
          file: { name: picked.name, type: picked.type, path: picked.path },
          value: "",
        });
      }
      return;
    }
    fileInputRefs.current[id]?.click();
  };

  const onFilePicked = async (id: string, file: File | undefined) => {
    if (!file) return;
    const f = await readFileAsBase64(file);
    // Once a file is chosen it enters file mode (type:"file"), showing the name/size and offering a re-pick
    update(id, { type: "file", file: f, value: "" });
  };

  return (
    <div className="space-y-1.5">
      {items.length === 0 && (
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Braces className="h-3.5 w-3.5" /> {t("body.noFormParams")}
        </div>
      )}
      {items.map((it) => {
        const isFile = mode === "form-data" && it.type === "file";
        return (
          <div key={it.id} className="flex items-center gap-1.5">
            <Checkbox
              checked={it.enabled}
              onCheckedChange={(c) => update(it.id, { enabled: !!c })}
            />
            <Input
              value={it.key}
              placeholder={t("kv.keyPlaceholder") as string}
              className="h-8 flex-1 font-mono text-xs"
              onChange={(e) => update(it.id, { key: e.target.value })}
            />
            <div className="flex flex-1 items-center gap-1">
              {isFile ? (
                <div className="flex h-8 flex-1 items-center gap-1.5 rounded-md border border-border bg-muted/40 px-2">
                  <Paperclip className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  <span
                    className="truncate font-mono text-xs"
                    title={it.file?.path ?? it.file?.name}
                  >
                    {it.file?.path ?? it.file?.name ?? "—"}
                  </span>
                  <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                    {it.file
                      ? it.file.data
                        ? fileSizeLabel((it.file.data.length * 3) / 4)
                        : it.file.path
                          ? (t("body.localFile") as string)
                          : ""
                      : ""}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    title={t("body.replaceFile") as string}
                    onClick={() => pickFileForRow(it.id)}
                  >
                    <FileUp className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    title={t("body.toText") as string}
                    onClick={() => toText(it.id)}
                  >
                    <Type className="h-3.5 w-3.5" />
                  </Button>
                </div>
              ) : (
                <>
                  <Input
                    value={it.value}
                    placeholder={t("kv.valuePlaceholder") as string}
                    className="h-8 flex-1 font-mono text-xs"
                    onChange={(e) => update(it.id, { value: e.target.value })}
                  />
                  {mode === "form-data" && (
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      title={t("body.toFile") as string}
                      onClick={() => pickFileForRow(it.id)}
                    >
                      <Paperclip className="h-3.5 w-3.5" />
                    </Button>
                  )}
                  {enableDynamic && (
                    <DynamicValuePicker
                      onInsert={(token) => insertDynamic(it.id, token)}
                    />
                  )}
                </>
              )}
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => remove(it.id)}
            >
              {isFile ? (
                <X className="h-3.5 w-3.5" />
              ) : (
                <Trash2 className="h-3.5 w-3.5" />
              )}
            </Button>
            {/* Hidden file input: used by the browser only (no real path available) to read base64;
                the Tauri desktop uses the native dialog instead (pickFileForRow → pickTauriFile) for the real path. */}
            {mode === "form-data" && !isTauri() && (
              <input
                ref={(el) => {
                  fileInputRefs.current[it.id] = el;
                }}
                type="file"
                className="hidden"
                onChange={(e) => {
                  onFilePicked(it.id, e.target.files?.[0]);
                  e.target.value = "";
                }}
              />
            )}
          </div>
        );
      })}
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

function fileSizeLabel(bytes: number): string {
  if (!bytes || bytes < 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v < 10 && u > 0 ? 1 : 0)} ${units[u]}`;
}
