// Workspace picker (the startup screen): lists every workspace as cards, entering the main UI after selection.
// Counts come from the per-workspace partition caches; deleting a workspace with data asks for confirmation (warning about clearing).
// Cards support duplicating (a deep copy), workspace-level export (openapi / swagger / postman) and
// workspace-level import (openapi / postman / har / k6 / jmeter / curl → a new workspace).
import { useRef, useState } from "react";
import {
  Boxes,
  Copy,
  Download,
  FolderOpen,
  Globe,
  History,
  Layers,
  Loader2,
  Pencil,
  Plus,
  Send,
  Trash2,
  Upload,
  Workflow,
} from "lucide-react";
import {
  siOpenapiinitiative,
  siPostman,
  siSwagger,
  type SimpleIcon,
} from "simple-icons";
import yaml from "js-yaml";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { BrandIcon } from "@/components/common/BrandIcon";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/useStore";
import { useT } from "@/lib/i18n";
import { toast } from "sonner";
import { defaultFileName, type ExportSpec } from "@/lib/exporters";
import { sniffImportFormat } from "@/lib/importSniff";
import {
  exportCollection,
  isTauri,
  parseImport,
  readTextFile,
  writeExportFile,
} from "@/lib/bridge";
import { MenuBar } from "./MenuBar";

const WS_SPECS: { value: ExportSpec; labelKey: string; icon: SimpleIcon }[] = [
  {
    value: "openapi",
    labelKey: "export.openapi",
    icon: siOpenapiinitiative,
  },
  { value: "swagger", labelKey: "export.swagger", icon: siSwagger },
  { value: "postman", labelKey: "export.postman", icon: siPostman },
];

export function WorkspacePicker() {
  const { t } = useT();
  const workspaces = useAppStore((s) => s.workspaces);
  const workspaceData = useAppStore((s) => s.workspaceData);
  const setActiveWorkspace = useAppStore((s) => s.setActiveWorkspace);
  const addWorkspace = useAppStore((s) => s.addWorkspace);
  const updateWorkspace = useAppStore((s) => s.updateWorkspace);
  const removeWorkspace = useAppStore((s) => s.removeWorkspace);
  const duplicateWorkspace = useAppStore((s) => s.duplicateWorkspace);
  const setActiveModule = useAppStore((s) => s.setActiveModule);
  const addCollection = useAppStore((s) => s.addCollection);
  const importRequests = useAppStore((s) => s.importRequests);
  const addModel = useAppStore((s) => s.addModel);
  const updateModel = useAppStore((s) => s.updateModel);

  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  /** Edit name / description dialog state */
  const [editWs, setEditWs] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  /** Delete confirmation dialog (a DOM dialog, reliably rendered in the Tauri webview; warns about clearing all data when present) */
  const [deleteWs, setDeleteWs] = useState<string | null>(null);
  /** Workspace-level export dialog state */
  const [exportWs, setExportWs] = useState<string | null>(null);
  const [exportSpec, setExportSpec] = useState<ExportSpec>("openapi");
  const [exportFormat, setExportFormat] = useState<"json" | "yaml">("json");
  const [exporting, setExporting] = useState(false);
  /** Workspace-level import state */
  const [importing, setImporting] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const openEdit = (wsId: string) => {
    const ws = workspaces.find((w) => w.id === wsId);
    if (!ws) return;
    setEditName(ws.name);
    setEditDesc(ws.description ?? "");
    setEditWs(wsId);
  };

  const saveEdit = () => {
    if (!editWs) return;
    updateWorkspace(editWs, {
      name: editName,
      description: editDesc.trim() ? editDesc.trim() : undefined,
    });
    setEditWs(null);
  };

  /** Total amount of data in this workspace (deleting with data needs confirmation: it clears everything) */
  const statsOf = (wsId: string) => {
    const d = workspaceData[wsId];
    return {
      collections: d?.collections.length ?? 0,
      requests: Object.keys(d?.requests ?? {}).length,
      models: d?.models.length ?? 0,
      environments: d?.environments.length ?? 0,
      scenarios: d?.scenarios.length ?? 0,
      history: d?.history.length ?? 0,
    };
  };
  const hasData = (wsId: string) => {
    const s = statsOf(wsId);
    return (
      s.collections +
        s.requests +
        s.models +
        s.environments +
        s.scenarios +
        s.history >
      0
    );
  };

  const create = () => {
    if (!name.trim()) return;
    const id = addWorkspace(name, desc || undefined);
    setCreating(false);
    setName("");
    setDesc("");
    setActiveModule("api");
    setActiveWorkspace(id);
  };

  const doWorkspaceExport = async () => {
    if (!exportWs) return;
    setExporting(true);
    try {
      const ws = workspaces.find((w) => w.id === exportWs);
      const json = await exportCollection(
        exportSpec,
        ws?.name ?? "workspace",
        "",
        undefined,
        exportWs,
      );
      const content =
        exportFormat === "yaml"
          ? yaml.dump(JSON.parse(json), {
              noRefs: true,
              indent: 2,
              lineWidth: 120,
            })
          : json;
      const fileName = defaultFileName(
        ws?.name ?? "workspace",
        exportSpec,
        exportFormat,
      );
      if (isTauri()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const filters =
          exportFormat === "yaml"
            ? [{ name: "YAML", extensions: ["yaml", "yml"] }]
            : [{ name: "JSON", extensions: ["json"] }];
        const path = await save({ defaultPath: fileName, filters });
        if (path) {
          await writeExportFile(path, content);
          toast.success(t("export.success", path));
        }
      } else {
        writeExportFile(fileName, content);
        toast.success(t("export.success", fileName));
      }
      setExportWs(null);
    } catch (e) {
      toast.error(
        t("export.failed", e instanceof Error ? e.message : String(e)),
      );
    } finally {
      setExporting(false);
    }
  };

  // ─── Workspace-level import ─────────────────────────────

  // Prefix the base_url placeholder only when ep.url is relative (starts with /);
  // absolute http(s):// URLs and {{var}}/{var} variable URLs are used as-is
  const qualifyUrl = (u: string) =>
    /^(https?:)?\/\//i.test(u) || /^\{/.test(u)
      ? u
      : `{{base_url}}${u.startsWith("/") ? "" : "/"}${u}`;

  /** Parse document content → a new workspace + default collection + requests/models (reusing the backend parse_import). */
  const doWorkspaceImport = async (content: string, fileName: string) => {
    if (!content.trim()) return;
    setImporting(true);
    try {
      const fmt = sniffImportFormat(content);
      const result = await parseImport(fmt, content);
      if (result.endpoints.length === 0 && result.schemas.length === 0) {
        toast.error(t("import.empty"));
        return;
      }
      // The workspace name comes from the file name (extension stripped) and the collection name from the source format
      const wsName =
        fileName.replace(/\.(json|yaml|yml)$/i, "").trim() ||
        t("workspace.untitled");
      const wsId = addWorkspace(wsName);
      setActiveWorkspace(wsId);

      const colName =
        fmt === "postman"
          ? "Postman"
          : fmt === "openapi"
            ? "OpenAPI"
            : fmt === "har"
              ? "HAR"
              : fmt === "jmeter"
                ? "JMeter"
                : fmt === "k6"
                  ? "k6"
                  : "cURL";
      const colId = addCollection(colName);

      // Data model (schema) → model
      const createdModelIds: Record<string, string> = {};
      for (const schema of result.schemas) {
        const modelId = addModel(schema.name);
        createdModelIds[schema.name] = modelId;
        const s = schema.schema_json ?? {};
        const props: Record<string, any> = s.properties ?? {};
        const required: string[] = s.required ?? [];
        const fields = Object.entries(props).map(
          ([name, field]: [string, any]) => {
            const rawType = (field.type ?? "string") as string;
            return {
              id: `f_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
              name,
              type: ([
                "string",
                "integer",
                "number",
                "boolean",
                "object",
                "array",
                "null",
              ].includes(rawType)
                ? rawType
                : "string") as
                | "string"
                | "integer"
                | "number"
                | "boolean"
                | "object"
                | "array"
                | "null",
              required: required.includes(name),
              example: field.example ?? field.default ?? "",
              description: field.description ?? "",
            };
          },
        );
        updateModel(modelId, {
          fields,
          description: (s.description as string) ?? "",
        });
      }

      // Endpoint → request (grouping comes from the backend ep.group, creating folders inside the collection)
      importRequests(
        result.endpoints.map((ep) => ({
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
        colId,
        null,
        createdModelIds,
      );

      toast.success(t("workspace.importSuccess", wsName));
      setActiveModule("api");
    } catch {
      toast.error(t("import.failed"));
    } finally {
      setImporting(false);
    }
  };

  /** Pick the import file: a system dialog on Tauri, a hidden file input on the web. */
  const pickImportFile = async () => {
    if (isTauri()) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({
        multiple: false,
        filters: [
          {
            name: t("workspace.importFilterName"),
            extensions: ["json", "yaml", "yml"],
          },
        ],
      });
      if (typeof path === "string") {
        const name = path.split(/[\\/]/).pop() ?? "workspace";
        doWorkspaceImport(await readTextFile(path), name);
      }
    } else {
      fileInputRef.current?.click();
    }
  };

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      {/* Menu bar (the title-bar layer, matching the main UI) */}
      <MenuBar />

      {/* Body: the workspace card grid */}
      <main className="flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-8 py-10">
        <h1 className="text-xl font-semibold">{t("workspace.pickTitle")}</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("workspace.pickDesc")}
        </p>

        <div className="mt-8 grid w-full max-w-4xl grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {workspaces.map((ws) => (
            <div
              key={ws.id}
              onClick={() => {
                setActiveModule("api");
                setActiveWorkspace(ws.id);
              }}
              className="group relative flex cursor-pointer flex-col overflow-hidden rounded-xl border border-border bg-card text-left transition-all hover:-translate-y-0.5 hover:border-primary/60 hover:shadow-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50"
            >
              {/* Actions (shown on hover; clicking does not enter) */}
              <span className="absolute right-2 top-2 z-10 hidden items-center gap-1 group-hover:flex">
                <span
                  role="button"
                  title={t("workspace.edit")}
                  onClick={(e) => {
                    e.stopPropagation();
                    openEdit(ws.id);
                  }}
                  className="flex h-6 w-6 cursor-pointer items-center justify-center rounded bg-background/80 text-muted-foreground hover:bg-primary/20 hover:text-primary"
                >
                  <Pencil className="h-3.5 w-3.5" />
                </span>
                <span
                  role="button"
                  title={t("workspace.duplicate")}
                  onClick={(e) => {
                    e.stopPropagation();
                    duplicateWorkspace(ws.id);
                  }}
                  className="flex h-6 w-6 cursor-pointer items-center justify-center rounded bg-background/80 text-muted-foreground hover:bg-primary/20 hover:text-primary"
                >
                  <Copy className="h-3.5 w-3.5" />
                </span>
                <span
                  role="button"
                  title={t("workspace.exportWs")}
                  onClick={(e) => {
                    e.stopPropagation();
                    setExportSpec("openapi");
                    setExportFormat("json");
                    setExportWs(ws.id);
                  }}
                  className="flex h-6 w-6 cursor-pointer items-center justify-center rounded bg-background/80 text-muted-foreground hover:bg-primary/20 hover:text-primary"
                >
                  <Download className="h-3.5 w-3.5" />
                </span>
                <span
                  role="button"
                  title={t("workspace.delete")}
                  onClick={(e) => {
                    e.stopPropagation();
                    setDeleteWs(ws.id);
                  }}
                  className="flex h-6 w-6 cursor-pointer items-center justify-center rounded bg-background/80 text-muted-foreground hover:bg-red-500 hover:text-white"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </span>
              </span>
              {/* Color bar */}
              <div
                className="h-1.5 w-full shrink-0"
                style={{ backgroundColor: ws.color ?? "#71717a" }}
              />
              <div className="flex min-h-36 flex-1 flex-col p-4">
                <div className="flex items-center gap-2">
                  <span
                    className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-white"
                    style={{ backgroundColor: ws.color ?? "#71717a" }}
                  >
                    <Globe className="h-4 w-4" />
                  </span>
                  <span className="truncate text-sm font-semibold">
                    {ws.name}
                  </span>
                </div>
                <p className="mt-2 line-clamp-2 min-h-8 text-xs text-muted-foreground">
                  {ws.description || t("workspace.noDesc")}
                </p>
                {/* Stats (live counts from the per-workspace partition caches; hover explains them) */}
                <div className="mt-auto grid grid-cols-3 gap-x-3 gap-y-1 pt-3 text-xs text-muted-foreground">
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statCollections")}
                  >
                    <FolderOpen className="h-3 w-3" />{" "}
                    {statsOf(ws.id).collections}
                  </span>
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statRequests")}
                  >
                    <Send className="h-3 w-3" /> {statsOf(ws.id).requests}
                  </span>
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statModels")}
                  >
                    <Layers className="h-3 w-3" /> {statsOf(ws.id).models}
                  </span>
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statEnvs")}
                  >
                    <Boxes className="h-3 w-3" /> {statsOf(ws.id).environments}
                  </span>
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statScenarios")}
                  >
                    <Workflow className="h-3 w-3" /> {statsOf(ws.id).scenarios}
                  </span>
                  <span
                    className="inline-flex items-center gap-1"
                    title={t("workspace.statHistory")}
                  >
                    <History className="h-3 w-3" /> {statsOf(ws.id).history}
                  </span>
                </div>
              </div>
            </div>
          ))}

          {/* New / import workspace card */}
          {creating ? (
            <div className="flex flex-col justify-center gap-2 rounded-xl border-2 border-dashed border-border bg-card/40 p-4">
              <Input
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && create()}
                placeholder={t("workspace.namePlaceholder")}
                className="h-8"
              />
              <Input
                value={desc}
                onChange={(e) => setDesc(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && create()}
                placeholder={t("workspace.descPlaceholder")}
                className="h-8"
              />
              <div className="mt-1 flex gap-2">
                <Button
                  size="sm"
                  className="flex-1"
                  onClick={create}
                  disabled={!name.trim()}
                >
                  {t("common.create")}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setCreating(false)}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </div>
          ) : (
            <div className="flex min-h-36 flex-col overflow-hidden rounded-xl border-2 border-dashed border-border text-muted-foreground">
              <button
                onClick={() => setCreating(true)}
                className="flex flex-1 items-center justify-center gap-2 text-sm transition-colors hover:bg-primary/10 hover:text-foreground"
              >
                <Plus className="h-5 w-5" />
                {t("workspace.new")}
              </button>
              <div className="border-t border-dashed border-border" />
              <button
                onClick={pickImportFile}
                disabled={importing}
                className="flex flex-1 items-center justify-center gap-2 text-sm transition-colors hover:bg-primary/10 hover:text-foreground disabled:opacity-60"
              >
                {importing ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : (
                  <Upload className="h-5 w-5" />
                )}
                {importing ? t("import.parsing") : t("workspace.importWs")}
              </button>
            </div>
          )}
          {/* Web mode file picker */}
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,.yaml,.yml"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) {
                const reader = new FileReader();
                reader.onload = () =>
                  doWorkspaceImport(String(reader.result ?? ""), f.name);
                reader.readAsText(f);
              }
              e.target.value = "";
            }}
          />
        </div>
      </main>

      {/* Delete confirmation dialog: warns that all of this workspace's data will be cleared when present */}
      <ConfirmDialog
        open={deleteWs !== null}
        onOpenChange={(o) => !o && setDeleteWs(null)}
        title={t("workspace.delete")}
        message={
          deleteWs
            ? hasData(deleteWs)
              ? t("workspace.deleteConfirm")
              : t("workspace.deleteConfirmEmpty")
            : ""
        }
        danger
        confirmLabel={t("common.delete")}
        onConfirm={() => {
          if (!deleteWs) return;
          removeWorkspace(deleteWs);
          setDeleteWs(null);
        }}
      />

      {/* Edit name / description dialog */}
      <Dialog
        open={editWs !== null}
        onOpenChange={(o) => !o && setEditWs(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Pencil className="h-4 w-4 text-primary" />
              {t("workspace.edit")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <div className="text-xs text-muted-foreground">
                {t("workspace.namePlaceholder")}
              </div>
              <Input
                autoFocus
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveEdit()}
                className="h-8"
              />
            </div>
            <div className="space-y-1.5">
              <div className="text-xs text-muted-foreground">
                {t("workspace.descPlaceholder")}
              </div>
              <Input
                value={editDesc}
                onChange={(e) => setEditDesc(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveEdit()}
                className="h-8"
              />
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" size="sm" onClick={() => setEditWs(null)}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" onClick={saveEdit} disabled={!editName.trim()}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Workspace-level export dialog */}
      <Dialog
        open={exportWs !== null}
        onOpenChange={(o) => !o && setExportWs(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Download className="h-4 w-4 text-primary" />
              {t("workspace.exportWs")}
              {exportWs && (
                <span className="truncate text-sm font-normal text-muted-foreground">
                  {workspaces.find((w) => w.id === exportWs)?.name}
                </span>
              )}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <div className="text-xs text-muted-foreground">
                {t("export.spec")}
              </div>
              <div className="flex flex-col gap-1">
                {WS_SPECS.map((s) => {
                  const active = exportSpec === s.value;
                  return (
                    <button
                      key={s.value}
                      type="button"
                      onClick={() => setExportSpec(s.value)}
                      className={cn(
                        "flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs transition-colors",
                        active
                          ? "border-primary bg-primary/10 text-foreground"
                          : "border-border text-muted-foreground hover:bg-accent/10",
                      )}
                    >
                      <BrandIcon
                        icon={s.icon}
                        className="h-3.5 w-3.5 shrink-0"
                      />
                      {t(s.labelKey)}
                    </button>
                  );
                })}
              </div>
            </div>
            <div className="space-y-1.5">
              <div className="text-xs text-muted-foreground">
                {t("export.fileFormat")}
              </div>
              <div className="flex gap-1.5">
                {(["json", "yaml"] as const).map((f) => {
                  const active = exportFormat === f;
                  return (
                    <button
                      key={f}
                      type="button"
                      onClick={() => setExportFormat(f)}
                      className={cn(
                        "flex-1 rounded-md border px-2.5 py-1.5 text-xs font-medium uppercase transition-colors",
                        active
                          ? "border-primary bg-primary/10 text-foreground"
                          : "border-border text-muted-foreground hover:bg-accent/10",
                      )}
                    >
                      {f}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setExportWs(null)}
              disabled={exporting}
            >
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              onClick={doWorkspaceExport}
              disabled={exporting}
              className="gap-1.5"
            >
              {exporting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              {t("export.button")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
