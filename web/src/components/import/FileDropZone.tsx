// Import format kinds and the file drop zone.
import { useRef, useState, type DragEvent } from "react";
import { Upload } from "lucide-react";
import { cn } from "@/lib/utils";

export type ImportFormat =
  "curl" | "postman" | "openapi" | "har" | "k6" | "jmeter";

/** File-picking rules per format (accept list and hint key). curl uses paste and has no drop zone. */
const FORMAT_RULES: Partial<
  Record<ImportFormat, { accept: string; labelKey: string }>
> = {
  postman: { accept: ".json", labelKey: "import.dropzone.postman" },
  openapi: { accept: ".json,.yaml,.yml", labelKey: "import.dropzone.openapi" },
  har: { accept: ".har,.json", labelKey: "import.dropzone.har" },
  jmeter: { accept: ".jmx,.xml", labelKey: "import.dropzone.jmeter" },
  k6: { accept: ".js", labelKey: "import.dropzone.k6" },
};

/** File drop / click-to-select zone (cURL uses paste and has no drop zone). */
export function FileDropZone({
  fmt,
  onFileLoad,
  t,
}: {
  fmt: ImportFormat;
  onFileLoad: (content: string) => void;
  t: (key: string) => string;
}) {
  const [dragOver, setDragOver] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const rule = FORMAT_RULES[fmt];

  const readFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (reader.result) onFileLoad(reader.result as string);
    };
    reader.readAsText(file);
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const file = e.dataTransfer.files?.[0];
    if (file) readFile(file);
  };

  return (
    <div
      className={cn(
        "mt-2 flex cursor-pointer flex-col items-center justify-center rounded-lg border-2 border-dashed py-4 text-xs text-muted-foreground transition-colors",
        dragOver
          ? "border-primary bg-primary/5 text-primary"
          : "border-border hover:border-primary/50",
      )}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
      onClick={() => fileRef.current?.click()}
    >
      <Upload className="mb-1 h-5 w-5" />
      <span>{rule ? t(rule.labelKey) : t("import.dropzone.openapi")}</span>
      <span className="mt-0.5 text-xs">{rule?.accept ?? ""}</span>
      <input
        ref={fileRef}
        type="file"
        accept={rule?.accept ?? ".json,.yaml,.yml"}
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) readFile(file);
          e.target.value = "";
        }}
      />
    </div>
  );
}
