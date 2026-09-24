// Binary request-body mode: file picker + selected-file info + clear.
// - Tauri desktop: pick via the native dialog and store the real path (no pre-reading, no size limit);
// - Browser: read base64 via a hidden <input> (capped at 50MB to avoid freezing the page).
import { useRef, useState } from "react";
import { FileUp, Paperclip, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { HttpRequest } from "@/data/types";
import { readFileAsBase64 } from "@/lib/requestBody";
import { isTauri } from "@/lib/bridge";
import { pickTauriFile } from "@/lib/filePicker";
import { useT } from "@/lib/i18n";
import { fileSize } from "@/lib/utils";

export function BinaryBodyEditor({
  req,
  set,
}: {
  req: HttpRequest;
  set: (patch: Partial<HttpRequest>) => void;
}) {
  const { t } = useT();
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [error, setError] = useState<string | null>(null);

  const onPick = async (file: File | undefined) => {
    if (!file) return;
    setError(null);
    // Cap at 50MB; avoid stuffing a huge file into base64 and freezing the page (only needed when pre-reading in the browser)
    if (file.size > 50 * 1024 * 1024) {
      setError(t("body.fileTooLarge") as string);
      return;
    }
    try {
      const f = await readFileAsBase64(file);
      set({ binaryFile: f });
    } catch {
      setError(t("body.fileReadError") as string);
    }
  };

  const onPickTauri = async () => {
    setError(null);
    const picked = await pickTauriFile();
    if (picked) {
      set({
        binaryFile: {
          name: picked.name,
          type: picked.type,
          path: picked.path,
        },
      });
    }
  };

  const file = req.binaryFile;

  return (
    <div className="space-y-2">
      <input
        ref={fileRef}
        type="file"
        className="hidden"
        onChange={(e) => {
          onPick(e.target.files?.[0]);
          e.target.value = "";
        }}
      />
      {file ? (
        <div className="flex items-center gap-2 rounded-md border border-border bg-muted/40 p-2">
          <Paperclip className="h-4 w-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0 flex-1">
            <div
              className="truncate font-mono text-xs"
              title={file.path ?? file.name}
            >
              {file.path ?? file.name}
            </div>
            <div className="text-xs text-muted-foreground">
              {file.type || "application/octet-stream"}
              {file.data
                ? ` · ${fileSize(file.data)}`
                : file.path
                  ? ` · ${t("body.localFile")}`
                  : ""}
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              isTauri() ? onPickTauri() : fileRef.current?.click()
            }
          >
            {t("body.replaceFile")}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => set({ binaryFile: null })}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      ) : (
        <Button
          variant="outline"
          size="sm"
          onClick={() => (isTauri() ? onPickTauri() : fileRef.current?.click())}
        >
          <FileUp className="h-3.5 w-3.5" /> {t("body.chooseFile")}
        </Button>
      )}
      {error && <div className="text-xs text-rose-400">{error}</div>}
      <div className="text-xs text-muted-foreground">
        {t("body.binaryHint")}
      </div>
    </div>
  );
}
