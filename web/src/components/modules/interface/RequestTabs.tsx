// Request builder area: params / headers / body / auth / pre-post scripts / examples / mock tabs.
// Manages its own local state related to this area (auto-header sync, example expand, schema dialog, script-reference dialog, etc.).
import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Eye,
  EyeOff,
  Lock,
  Shrink,
  Sparkles,
  Terminal,
  Wand2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  KeyValueEditor,
  type CommonKey,
} from "@/components/common/KeyValueEditor";
import { DynamicValuePicker } from "@/components/common/DynamicValuePicker";
import { FormDataEditor } from "@/components/common/FormDataEditor";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { extractPathParamNames } from "@/lib/resolve";
import {
  formatJsonTolerant,
  formatXml,
  getActiveBody,
  minifyJsonTolerant,
  minifyXml,
} from "@/lib/requestBody";
import { generateModelBody } from "@/lib/generate";
import { useAppStore } from "@/store/useStore";
import { uid } from "@/data/seed";
import type {
  BodyMode,
  DataModel,
  HttpRequest,
  KeyValue,
  Locale,
} from "@/data/types";
import { BinaryBodyEditor } from "./BinaryBodyEditor";
import { AssertionEditor } from "@/components/common/AssertionEditor";
import { AuthEditor } from "./AuthEditor";
import { MockInterfaceEditor } from "./MockEditor";
import { ResponseSchemaDialog } from "./ResponseSchemaDialog";
import { modelToSchema } from "@/lib/modelSchema";
import { BodyEditor, type BodyEditorHandle } from "./BodyEditor";
import { ReferenceDialog } from "@/components/common/ReferenceDialog";
import { RequestActionsEditor } from "./actions/RequestActionsEditor";
import { ScriptTimingHint } from "./actions/ScriptTimingHint";
import { getPostActions, getPreActions } from "@/lib/requestActions";
import type { DataSourceOption } from "@/lib/dbQuery";

const COMMON_HEADERS: CommonKey[] = [
  { key: "Content-Type", value: "application/json" },
  { key: "Accept", value: "application/json" },
  { key: "Authorization", value: "Bearer {{token}}" },
  { key: "User-Agent", value: "Orbit/1.0" },
  { key: "Accept-Language", value: "zh-CN,zh;q=0.9" },
  // No Accept-Encoding shortcut: it would make the exported curl carry --compressed, which some servers reject
  { key: "Connection", value: "keep-alive" },
  { key: "Cache-Control", value: "no-cache" },
  { key: "Referer", value: "" },
  { key: "Origin", value: "" },
  { key: "X-Requested-With", value: "XMLHttpRequest" },
  { key: "X-API-Key", value: "{{api_key}}" },
  { key: "Cookie", value: "" },
];

export function RequestTabs({
  req,
  locale,
  models,
  requestTab,
  setRequestTab,
}: {
  req: HttpRequest;
  locale: Locale;
  models: DataModel[];
  requestTab:
    | "params"
    | "headers"
    | "body"
    | "auth"
    | "prescript"
    | "postscript"
    | "assertions"
    | "examples"
    | "mock";
  setRequestTab: (t: typeof requestTab) => void;
}) {
  const { t, format } = useT();
  const updateDraft = useAppStore((s) => s.updateDraft);
  const environments = useAppStore((s) => s.environments);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const globalVariables = useAppStore((s) => s.globalVariables);
  // Data-source options for DB actions + the latest execution results (action-card status badge)
  const dataSources = useAppStore((s) => s.dataSources) as DataSourceOption[];
  const response = useAppStore((s) => s.response);
  // Script-library table: both the "reference from library" entry and reference cards rely on it to resolve library items
  const actionTemplates = useAppStore((s) => s.actionTemplates);
  const setActiveModule = useAppStore((s) => s.setActiveModule);
  // Variable sources for script autocompletion: global variables + current environment variables + secrets ({{name}} completion)
  const activeEnv = environments.find((e) => e.id === activeEnvId);
  const scriptVariables = [
    ...Object.keys(globalVariables ?? {}),
    ...Object.keys(activeEnv?.variables ?? {}),
    ...Object.keys(activeEnv?.secrets ?? {}),
  ];
  // Pre-actions = a single ordered list (includes the built-in interpolation node); post-actions are a separate list.
  // The old single-script fields are normalized into the list; last version's "pre-interpolation actions" field is also merged before the anchor.
  const preActions = getPreActions(req);
  const postActions = getPostActions(req);
  /** Script-syntax reference entry: rendered into the action list toolbar (shared by pre/post), on the same row as "add action" / "ⓘ info" */
  const scriptRefButton = (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 px-2 text-xs"
      onClick={() => setScriptRefOpen(true)}
    >
      <Terminal className="h-3.5 w-3.5 mr-1" />
      {t("script.reference")}
    </Button>
  );
  const bodyRef = useRef<BodyEditorHandle>(null);
  // Request headers: whether to show "auto default headers" together with user headers (hidden by default; off collapses auto headers)
  const [showAutoHeaders, setShowAutoHeaders] = useState(false);
  // Script-syntax reference dialog
  const [scriptRefOpen, setScriptRefOpen] = useState(false);
  // Record each auto header's "last generated value", to tell user-modified values apart from still-default ones
  const prevAutoRef = useRef<Record<string, string>>({});
  // Response examples tab: each response expands/collapses independently (all can be collapsed).
  // Only on the "first open of the examples tab" does the first one expand by default; afterwards the user's state is fully preserved (including request switches).
  const [openRespIds, setOpenRespIds] = useState<Set<string>>(new Set());
  const initRespRef = useRef(false);
  // View data-model dialog (shared by response examples / request-body model): title + schema
  const [schemaDialog, setSchemaDialog] = useState<{
    title: string;
    schema: any;
  } | null>(null);

  // Unified update entry: write draft and mark dirty
  const set = (patch: Partial<HttpRequest>, opts?: { silent?: boolean }) =>
    updateDraft(req.id, patch, opts);

  useEffect(() => {
    if (requestTab === "examples" && !initRespRef.current) {
      const first = req?.responses?.[0];
      if (first) {
        initRespRef.current = true;
        setOpenRespIds(new Set([first.id]));
      }
    }
  }, [requestTab, req]);

  // On url change, sync the path-param list: names are derived from the `{name}` placeholders in the url (read-only),
  // user-edited values are preserved; placeholders no longer present in the url are removed from the list.
  useEffect(() => {
    const names = extractPathParamNames(req.url);
    const existing = new Map((req.pathParams ?? []).map((p) => [p.key, p]));
    const next: KeyValue[] = names.map((n) => {
      const prev = existing.get(n);
      return prev
        ? { ...prev, key: n }
        : { id: uid("kv"), key: n, value: "", enabled: true };
    });
    const cur = req.pathParams ?? [];
    const changed =
      next.length !== cur.length ||
      cur.some((p, i) => next[i]?.key !== p.key || next[i]?.value !== p.value);
    if (changed) set({ pathParams: next }, { silent: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req?.url]);

  // Extract the path from the request URL (strip host, template variables, query params)
  const extractPath = (url: string): string => {
    // Drop query params
    const noQuery = url.split("?")[0];
    if (noQuery.startsWith("http")) {
      try {
        // new URL().pathname would percent-encode `{id}` into `%7Bid%7D`,
        // so it must be decoded back, otherwise the Mock interface's path template can't be recognized by the backend
        return decodeURIComponent(new URL(noQuery).pathname);
      } catch {
        return "/";
      }
    }
    // Find the first "/", skipping template variables like {{base_url}}
    const slashIdx = noQuery.indexOf("/");
    if (slashIdx < 0) return "/";
    return noQuery.slice(slashIdx);
  };

  // Find whether the current request already has a matching Mock interface (route key = method + path)
  const currentPath = req ? extractPath(req.url) : "/";

  // Insert text at the Body editor's cursor (via CodeMirror insertAtCursor; falls back to append when not mounted)
  const insertBody = (text: string) => {
    const el = bodyRef.current;
    if (el && typeof el.insertAtCursor === "function") {
      el.insertAtCursor(text);
      return;
    }
    const body = getActiveBody(req);
    set({
      bodyByMode: { ...(req.bodyByMode ?? {}), [req.bodyMode]: body + text },
    });
  };

  // Sync auto default headers into req.headers (auto:true) while respecting user edits:
  // - when a source (bodyMode / contentType / cookies) changes, only update auto headers that are still at their default value;
  // - auto headers whose value the user manually changed stay unchanged;
  // - when a source disappears (e.g. Content-Type for bodyMode=none, or cleared Cookie), remove the corresponding auto header.
  useEffect(() => {
    const desired: { key: string; value: string }[] = [
      { key: "Accept", value: "*/*" },
      // Accept-Encoding is no longer a default header (avoids the exported curl carrying --compressed, which some servers reject)
      { key: "User-Agent", value: "Orbit/1.0" },
      { key: "Connection", value: "keep-alive" },
      { key: "Cache-Control", value: "no-cache" },
    ];
    const cookie = req.cookies
      .filter((c) => c.enabled && c.name)
      .map((c) => `${c.name}=${c.value}`)
      .join("; ");
    if (cookie) desired.push({ key: "Cookie", value: cookie });

    const desiredKeys = new Set(desired.map((d) => d.key.toLowerCase()));
    const prev = prevAutoRef.current;
    const next = [...req.headers];
    let changed = false;

    for (const d of desired) {
      const key = d.key.toLowerCase();
      const idx = next.findIndex((h) => h.key.toLowerCase() === key);
      if (idx === -1) {
        next.push({
          id: uid("kv"),
          key: d.key,
          value: d.value,
          enabled: true,
          auto: true,
        });
        changed = true;
      } else if (next[idx].auto) {
        // Only sync to the latest generated value when the current value is still the last generated one (user hasn't changed it)
        if (prev[key] === next[idx].value && next[idx].value !== d.value) {
          next[idx] = { ...next[idx], value: d.value };
          changed = true;
        }
      }
      prev[key] = d.value;
    }

    // Remove auto headers whose source disappeared
    for (let i = next.length - 1; i >= 0; i--) {
      const h = next[i];
      if (h.auto && h.key && !desiredKeys.has(h.key.toLowerCase())) {
        next.splice(i, 1);
        delete prev[h.key.toLowerCase()];
        changed = true;
      }
    }

    if (changed) set({ headers: next }, { silent: true });
    // Only re-sync when a source changes, to avoid a loop with user edits
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req?.id, req?.bodyMode, req?.contentType, req?.cookies]);

  // Header editing: auto default headers and user headers are merged into one editor; hidden mode collapses auto headers
  const handleHeadersChange = (items: KeyValue[]) => {
    if (showAutoHeaders) {
      set({ headers: items });
    } else {
      const auto = req.headers.filter((h) => h.auto);
      set({ headers: [...auto, ...items] });
    }
  };

  const tryFormat = () => {
    const current = getActiveBody(req);
    if (req.bodyMode === "xml") {
      const out = formatXml(current);
      if (out != null)
        set({ bodyByMode: { ...(req.bodyByMode ?? {}), xml: out } });
      return;
    }
    // json / others: tolerate {{var}} when beautifying JSON
    const out = formatJsonTolerant(current);
    if (out != null)
      set({ bodyByMode: { ...(req.bodyByMode ?? {}), [req.bodyMode]: out } });
  };

  // Minify: json → single line; xml → strip whitespace between tags
  const tryMinify = () => {
    const current = getActiveBody(req);
    if (req.bodyMode === "xml") {
      const out = minifyXml(current);
      if (out != null)
        set({ bodyByMode: { ...(req.bodyByMode ?? {}), xml: out } });
      return;
    }
    const out = minifyJsonTolerant(current);
    if (out != null)
      set({ bodyByMode: { ...(req.bodyByMode ?? {}), [req.bodyMode]: out } });
  };

  return (
    <Tabs
      value={requestTab}
      onValueChange={(v) => setRequestTab(v as typeof requestTab)}
      className="flex h-full flex-col"
    >
      <div className="border-b border-border px-2">
        <TabsList>
          {[
            ["params", t("request.params")],
            ["headers", t("request.headers")],
            ["body", t("request.body")],
            ["auth", t("request.auth")],
            ["prescript", t("request.prescript")],
            ["postscript", t("request.postscript")],
            ["assertions", t("request.assertions")],
            ["examples", t("request.examples")],
            ["mock", "Mock"],
          ].map(([k, label]) => {
            // Pre/post tabs show the action count; the assertions tab shows the enabled assertion count
            // Script count excludes the built-in interpolation node (it is not a user action)
            const scriptCount =
              k === "prescript"
                ? preActions.filter((a) => a.kind !== "interpolate").length
                : k === "postscript"
                  ? postActions.length
                  : k === "assertions"
                    ? (req.assertions ?? []).filter(
                        (a) => a.meta?.enabled !== false,
                      ).length
                    : 0;
            return (
              <TabsTrigger key={k} value={k}>
                {label}
                {scriptCount > 0 && (
                  <span
                    className={cn(
                      "ml-1 rounded-full px-1 text-xs font-medium leading-3.5",
                      k === "assertions"
                        ? "bg-emerald-500/15 text-emerald-400"
                        : "bg-fuchsia-500/15 text-fuchsia-400",
                    )}
                    title={
                      k === "prescript"
                        ? t("script.hasPre")
                        : k === "postscript"
                          ? t("script.hasPost")
                          : undefined
                    }
                  >
                    {scriptCount}
                  </span>
                )}
              </TabsTrigger>
            );
          })}
        </TabsList>
      </div>

      <ScrollArea className="flex-1">
        {/* body tab: content fills the viewport height (editor fills); other tabs stay scrollable */}
        <div
          className={requestTab === "body" ? "flex h-full flex-col p-3" : "p-3"}
        >
          <TabsContent value="params">
            <div className="space-y-3">
              {(req.pathParams ?? []).length > 0 && (
                <div>
                  <div className="mb-1 flex items-center justify-between">
                    <span className="text-sm font-semibold text-muted-foreground">
                      {t("request.pathParams")}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {t("request.pathParamsHint")}
                    </span>
                  </div>
                  <div className="space-y-1.5">
                    {(req.pathParams ?? []).map((p) => (
                      <div key={p.key} className="flex items-center gap-1.5">
                        <span
                          title={t("request.pathParamsLocked")}
                          className="flex w-28 shrink-0 items-center gap-1 rounded border border-border bg-muted/40 px-2 py-1.5 font-mono text-xs text-foreground"
                        >
                          <Lock className="h-3 w-3 shrink-0 text-muted-foreground" />
                          <span className="truncate">{p.key}</span>
                        </span>
                        <div className="flex flex-1 items-center gap-1">
                          <Input
                            value={p.value}
                            placeholder={t("kv.valuePlaceholder") as string}
                            className="h-8 flex-1 font-mono text-xs"
                            onChange={(e) =>
                              set({
                                pathParams: (req.pathParams ?? []).map((x) =>
                                  x.key === p.key
                                    ? { ...x, value: e.target.value }
                                    : x,
                                ),
                              })
                            }
                          />
                          <DynamicValuePicker
                            onInsert={(token) =>
                              set({
                                pathParams: (req.pathParams ?? []).map((x) =>
                                  x.key === p.key
                                    ? {
                                        ...x,
                                        value: x.value
                                          ? `${x.value}{{${token}}}`
                                          : `{{${token}}}`,
                                      }
                                    : x,
                                ),
                              })
                            }
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              <div>
                <div className="mb-1 text-sm font-semibold text-muted-foreground">
                  {t("request.queryParams")}
                </div>
                <KeyValueEditor
                  items={req.queryParams}
                  enableDynamic
                  onChange={(v) => set({ queryParams: v })}
                />
              </div>
            </div>
          </TabsContent>
          <TabsContent value="headers">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-sm font-semibold text-muted-foreground">
                {t("request.headers")}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 gap-1 px-2 text-xs text-muted-foreground"
                onClick={() => setShowAutoHeaders((v) => !v)}
              >
                {showAutoHeaders ? (
                  <Eye className="h-3.5 w-3.5" />
                ) : (
                  <EyeOff className="h-3.5 w-3.5" />
                )}
                {showAutoHeaders
                  ? t("request.hideAutoHeaders")
                  : t("request.showAutoHeaders")}
                <span className="ml-0.5 rounded bg-muted px-1 text-xs">
                  {req.headers.filter((h) => h.auto).length}
                </span>
              </Button>
            </div>
            <KeyValueEditor
              items={
                showAutoHeaders
                  ? req.headers
                  : req.headers.filter((h) => !h.auto)
              }
              enableDynamic
              commonKeys={COMMON_HEADERS}
              onChange={handleHeadersChange}
            />
          </TabsContent>
          <TabsContent value="body" className="flex min-h-0 flex-1 flex-col">
            <div className="mb-2 flex items-center gap-2">
              <Select
                value={req.bodyMode}
                onValueChange={(v) => set({ bodyMode: v as BodyMode })}
              >
                <SelectTrigger className="h-8 w-44">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("body.none")}</SelectItem>
                  <SelectItem value="json">json</SelectItem>
                  <SelectItem value="xml">xml</SelectItem>
                  <SelectItem value="form-data">form-data</SelectItem>
                  <SelectItem value="x-www-form-urlencoded">
                    x-www-form-urlencoded
                  </SelectItem>
                  <SelectItem value="raw">raw</SelectItem>
                  <SelectItem value="binary">{t("body.binary")}</SelectItem>
                </SelectContent>
              </Select>
              {(req.bodyMode === "json" || req.bodyMode === "xml") && (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={tryFormat}
                    title={t("request.format")}
                  >
                    <Wand2 className="h-3.5 w-3.5" /> {t("request.format")}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={tryMinify}
                    title={t("request.minify")}
                  >
                    <Shrink className="h-3.5 w-3.5" /> {t("request.minify")}
                  </Button>
                </>
              )}
              {(req.bodyMode === "json" ||
                req.bodyMode === "raw" ||
                req.bodyMode === "xml") && (
                <DynamicValuePicker onInsert={(t) => insertBody(`{{${t}}}`)} />
              )}
              {req.bodyMode === "json" &&
                models.length > 0 &&
                (req.modelId &&
                models.some((x: DataModel) => x.id === req.modelId) ? (
                  <>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={async () => {
                        const m = models.find(
                          (x: DataModel) => x.id === req.modelId,
                        )!;
                        if (m)
                          set({
                            bodyByMode: {
                              ...(req.bodyByMode ?? {}),
                              json: await generateModelBody(m),
                            },
                            bodyMode: "json",
                          });
                      }}
                    >
                      <Sparkles className="h-3.5 w-3.5" />{" "}
                      {t("request.autoGen")}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      className="gap-1"
                      onClick={() => {
                        const m = models.find(
                          (x: DataModel) => x.id === req.modelId,
                        )!;
                        if (m)
                          setSchemaDialog({
                            title: m.name,
                            schema: modelToSchema(m),
                          });
                      }}
                    >
                      {t("request.viewSchema")}
                    </Button>
                  </>
                ) : (
                  <Popover>
                    <PopoverTrigger asChild>
                      <Button variant="ghost" size="sm">
                        <Sparkles className="h-3.5 w-3.5" />{" "}
                        {t("request.autoGen")}
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent align="start" className="w-64 p-0">
                      <div className="border-b border-border px-3 py-2 text-xs font-medium text-muted-foreground">
                        {t("request.genByModel")}
                      </div>
                      <ScrollArea className="h-64">
                        <div className="p-1.5">
                          {models.map((m: DataModel) => (
                            <button
                              key={m.id}
                              onClick={async () => {
                                set({
                                  bodyByMode: {
                                    ...(req.bodyByMode ?? {}),
                                    json: await generateModelBody(m),
                                  },
                                  bodyMode: "json",
                                });
                              }}
                              className="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-xs hover:bg-accent/15"
                            >
                              <span className="font-medium">{m.name}</span>
                              <span className="text-xs text-muted-foreground">
                                {format("model.fieldCount", m.fields.length)}
                              </span>
                            </button>
                          ))}
                        </div>
                      </ScrollArea>
                    </PopoverContent>
                  </Popover>
                ))}
            </div>
            {req.bodyMode === "none" ? (
              <div className="text-xs text-muted-foreground">
                {t("request.noBody")}
              </div>
            ) : req.bodyMode === "binary" ? (
              <BinaryBodyEditor req={req} set={set} />
            ) : req.bodyMode === "form-data" ||
              req.bodyMode === "x-www-form-urlencoded" ? (
              <FormDataEditor
                items={req.formParams ?? []}
                mode={req.bodyMode === "form-data" ? "form-data" : "urlencoded"}
                enableDynamic
                onChange={(items) => set({ formParams: items })}
              />
            ) : (
              <BodyEditor
                ref={bodyRef}
                value={getActiveBody(req)}
                onChange={(v) =>
                  set({
                    bodyByMode: {
                      ...(req.bodyByMode ?? {}),
                      [req.bodyMode]: v,
                    },
                  })
                }
                mode={
                  req.bodyMode === "json"
                    ? "json"
                    : req.bodyMode === "xml"
                      ? "xml"
                      : "raw"
                }
                variables={scriptVariables}
                fill
              />
            )}
          </TabsContent>
          <TabsContent value="auth">
            <AuthEditor req={req} set={set} locale={locale} />
          </TabsContent>
          <TabsContent value="prescript">
            {/* A single ordered list: items before the built-in "interpolation" node = pre-interpolation, after = post-interpolation.
               The entry points live on one row of the action-list toolbar: |syntax reference|add action|ⓘ| */}
            <RequestActionsEditor
              value={preActions}
              // Editing migrates immediately: write the single list and clear the old single-script field, so a cleared list doesn't keep running the old script
              onChange={(next) => set({ preActions: next, prereqScript: "" })}
              kind="pre"
              allowInterpolate
              variables={scriptVariables}
              dataSources={dataSources}
              results={response?.preActions}
              templates={actionTemplates}
              onOpenLibrary={() => setActiveModule("actionlib")}
              headerLeading={scriptRefButton}
              headerTrailing={<ScriptTimingHint kind="pre" />}
            />
          </TabsContent>
          <TabsContent value="postscript">
            <RequestActionsEditor
              value={postActions}
              onChange={(next) => set({ postActions: next, postreqScript: "" })}
              kind="post"
              variables={scriptVariables}
              dataSources={dataSources}
              results={response?.postActions}
              templates={actionTemplates}
              onOpenLibrary={() => setActiveModule("actionlib")}
              headerLeading={scriptRefButton}
              headerTrailing={<ScriptTimingHint kind="post" />}
            />
          </TabsContent>
          <TabsContent value="assertions">
            <AssertionEditor
              value={req.assertions ?? []}
              onChange={(next) => set({ assertions: next })}
            />
          </TabsContent>
          <TabsContent value="examples">
            {req.responses && req.responses.length > 0 ? (
              <div className="space-y-2">
                {req.responses.map((r) => {
                  // Pure Set check: each response is independent, all may be collapsed, no auto-fallback
                  const isOpen = openRespIds.has(r.id);
                  return (
                    <div
                      key={r.id}
                      className="overflow-hidden rounded-md border border-border"
                    >
                      <div className="flex items-center gap-1">
                        <button
                          type="button"
                          onClick={() =>
                            setOpenRespIds((prev) => {
                              const next = new Set(prev);
                              if (next.has(r.id)) next.delete(r.id);
                              else next.add(r.id);
                              return next;
                            })
                          }
                          className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left hover:bg-accent/10"
                        >
                          {isOpen ? (
                            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                          )}
                          <Badge variant="outline">{r.status}</Badge>
                          <span className="min-w-0 flex-1 truncate text-sm font-medium">
                            {r.name}
                          </span>
                        </button>
                        {r.schema && (
                          <Button
                            variant="outline"
                            size="sm"
                            className="mr-1 h-6 shrink-0 gap-1 px-1.5 text-xs"
                            onClick={() =>
                              setSchemaDialog({
                                title: `${r.status} ${r.name}`,
                                schema: r.schema,
                              })
                            }
                          >
                            {t("request.viewSchema")}
                          </Button>
                        )}
                      </div>
                      {isOpen && (
                        <div className="border-t border-border px-2 py-2">
                          <pre className="overflow-auto rounded bg-background p-2 font-mono text-xs text-muted-foreground">
                            {r.body}
                          </pre>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="text-xs text-muted-foreground">
                {t("request.noExamples")}
              </div>
            )}
          </TabsContent>
          <TabsContent value="mock">
            <MockInterfaceEditor req={req} path={currentPath} />
          </TabsContent>
        </div>
      </ScrollArea>

      {/* Interface reference dialog (dynamic-value / script dual tabs; opens on the script tab when launched from the script-syntax reference entry) */}
      <ReferenceDialog
        open={scriptRefOpen}
        onOpenChange={setScriptRefOpen}
        initialTab="script"
      />

      {/* Data-model view dialog (shared by response examples / request-body model) */}
      {schemaDialog && (
        <ResponseSchemaDialog
          title={schemaDialog.title}
          schema={schemaDialog.schema}
          open
          onOpenChange={(v) => {
            if (!v) setSchemaDialog(null);
          }}
          t={t}
        />
      )}
    </Tabs>
  );
}
