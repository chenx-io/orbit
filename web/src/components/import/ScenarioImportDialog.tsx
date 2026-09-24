import { useState, useCallback } from "react";
import { Download, FileJson, Loader2 } from "lucide-react";
import { BrandIcon } from "@/components/common/BrandIcon";
import {
  siApachejmeter,
  siCurl,
  siK6,
  siOpenapiinitiative,
  siPostman,
} from "simple-icons";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { importScenario, readTextFile, isTauri } from "@/lib/bridge";
import { sniffImportFormat } from "@/lib/importSniff";

type ImportFormat = "curl" | "postman" | "openapi" | "jmeter" | "k6" | "har";

export function ScenarioImportDialog() {
  const { t, format } = useT();
  const open = useAppStore((s) => s.ui.scenarioImportOpen ?? false);
  const setOpen = useCallback(
    (v: boolean) => useAppStore.getState().setScenarioImportOpen(v),
    [],
  );
  const addScenario = useAppStore((s) => s.addScenario);
  const updateScenario = useAppStore((s) => s.updateScenario);
  const setActive = useAppStore((s) => s.setActiveScenario);

  const [tab, setTab] = useState<ImportFormat>("curl");
  const [raw, setRaw] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const doImport = useCallback(async () => {
    if (!raw.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const { yaml } = await importScenario(tab, raw);
      const plan: any = parseYamlLite(yaml);
      const name =
        plan?.scenarios?.[0]?.name ||
        t(`import.tab.${tab}` as any) ||
        t("import.scenarioTitle");
      const steps = plan?.scenarios?.[0]?.steps || [];

      const scenarioId = addScenario(name);
      setActive(scenarioId);

      // Convert YAML steps to ScenarioStep format
      if (steps.length > 0) {
        const converted = steps.map((s: any, i: number) =>
          yamlStepToScenarioStep(s, i),
        );
        updateScenario(scenarioId, { steps: converted });
      }
      setOpen(false);
      setRaw("");
    } catch (e: any) {
      setError(e?.message || String(e));
    } finally {
      setLoading(false);
    }
  }, [raw, tab, addScenario, setActive, updateScenario, setOpen, t]);

  const handleFileLoad = useCallback((content: string) => {
    // Sniff the format of a dropped file and switch tabs (so the default curl tab cannot misparse YAML/JSON)
    setTab(sniffImportFormat(content));
    setRaw(content);
  }, []);

  // Tauri drag-drop
  if (isTauri()) {
    import("@tauri-apps/api/event")
      .then(({ listen }) => {
        listen<{ paths: string[] }>("tauri://drag-drop", async (event) => {
          const path = event.payload.paths?.[0];
          if (!path) return;
          try {
            const content = await readTextFile(path);
            handleFileLoad(content);
          } catch {
            /* ignore */
          }
        });
      })
      .catch(() => {});
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-4 w-4 text-primary" />{" "}
            {t("import.scenarioTitle")}
          </DialogTitle>
          <DialogDescription>{t("import.scenarioDesc")}</DialogDescription>
        </DialogHeader>

        <Tabs
          value={tab}
          onValueChange={(v) => {
            setTab(v as ImportFormat);
            setRaw("");
            setError(null);
          }}
        >
          <TabsList className="h-8">
            <TabsTrigger value="curl" className="gap-1.5 text-xs">
              <BrandIcon icon={siCurl} className="h-3.5 w-3.5" /> cURL
            </TabsTrigger>
            <TabsTrigger value="postman" className="gap-1.5 text-xs">
              <BrandIcon icon={siPostman} className="h-3.5 w-3.5" /> Postman
            </TabsTrigger>
            <TabsTrigger value="openapi" className="gap-1.5 text-xs">
              <BrandIcon icon={siOpenapiinitiative} className="h-3.5 w-3.5" />{" "}
              OpenAPI
            </TabsTrigger>
            <TabsTrigger value="jmeter" className="gap-1.5 text-xs">
              <BrandIcon icon={siApachejmeter} className="h-3.5 w-3.5" /> JMeter
            </TabsTrigger>
            <TabsTrigger value="k6" className="gap-1.5 text-xs">
              <BrandIcon icon={siK6} className="h-3.5 w-3.5" /> k6
            </TabsTrigger>
            <TabsTrigger value="har" className="gap-1.5 text-xs">
              <FileJson className="h-3.5 w-3.5" /> HAR
            </TabsTrigger>
          </TabsList>

          {(
            [
              "curl",
              "postman",
              "openapi",
              "jmeter",
              "k6",
              "har",
            ] as ImportFormat[]
          ).map((fmt) => (
            <TabsContent key={fmt} value={fmt} className="mt-2">
              <Textarea
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                placeholder={
                  fmt === "curl"
                    ? "curl -X GET https://api.example.com/users"
                    : fmt === "jmeter"
                      ? t("import.pasteJmx")
                      : fmt === "k6"
                        ? t("import.pasteK6")
                        : fmt === "har"
                          ? t("import.harPlaceholder")
                          : format("import.pasteFormat", fmt)
                }
                className="min-h-55 font-mono text-xs"
              />
            </TabsContent>
          ))}
        </Tabs>

        {error && <p className="text-xs text-destructive">{error}</p>}

        <div className="flex items-center gap-2 border-t border-border pt-3">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setOpen(false);
              setRaw("");
            }}
          >
            {t("common.cancel")}
          </Button>
          <Button
            size="sm"
            className="ml-auto gap-1.5"
            onClick={doImport}
            disabled={!raw.trim() || loading}
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            {t("import.doImport")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ─── Helpers ──────────────────────────────────────────

function parseYamlLite(yaml: string): any {
  // Simple YAML parser for the test plan structure
  // We only need to extract: scenarios[0].name and scenarios[0].steps
  const lines = yaml.split("\n");
  const root: any = {};
  let current: any = root;
  const stack: { obj: any; indent: number }[] = [{ obj: root, indent: -1 }];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim() || line.trim().startsWith("#")) continue;
    const indent = line.search(/\S/);
    const trimmed = line.trim();

    // Pop stack to correct indent level
    while (stack.length > 1 && stack[stack.length - 1].indent >= indent) {
      stack.pop();
    }
    current = stack[stack.length - 1].obj;

    if (trimmed.startsWith("- ")) {
      // Array item
      if (!current._arr) {
        current._arr = [];
        (current as any)._isArray = true;
      }
      const item: any = {};
      current._arr.push(item);
      stack.push({ obj: item, indent });
      current = item;
      // Parse the value after "- "
      const rest = trimmed.slice(2);
      const colonIdx = rest.indexOf(":");
      if (colonIdx >= 0 && rest[colonIdx + 1] === " ") {
        current[rest.slice(0, colonIdx).trim()] = rest
          .slice(colonIdx + 1)
          .trim()
          .replace(/^"(.*)"$/, "$1");
      }
    } else {
      const colonIdx = trimmed.indexOf(":");
      if (colonIdx >= 0) {
        const key = trimmed.slice(0, colonIdx).trim();
        const val = trimmed
          .slice(colonIdx + 1)
          .trim()
          .replace(/^"(.*)"$/, "$1");
        // May be a nested object
        if (val === "") {
          current[key] = {};
          stack.push({ obj: current[key], indent });
        } else {
          current[key] = val;
        }
      }
    }
  }

  // Normalize: convert _arr to arrays
  function normalize(obj: any): any {
    if (obj._arr) return obj._arr.map(normalize);
    if (typeof obj === "object" && obj !== null) {
      const out: any = {};
      for (const [k, v] of Object.entries(obj)) {
        if (k.startsWith("_")) continue;
        out[k] = normalize(v);
      }
      return out;
    }
    return obj;
  }
  return normalize(root);
}

let _stCounter = 0;
function yamlStepToScenarioStep(s: any, _i: number): any {
  const id = `st_import_${++_stCounter}_${Date.now().toString(36)}`;
  const base = {
    id,
    name: s.name || "Step",
    type: s.type || "request",
    disabled: s.disabled === true || s.disabled === "true",
  };

  if (s.type === "request" || !s.type) {
    return {
      ...base,
      type: "request",
      requestId: "", // populated later by user
    };
  }
  if (s.type === "wait") {
    const dur = s.duration || "1s";
    const ms = dur.endsWith("ms") ? parseInt(dur) : parseFloat(dur) * 1000;
    return { ...base, type: "wait", ms: ms || 1000 };
  }
  if (s.type === "loop") {
    return {
      ...base,
      type: "loop",
      count: s.count || 1,
      children: (s.steps || []).map((c: any, j: number) =>
        yamlStepToScenarioStep(c, j),
      ),
    };
  }
  if (s.type === "setvar") {
    return {
      ...base,
      type: "setvar",
      varKey: s.key || "var",
      varValue: s.value || "",
    };
  }
  if (s.type === "condition") {
    return {
      ...base,
      type: "condition",
      expr: s.expression || "true",
      children: (s.then || []).map((c: any, j: number) =>
        yamlStepToScenarioStep(c, j),
      ),
      elseChildren: (s.else || []).map((c: any, j: number) =>
        yamlStepToScenarioStep(c, j),
      ),
    };
  }
  if (s.type === "group") {
    return {
      ...base,
      type: "group",
      children: (s.steps || []).map((c: any, j: number) =>
        yamlStepToScenarioStep(c, j),
      ),
    };
  }
  return base;
}
