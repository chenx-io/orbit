// Import dialog: every format (cURL / Postman / OpenAPI / HAR / JMeter / k6) — parse → select → import into a collection/folder/scenario.
// UI details are split into sibling components (FileDropZone / EndpointList / CollectionFolderPicker).
import { useCallback, useEffect, useState } from "react";
import {
  Download,
  FileJson,
  Wand2,
  Loader2,
  FolderOpen,
  Plus,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { Switch } from "@/components/ui/switch";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import {
  parseImport,
  readTextFile,
  isTauri,
  type ImportParseResult,
} from "@/lib/bridge";
import { FileDropZone, type ImportFormat } from "./FileDropZone";
import { sniffImportFormat } from "@/lib/importSniff";
import { EndpointGroups, SchemaItem } from "./EndpointList";
import { CollectionFolderPicker } from "./CollectionFolderPicker";
import { BrandIcon } from "@/components/common/BrandIcon";
import {
  siApachejmeter,
  siCurl,
  siK6,
  siOpenapiinitiative,
  siPostman,
} from "simple-icons";

export function ImportDialog() {
  const { t, format } = useT();
  const open = useAppStore((s) => s.ui.importOpen);
  const setOpen = useAppStore((s) => s.setImportOpen);
  const collections = useAppStore((s) => s.collections);
  const addCollection = useAppStore((s) => s.addCollection);
  const importRequests = useAppStore((s) => s.importRequests);
  const addScenario = useAppStore((s) => s.addScenario);
  const addModel = useAppStore((s) => s.addModel);
  const updateModel = useAppStore((s) => s.updateModel);

  const [tab, setTab] = useState<ImportFormat>("curl");
  const [raw, setRaw] = useState("");
  const [parseResult, setParseResult] = useState<ImportParseResult | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Selection state
  const [selectedEndpoints, setSelectedEndpoints] = useState<Set<number>>(
    new Set(),
  );
  const [selectedSchemas, setSelectedSchemas] = useState<Set<number>>(
    new Set(),
  );
  const [phaseTab, setPhaseTab] = useState<"endpoints" | "models">("endpoints");
  const [importAsScenario, setImportAsScenario] = useState(false);

  // Target directory
  const [targetCollectionId, setTargetCollectionId] = useState<string>("");
  const [targetFolderId, setTargetFolderId] = useState<string | null>(null);

  // ─── Core parse with optional content override ──
  // formatOverride: passed explicitly after sniffFormat decides on a dropped file, so the async setTab cannot make parsing use the old tab
  const doParse = useCallback(
    async (contentOverride?: string, formatOverride?: ImportFormat) => {
      const input = contentOverride ?? raw;
      const fmt = formatOverride ?? tab;
      if (!input.trim()) return;
      setLoading(true);
      setError(null);
      setParseResult(null);
      setSelectedEndpoints(new Set());
      setSelectedSchemas(new Set());
      try {
        const result = await parseImport(fmt, input);
        if (result.endpoints.length === 0 && result.schemas.length === 0) {
          setError(t("import.empty"));
        } else {
          setParseResult(result);
          // Select all by default
          setSelectedEndpoints(new Set(result.endpoints.map((_, i) => i)));
          setSelectedSchemas(new Set(result.schemas.map((_, i) => i)));
        }
      } catch {
        setError(t("import.failed"));
      } finally {
        setLoading(false);
      }
    },
    [raw, tab, t],
  );

  // File import: auto-sniff the format from the content (never trust the current tab — it may not be the target format when a file is dropped).
  // Detection lives in lib/importSniff.ts: OpenAPI/Swagger (JSON+YAML) → Postman → HAR → JMeter → k6 → curl fallback.
  const handleFileLoad = useCallback(
    (content: string) => {
      const fmt = sniffImportFormat(content);
      setTab(fmt);
      setRaw(content);
      doParse(content, fmt);
    },
    [doParse],
  );

  // ─── Tauri OS drag-drop listener ──────────────
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    import("@tauri-apps/api/event")
      .then(({ listen }) => {
        listen<{ paths: string[] }>("tauri://drag-drop", async (event) => {
          const path = event.payload.paths?.[0];
          if (!path) return;
          try {
            const content = await readTextFile(path);
            handleFileLoad(content);
          } catch {
            // silently ignore
          }
        }).then((fn) => {
          unlisten = fn;
        });
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, [handleFileLoad]);

  // Prefix the base_url placeholder only when ep.url is relative (starts with /);
  // absolute http(s):// URLs and {{var}}/{var} variable URLs are used as-is (avoiding a duplicated base_url)
  const qualifyUrl = (u: string) =>
    /^(https?:)?\/\//i.test(u) || /^\{/.test(u)
      ? u
      : `{{base_url}}${u.startsWith("/") ? "" : "/"}${u}`;

  const doImport = () => {
    if (!parseResult) return;

    // Scenario mode: create a new scenario
    if (importAsScenario) {
      addScenario(
        format("import.generatedScenarioName", new Date().toLocaleTimeString()),
      );
      importRequests(
        parseResult.endpoints.map((ep) => ({
          name: ep.name,
          method: ep.method,
          url: qualifyUrl(ep.url),
          headers: ep.headers,
          queryParams: ep.query_params || {},
          body: ep.body,
          contentType: ep.content_type,
          group: ep.group,
          summary: ep.summary,
          modelRef: ep.model_ref || "",
          authType: ep.auth_type || "none",
          authKeyName: ep.auth_key_name || "",
          authAddTo: ep.auth_add_to || "",
          responses: ep.responses || [],
          prereqScript: ep.pre_script || "",
          postreqScript: ep.post_script || "",
        })),
        targetCollectionId || undefined,
        targetFolderId || null,
        {},
      );
      resetAndClose();
      return;
    }

    // API mode
    const toImport = parseResult.endpoints
      .filter((_, i) => selectedEndpoints.has(i))
      .map((ep) => ({
        name: ep.name,
        method: ep.method,
        url: qualifyUrl(ep.url),
        headers: ep.headers,
        queryParams: ep.query_params || {},
        body: ep.body,
        contentType: ep.content_type,
        group: ep.group,
        summary: ep.summary,
        modelRef: ep.model_ref || "",
        authType: ep.auth_type || "none",
        authKeyName: ep.auth_key_name || "",
        authAddTo: ep.auth_add_to || "",
        responses: ep.responses || [],
        prereqScript: ep.pre_script || "",
        postreqScript: ep.post_script || "",
      }));
    const createdModelIds: Record<string, string> = {};

    // Import selected schemas + any models referenced by selected endpoints
    const selectedSchemasList = parseResult.schemas.filter((_, i) =>
      selectedSchemas.has(i),
    );
    // Also include schemas referenced by selected endpoints via model_ref
    const referencedModelNames = new Set(
      toImport.map((ep) => ep.modelRef).filter(Boolean),
    );
    for (const schema of parseResult.schemas) {
      if (
        referencedModelNames.has(schema.name) &&
        !selectedSchemasList.some((s) => s.name === schema.name)
      ) {
        selectedSchemasList.push(schema);
      }
    }
    for (const schema of selectedSchemasList) {
      const modelId = addModel(schema.name);
      createdModelIds[schema.name] = modelId;
      const s = schema.schema_json ?? {};
      const props: Record<string, any> = s.properties ?? {};
      const required: string[] = s.required ?? [];
      const fields = Object.entries(props).map(
        ([name, field]: [string, any]) => {
          const rawType = (field.type ?? "string") as string;
          const validType = (
            [
              "string",
              "integer",
              "number",
              "boolean",
              "object",
              "array",
              "null",
            ].includes(rawType)
              ? rawType
              : "string"
          ) as
            | "string"
            | "integer"
            | "number"
            | "boolean"
            | "object"
            | "array"
            | "null";
          return {
            id: `f_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
            name,
            type: validType,
            required: required.includes(name),
            example: field.example ?? field.default ?? "",
            description: field.description ?? "",
          };
        },
      );
      // The OpenAPI schema description is used as the model description (a standard field)
      updateModel(modelId, {
        fields,
        description: (s.description as string) ?? "",
      });
    }

    importRequests(
      toImport,
      targetCollectionId || undefined,
      targetFolderId || null,
      createdModelIds,
    );

    resetAndClose();
  };

  const resetAndClose = () => {
    setOpen(false);
    setRaw("");
    setParseResult(null);
    setSelectedEndpoints(new Set());
    setSelectedSchemas(new Set());
    setError(null);
  };

  const toggleEndpoint = (i: number) => {
    setSelectedEndpoints((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };
  const toggleSchema = (i: number) => {
    setSelectedSchemas((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };
  const selectAllEndpoints = () =>
    setSelectedEndpoints(
      new Set(parseResult?.endpoints.map((_, i) => i) ?? []),
    );
  const deselectAllEndpoints = () => setSelectedEndpoints(new Set());

  // Initial collection
  const defaultColId = collections[0]?.id ?? "";
  const effectiveColId = targetCollectionId || defaultColId;

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v) resetAndClose();
        else setOpen(true);
      }}
    >
      <DialogContent className="max-w-5xl max-h-[90vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-4 w-4 text-primary" /> {t("import.title")}
          </DialogTitle>
        </DialogHeader>

        {/* Phase 1: Input */}
        {!parseResult && (
          <>
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
                  <BrandIcon
                    icon={siOpenapiinitiative}
                    className="h-3.5 w-3.5"
                  />{" "}
                  OpenAPI/Swagger
                </TabsTrigger>
                <TabsTrigger value="har" className="gap-1.5 text-xs">
                  <FileJson className="h-3.5 w-3.5" /> HAR
                </TabsTrigger>
                <TabsTrigger value="jmeter" className="gap-1.5 text-xs">
                  <BrandIcon icon={siApachejmeter} className="h-3.5 w-3.5" />{" "}
                  JMeter
                </TabsTrigger>
                <TabsTrigger value="k6" className="gap-1.5 text-xs">
                  <BrandIcon icon={siK6} className="h-3.5 w-3.5" /> k6
                </TabsTrigger>
              </TabsList>

              {(
                [
                  "curl",
                  "postman",
                  "openapi",
                  "har",
                  "jmeter",
                  "k6",
                ] as ImportFormat[]
              ).map((fmt) => (
                <TabsContent key={fmt} value={fmt} className="mt-2">
                  <Textarea
                    value={raw}
                    onChange={(e) => setRaw(e.target.value)}
                    placeholder={
                      fmt === "curl"
                        ? "curl -X POST 'https://api.example.com/login' -H 'Content-Type: application/json' -d '{\"user\":\"admin\"}'"
                        : fmt === "postman"
                          ? t("import.postmanPlaceholder")
                          : fmt === "openapi"
                            ? t("import.openapiPlaceholder")
                            : fmt === "har"
                              ? t("import.harPlaceholder")
                              : fmt === "jmeter"
                                ? t("import.pasteJmx")
                                : t("import.pasteK6")
                    }
                    className="min-h-50 font-mono text-xs"
                  />
                  {fmt !== "curl" && (
                    <FileDropZone fmt={fmt} onFileLoad={handleFileLoad} t={t} />
                  )}
                </TabsContent>
              ))}
            </Tabs>

            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={() => doParse()}
                disabled={!raw.trim() || loading}
                className="gap-1.5"
              >
                {loading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Wand2 className="h-3.5 w-3.5" />
                )}
                {t("import.parse")}
              </Button>
              {error && <span className="text-xs text-rose-400">{error}</span>}
            </div>
          </>
        )}

        {/* Phase 2: Parse result */}
        {parseResult && (
          <div className="flex flex-1 flex-col min-h-0 gap-2">
            {/* Tabs + toolbar */}
            <div className="flex items-center gap-2">
              <div className="flex rounded-md bg-muted p-0.5">
                {(["endpoints", "models"] as const).map((k) => (
                  <button
                    key={k}
                    onClick={() => setPhaseTab(k)}
                    className={cn(
                      "rounded px-3 py-1 text-xs font-medium transition-colors",
                      phaseTab === k
                        ? "bg-background text-foreground shadow-sm"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {t(
                      `import.tab${k === "endpoints" ? "Endpoints" : "Models"}` as any,
                    )}
                    <Badge variant="outline" className="ml-1.5 text-xs">
                      {k === "endpoints"
                        ? `${selectedEndpoints.size}/${parseResult.endpoints.length}`
                        : `${selectedSchemas.size}/${parseResult.schemas.length}`}
                    </Badge>
                  </button>
                ))}
              </div>
              <div className="ml-auto flex items-center gap-1.5">
                {phaseTab === "endpoints" ? (
                  <>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={selectAllEndpoints}
                      className="text-xs h-7"
                    >
                      {t("import.selectAll")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={deselectAllEndpoints}
                      className="text-xs h-7"
                    >
                      {t("import.deselectAll")}
                    </Button>
                  </>
                ) : (
                  <>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setSelectedSchemas(
                          new Set(parseResult.schemas.map((_, i) => i)),
                        )
                      }
                      className="text-xs h-7"
                    >
                      {t("import.selectAll")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => setSelectedSchemas(new Set())}
                      className="text-xs h-7"
                    >
                      {t("import.deselectAll")}
                    </Button>
                  </>
                )}
              </div>
            </div>

            {/* Directory selector (compact) */}
            <div className="flex items-center gap-2 rounded-md border border-border px-3 py-1.5">
              <FolderOpen className="h-3.5 w-3.5 shrink-0 text-amber-400" />
              <span className="text-xs text-muted-foreground">
                {t("import.targetDir")}:
              </span>
              {collections.length > 0 ? (
                <CollectionFolderPicker
                  collections={collections}
                  selectedColId={effectiveColId}
                  selectedFolderId={targetFolderId}
                  onSelect={(colId, folderId) => {
                    setTargetCollectionId(colId);
                    setTargetFolderId(folderId);
                  }}
                />
              ) : (
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 gap-1 text-xs"
                  onClick={() => {
                    addCollection(t("import.defaultCollection"));
                    // Select the newly created collection (it sits at the end of the array)
                    const updated = useAppStore.getState().collections;
                    const latest = updated[updated.length - 1];
                    if (latest) setTargetCollectionId(latest.id);
                  }}
                >
                  <Plus className="h-3 w-3" /> {t("collection.new")}
                </Button>
              )}
            </div>

            {/* List area */}
            <div className="flex min-h-0 flex-1 flex-col overflow-auto rounded-lg border border-border">
              <div className="space-y-0.5 p-2">
                {phaseTab === "endpoints" ? (
                  <EndpointGroups
                    endpoints={parseResult.endpoints}
                    selected={selectedEndpoints}
                    onToggle={toggleEndpoint}
                  />
                ) : (
                  parseResult.schemas.map((schema, i) => (
                    <SchemaItem
                      key={i}
                      schema={schema}
                      selected={selectedSchemas.has(i)}
                      onToggle={() => toggleSchema(i)}
                    />
                  ))
                )}
              </div>
            </div>

            {/* Bottom bar */}
            <div className="flex items-center gap-2 border-t border-border pt-2">
              <Button size="sm" variant="ghost" onClick={resetAndClose}>
                {t("common.cancel")}
              </Button>
              <label className="ml-2 flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer">
                <Switch
                  checked={importAsScenario}
                  onCheckedChange={setImportAsScenario}
                  className="scale-75"
                />
                {t("import.asScenario")}
              </label>
              <Button
                size="sm"
                className="ml-auto gap-1.5"
                onClick={doImport}
                disabled={selectedEndpoints.size === 0}
              >
                <Download className="h-3.5 w-3.5" />
                {t("import.doImport")} ({selectedEndpoints.size})
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
