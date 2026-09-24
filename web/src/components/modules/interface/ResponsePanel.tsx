// Response area: status bar (status code / duration / size), error banner, and body/headers/request/tests/script tabs.
// Manages its own local state related to this area (beautify toggle, request-code tool, copy feedback).
import { useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  Clock,
  Copy,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ResponseBodyViewer } from "./ResponseBodyViewer";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { TimingBreakdown } from "@/components/common/TimingBreakdown";
import { useT } from "@/lib/i18n";
import { isTauri } from "@/lib/bridge/client";
import { cn, methodColor } from "@/lib/utils";
import { buildRequestUrl, resolveTemplate } from "@/lib/resolve";
import { formatXml, getActiveBody, stripBodyComments } from "@/lib/requestBody";
import {
  generateRequestCode,
  REQUEST_CODE_TOOLS,
  type RequestCodeTool,
} from "@/lib/requestCode";
import { useAppStore } from "@/store/useStore";
import type { AssertionResult, HttpRequest, HttpResponse } from "@/data/types";
import { ScriptPanel } from "./ScriptPanel";
import { fileSize } from "@/lib/utils";

/** Whether the response is XML: Content-Type matches xml (incl. suffix forms like application/atom+xml),
 *  or, when missing/non-JSON, the body starts with `<` (e.g. an XML payload without Content-Type). */
function isXmlResponse(res: HttpResponse): boolean {
  const ct = (
    res.headers["content-type"] ??
    res.headers["Content-Type"] ??
    ""
  ).toLowerCase();
  if (ct.includes("json")) return false;
  if (ct.includes("xml")) return true;
  return /^\s*</.test(res.body);
}

/** The fully built request (for the "request" tab: url+query, resolved variables) */
interface BuiltRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
  bodyMode: HttpRequest["bodyMode"];
  formParams: HttpRequest["formParams"];
  binaryFile: { name: string; type: string; path?: string } | null;
}

export function ResponsePanel({
  req,
  response,
  responseError,
  assertions,
  responseTab,
  setResponseTab,
}: {
  req: HttpRequest;
  response: HttpResponse | null;
  responseError: string | null;
  assertions: AssertionResult[];
  responseTab: "body" | "headers" | "tests" | "request" | "script";
  setResponseTab: (
    t: "body" | "headers" | "tests" | "request" | "script",
  ) => void;
}) {
  const { t, format } = useT();
  const locale = useAppStore((s) => s.locale);
  const environments = useAppStore((s) => s.environments);
  const activeEnvId = useAppStore((s) => s.activeEnvId);
  const globalVariables = useAppStore((s) => s.globalVariables);
  const [builtRequest, setBuiltRequest] = useState<BuiltRequest | null>(null);
  // Response Body tab: beautify toggle
  const [prettyView, setPrettyView] = useState(true);
  // Request code: currently selected client tool + copy feedback
  const [codeTool, setCodeTool] = useState<RequestCodeTool>("curl");
  const [codeCopied, setCodeCopied] = useState(false);

  // Build a live request snapshot (url with query, resolved variables) for the "request" tab to show what was actually sent
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const env = environments.find((e) => e.id === activeEnvId);
      const vars: Record<string, string> = {
        ...globalVariables,
        ...(env?.variables ?? {}),
        // Consistent with real sending (requestRunner): merge secrets into the variable snapshot
        ...(env?.secrets ?? {}),
      };
      // Share the same URL-building logic as single-shot/load-test (lib/resolve.buildRequestUrl),
      // so the "request" tab shows exactly the URL actually sent
      const url = await buildRequestUrl(req, vars, locale);
      const headers: Record<string, string> = {};
      await Promise.all(
        req.headers
          .filter((h) => h.enabled && h.key)
          .map(async (h) => {
            headers[h.key] = await resolveTemplate(h.value, vars, locale);
          }),
      );
      // Strip comments (JSON // and /* */, XML <!-- -->) before generating the command, keeping only valid data
      const rawBody = getActiveBody(req);
      const strippedBody = rawBody
        ? stripBodyComments(rawBody, req.bodyMode)
        : "";
      const body = strippedBody
        ? await resolveTemplate(strippedBody, vars, locale)
        : "";
      if (!cancelled) {
        setBuiltRequest({
          method: req.method,
          url,
          headers,
          body,
          bodyMode: req.bodyMode,
          formParams: req.formParams ?? [],
          binaryFile: req.binaryFile
            ? {
                name: req.binaryFile.name,
                type: req.binaryFile.type,
                path: req.binaryFile.path,
              }
            : null,
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [req, environments, activeEnvId, globalVariables, locale]);

  const statusColor =
    response == null
      ? "text-muted-foreground"
      : response.status < 300
        ? "text-emerald-400"
        : response.status < 400
          ? "text-blue-400"
          : response.status < 500
            ? "text-amber-400"
            : "text-rose-400";

  let prettyBody = response?.body ?? "";
  if (response && prettyView) {
    if (isXmlResponse(response)) {
      // XML: same standard as the request-body "format" (leaf element values stay on one line)
      prettyBody = formatXml(response.body) ?? response.body;
    } else {
      try {
        prettyBody = JSON.stringify(JSON.parse(response.body), null, 2);
      } catch {
        prettyBody = response.body;
      }
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-9 items-center gap-3 border-b border-border px-3">
        {response ? (
          <>
            <span className={`font-mono text-sm font-bold ${statusColor}`}>
              {response.status}
            </span>
            <span className="text-xs text-muted-foreground">
              {response.statusText}
            </span>
            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="flex items-center gap-1 rounded px-1 text-xs text-muted-foreground transition-colors hover:bg-accent/30 hover:text-foreground"
                  title={t("response.timingHint")}
                >
                  <Clock className="h-3 w-3" />
                  {response.duration}ms
                </button>
              </PopoverTrigger>
              <PopoverContent align="start" className="w-72 p-3">
                <TimingBreakdown
                  timing={response.timing}
                  totalMs={response.duration}
                />
              </PopoverContent>
            </Popover>
            <span className="text-xs text-muted-foreground">
              {(response.size / 1024).toFixed(1)} KB
            </span>
          </>
        ) : (
          <span className="text-xs text-muted-foreground">
            {t("response.notSent")}
          </span>
        )}
      </div>

      {/* Error banner */}
      {responseError && (
        <div className="flex items-start gap-2 border-b border-border bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>
            {t("response.error.title")}：{responseError}
            {/* The "start backend" hint only makes sense in the browser preview: the desktop backend is always running,
                showing it unconditionally would misattribute real failures (params / script / network) to "backend not connected" */}
            {!isTauri() && (
              <>
                <br />
                {t("response.error.hint")}
              </>
            )}
          </span>
        </div>
      )}

      <Tabs
        value={responseTab}
        onValueChange={(v) => setResponseTab(v as typeof responseTab)}
        className="flex flex-1 flex-col min-h-0"
      >
        <div className="flex items-center justify-between border-b border-border pl-2 pr-3">
          <TabsList>
            {[
              ["body", t("response.body")],
              ["headers", t("response.headers")],
              ["request", t("response.request")],
              ["tests", t("response.tests")],
              ["script", t("response.script")],
            ].map(([k, label]) => (
              <TabsTrigger key={k} value={k}>
                {label}
              </TabsTrigger>
            ))}
          </TabsList>
          {responseTab === "body" && (
            <label className="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
              <Switch
                checked={prettyView}
                onCheckedChange={setPrettyView}
                aria-label={t("response.beautify")}
              />
              {t("response.beautify")}
            </label>
          )}
        </div>
        <div className="flex-1 min-h-0 overflow-auto">
          <TabsContent value="body" className="m-0">
            {prettyBody ? (
              <ResponseBodyViewer
                body={prettyBody}
                contentType={
                  response?.headers["content-type"] ??
                  response?.headers["Content-Type"]
                }
              />
            ) : (
              <pre className="p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words">
                —
              </pre>
            )}
          </TabsContent>
          <TabsContent value="headers" className="m-0">
            <div className="p-3 text-xs">
              {response ? (
                <Table className="text-xs">
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead className="h-7 w-1/3 text-xs">
                        {t("response.headerName")}
                      </TableHead>
                      <TableHead className="h-7 text-xs">
                        {t("response.headerValue")}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {Object.entries(response.headers).map(([k, v]) => (
                      <TableRow
                        key={k}
                        className="border-border/60 odd:bg-black/[0.045] dark:odd:bg-white/[0.055]"
                      >
                        <TableCell className="py-1 align-top font-mono font-medium break-all whitespace-normal">
                          {k}
                        </TableCell>
                        <TableCell className="py-1 align-top font-mono text-muted-foreground break-all whitespace-normal">
                          {v}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              ) : (
                <span className="text-muted-foreground">—</span>
              )}
            </div>
          </TabsContent>
          <TabsContent value="request" className="m-0">
            <div className="p-3 space-y-3">
              {(() => {
                const built = response?.request ?? builtRequest;
                if (!built) {
                  return (
                    <span className="text-xs text-muted-foreground">
                      {t("response.noRequest")}
                    </span>
                  );
                }
                return (
                  <>
                    <div>
                      <div className="text-sm font-semibold text-muted-foreground mb-1">
                        {t("response.requestUrl")}
                      </div>
                      <div className="flex items-center gap-2">
                        <span
                          className={cn(
                            "font-mono text-xs font-bold",
                            methodColor(built.method),
                          )}
                        >
                          {built.method}
                        </span>
                        <span className="font-mono text-xs break-all">
                          {built.url}
                        </span>
                      </div>
                    </div>
                    <div>
                      <div className="text-sm font-semibold text-muted-foreground mb-1">
                        {t("response.requestHeaders")}
                      </div>
                      {Object.keys(built.headers).length ? (
                        Object.entries(built.headers).map(([k, v]) => (
                          <div key={k} className="flex gap-2 py-0.5">
                            <span className="font-mono text-primary shrink-0 text-xs">
                              {k}:
                            </span>
                            <span className="font-mono text-muted-foreground text-xs break-all">
                              {v}
                            </span>
                          </div>
                        ))
                      ) : (
                        <span className="text-xs text-muted-foreground">—</span>
                      )}
                    </div>
                    <div>
                      <div className="text-sm font-semibold text-muted-foreground mb-1">
                        {t("response.requestBody")}
                      </div>
                      {built.body ? (
                        <pre className="font-mono text-xs leading-relaxed whitespace-pre-wrap rounded bg-background p-2 border border-border">
                          {built.body}
                        </pre>
                      ) : req.bodyMode === "binary" ? (
                        <span className="text-xs text-muted-foreground">
                          {req.binaryFile
                            ? format(
                                "response.binarySummary",
                                req.binaryFile.name,
                                req.binaryFile.data
                                  ? fileSize(req.binaryFile.data)
                                  : t("body.localFile"),
                              )
                            : "—"}
                        </span>
                      ) : req.bodyMode === "form-data" ? (
                        <span className="text-xs text-muted-foreground">
                          {format(
                            "response.formDataSummary",
                            (req.formParams ?? []).filter(
                              (p) => p.enabled && p.key,
                            ).length,
                            (req.formParams ?? []).filter((p) => p.file).length,
                          )}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">—</span>
                      )}
                    </div>
                    <div>
                      <div className="text-sm font-semibold text-muted-foreground mb-1">
                        {t("response.requestCode")}
                      </div>
                      <div className="flex items-center justify-between gap-2 mb-2">
                        <Tabs
                          value={codeTool}
                          onValueChange={(v) =>
                            setCodeTool(v as RequestCodeTool)
                          }
                          className="min-w-0 flex-1"
                        >
                          <TabsList className="h-7 flex-wrap">
                            {REQUEST_CODE_TOOLS.map((tool) => (
                              <TabsTrigger
                                key={tool.id}
                                value={tool.id}
                                className="px-2 text-xs"
                              >
                                {tool.label}
                              </TabsTrigger>
                            ))}
                          </TabsList>
                        </Tabs>
                        <div className="flex shrink-0 items-center gap-1">
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-8 px-2.5 text-xs"
                            onClick={() => {
                              const code = generateRequestCode(built, codeTool);
                              navigator.clipboard
                                ?.writeText(code)
                                .then(() => {
                                  setCodeCopied(true);
                                  setTimeout(() => setCodeCopied(false), 1500);
                                })
                                .catch(() => {});
                            }}
                          >
                            <Copy className="h-3.5 w-3.5 mr-1" />
                            {codeCopied ? t("common.copied") : t("common.copy")}
                          </Button>
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-8 px-1.5 text-xs"
                                title={t("response.copySingleHint")}
                              >
                                <ChevronDown className="h-3.5 w-3.5" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              <DropdownMenuItem
                                onClick={() => {
                                  const code = generateRequestCode(
                                    built,
                                    codeTool,
                                    false,
                                  );
                                  navigator.clipboard
                                    ?.writeText(code)
                                    .then(() => {
                                      setCodeCopied(true);
                                      setTimeout(
                                        () => setCodeCopied(false),
                                        1500,
                                      );
                                    })
                                    .catch(() => {});
                                }}
                              >
                                <Copy className="h-3.5 w-3.5 mr-1.5" />
                                {t("response.copySingle")}
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                      </div>
                      <pre className="font-mono text-xs leading-relaxed whitespace-pre-wrap break-words rounded bg-background p-2 border border-border">
                        {generateRequestCode(built, codeTool)}
                      </pre>
                    </div>
                  </>
                );
              })()}
            </div>
          </TabsContent>
          <TabsContent value="tests" className="m-0">
            <div className="p-3">
              {/* Assertions (built-in + DB/Redis) and post-script pm.test results, summarized together */}
              {(() => {
                const results: {
                  name: string;
                  passed: boolean;
                  message: string;
                  group: string;
                }[] = [
                  ...assertions.map((a) => ({ ...a, group: "assert" })),
                  ...(response?.postTests ?? []).map((a) => ({
                    name: a.name,
                    passed: a.passed,
                    message: a.message,
                    group: "script",
                  })),
                ];
                const passed = results.filter((r) => r.passed).length;
                return (
                  <div className="space-y-1.5">
                    {results.length > 0 && (
                      <div className="mb-2 text-xs text-muted-foreground">
                        {format("response.passedCount", passed, results.length)}
                      </div>
                    )}
                    {results.length === 0 && (
                      <div className="text-xs text-muted-foreground">
                        {t("response.noAssertions")}
                      </div>
                    )}
                    {results.map((a, idx) => (
                      <div
                        key={idx}
                        className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-xs break-all"
                      >
                        {a.passed ? (
                          <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />
                        ) : (
                          <XCircle className="h-4 w-4 shrink-0 text-rose-400" />
                        )}
                        <span
                          className={
                            a.group === "script"
                              ? "shrink-0 rounded bg-muted px-1.5 text-[10px] text-muted-foreground"
                              : "shrink-0 rounded bg-emerald-500/10 px-1.5 text-[10px] text-emerald-400"
                          }
                        >
                          {a.group === "script"
                            ? t("response.groupScript")
                            : t("response.groupAssert")}
                        </span>
                        <span className="flex-1">{a.name}</span>
                        <span className="text-muted-foreground">
                          {a.message}
                        </span>
                      </div>
                    ))}
                  </div>
                );
              })()}
            </div>
          </TabsContent>
          <TabsContent value="script" className="m-0">
            <ScriptPanel
              preLogs={response?.preLogs}
              postLogs={response?.postLogs}
              postTests={response?.postTests}
            />
          </TabsContent>
        </div>
      </Tabs>
    </div>
  );
}
