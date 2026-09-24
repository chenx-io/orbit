// Request executor: sends the current request to the engine as an "un-interpolated template" and writes back the response/assertions.
//
// **Variable interpolation and request assembly all happen in the Rust engine** (no TS-side pre-substitution in the frontend):
// this is what lets variables written by pre-interpolation scripts take part in the current interpolation, and lets one-off
// debugging share one pipeline with scenarios / load tests (the displayed snapshot is what the engine actually sent).
import type { StateCreator } from "zustand";
import type { AssertionResult, HttpResponse, SentRequest } from "@/data/types";
import { byteLength } from "@/lib/request";
import { buildRequestTemplate } from "@/lib/requestTemplate";
import { t, tFormat } from "@/lib/localeDict";
import { executeRequest, type ProxyResponse } from "@/lib/bridge";
import { executeOnAgent, listAgents } from "@/lib/bridge/distributed";
import {
  actionsToLegacyScript,
  getPostActions,
  getPreActions,
  hasDbAction,
} from "@/lib/requestActions";
import type { AppState } from "../types";
import { isHttpRequest } from "@/data/types";

export interface RequestRunnerSlice {
  response: HttpResponse | null;
  responseError: string | null;
  assertionResults: AssertionResult[];
  loading: boolean;
  responseTab: "body" | "headers" | "tests" | "request" | "script";

  sendRequest: (requestId?: string) => Promise<void>;
  setResponseTab: (t: RequestRunnerSlice["responseTab"]) => void;
}

export const createRequestRunnerSlice: StateCreator<
  AppState,
  [],
  [],
  RequestRunnerSlice
> = (set, get) => ({
  response: null,
  responseError: null,
  assertionResults: [],
  loading: false,
  responseTab: "body",

  sendRequest: async (requestId) => {
    const s = get();
    const activeTab = s.tabs.find((t) => t.id === s.activeTabId);
    const rid = requestId ?? activeTab?.requestId;
    const req = rid ? (s.drafts[rid] ?? s.requests[rid]) : null;
    if (!req) return;
    // One-off debugging is HTTP-only for now; run other protocols through an automation scenario (/api/run)
    if (!isHttpRequest(req)) {
      set({
        loading: false,
        responseError: t("request.debug.unsupportedProtocol"),
      });
      return;
    }
    set({ loading: true });
    get().track("request_send");

    const env = s.environments.find((e) => e.id === s.activeEnvId);
    const vars: Record<string, string> = {
      ...s.globalVariables,
      ...(env?.variables ?? {}),
      // Secrets are merged into the variable snapshot: usable both for {{secret}} template resolution
      // and by scripts via pm.environment.get / pm.secret.get (scripts may read but never write them).
      ...(env?.secrets ?? {}),
    };

    // Pre/post actions (scripts / database queries / built-in interpolation node); legacy single-script fields are normalized into the action list.
    // Pre-actions form one ordered list: before the built-in interpolation node = pre-interpolation, after it = post-interpolation.
    const preActions = getPreActions(req);
    const postActions = getPostActions(req);

    // Un-interpolated request template: placeholders (`{{var}}` / `{{$...}}`) and structured params are handed to the engine as-is,
    // which then runs "actions before the anchor → interpolation + assembly encoding → actions after the anchor" in list order.
    // **The frontend no longer substitutes variables** — otherwise variables produced by pre-interpolation scripts could not take part in this interpolation.
    const requestTemplate = buildRequestTemplate(req);

    // The "Request" tab snapshot comes from what the engine returns (displayed = actually sent); falls back to the request config otherwise
    const buildSnapshot = (from: ProxyResponse["request"]): SentRequest => ({
      method: from?.method ?? req.method,
      url: from?.url ?? req.url,
      headers: from?.headers ?? {},
      body: from?.body ?? "",
      bodyMode: req.bodyMode,
      formParams: req.formParams,
      binaryFile: req.binaryFile
        ? {
            name: req.binaryFile.name,
            type: req.binaryFile.type,
            path: req.binaryFile.path,
          }
        : null,
    });

    let proxy: ProxyResponse;
    try {
      const target = s.executionTarget;
      if (target.mode === "agent") {
        // A distributed agent only sends the final message: the whole pre-action list (including the built-in node) runs on the **control side**
        // — actions before the anchor, interpolation and assembly, and signing after it all happen here; the agent never interpolates.
        const resolved = await executeRequest({
          method: req.method,
          url: req.url,
          requestTemplate,
          preActions: preActions.length ? preActions : null,
          // Script library table: used on the control side to expand references (the agent never interpolates)
          actionTemplates: s.actionTemplates.length ? s.actionTemplates : null,
          envVars: vars,
          dryRun: true,
        });
        const builtRequest = resolved.request;
        if (!builtRequest) {
          throw new Error(t("request.distributed.resolveFailed"));
        }
        // The agent protocol carries a single post-response script only: post-response DB actions are skipped with a warning (never silently dropped)
        if (hasDbAction(postActions)) {
          console.warn(
            "[requestRunner] distributed agents do not support post-response database actions yet; skipped",
          );
        }
        // A single request can only be routed to one agent: prefer the first selected one, or the first idle agent for "all"
        let agentId: string | null =
          target.agentIds && target.agentIds.length > 0
            ? target.agentIds[0]
            : null;
        if (!agentId) {
          const agents = await listAgents();
          const idle = agents.find((a) => a.state === "idle");
          agentId = idle?.id ?? null;
        }
        if (!agentId) {
          throw new Error(t("request.distributed.noAgent"));
        }
        const result = await executeOnAgent(agentId, {
          method: builtRequest.method,
          url: builtRequest.url,
          headers: builtRequest.headers,
          body: builtRequest.body,
          // Pre-actions (including signing after the anchor) already ran on the control side; sending them again would duplicate work
          prereqScript: null,
          postreqScript: actionsToLegacyScript(postActions) ?? null,
          envVars: vars,
        });
        if (result.error) throw new Error(result.error);
        proxy = {
          status: result.status,
          statusText: String(result.status),
          headers: result.headers,
          body: result.body,
          duration: result.duration_ms,
          size: byteLength(result.body),
          timing: result.timing
            ? {
                dns: result.timing.dns_ms,
                connect: result.timing.tcp_ms,
                tls: result.timing.tls_ms,
                ttfb: result.timing.ttfb_ms,
                download: result.timing.download_ms,
              }
            : undefined,
          // Control-side resolution logs (including the built-in node) come first, agent-side (post-response script) logs after
          preLogs: [...(resolved.preLogs ?? []), ...(result.pre_logs ?? [])],
          postLogs: result.post_logs ?? [],
          postTests: (result.post_tests ?? []).map((t) => ({
            name: t.name,
            passed: t.passed,
            message: t.message,
          })),
          varsSet: resolved.varsSet,
          tempVarsSet: resolved.tempVarsSet,
          preActions: resolved.preActions,
          actionVars: resolved.actionVars,
          request: builtRequest,
        };
      } else {
        proxy = await executeRequest({
          method: req.method,
          url: req.url,
          requestTemplate,
          // One ordered list (including the built-in node): before the anchor = pre-interpolation, after it = post-interpolation
          preActions: preActions.length ? preActions : null,
          postActions: postActions.length ? postActions : null,
          // Script library table: the engine expands `{ type: ref }` from it (same source as the action list, so the entries are always current)
          actionTemplates: s.actionTemplates.length ? s.actionTemplates : null,
          // When the action list is non-empty the legacy single-script field is not sent, avoiding "the list is empty/disabled but the old script still runs"
          prereqScript: preActions.length ? null : (req.prereqScript ?? null),
          postreqScript: postActions.length
            ? null
            : (req.postreqScript ?? null),
          envVars: vars,
          // Post-response assertions (built-in + DB/Redis), evaluated by the backend after the response
          assertions: req.assertions?.length ? req.assertions : null,
        });
      }
    } catch (e) {
      // The real backend call failed: report it explicitly instead of falling back to fake data
      set({
        loading: false,
        responseError: e instanceof Error ? e.message : String(e),
      });
      return;
    }

    // What the "Request" tab and generated code show = the final request snapshot returned by the engine
    const effectiveSent: SentRequest = buildSnapshot(proxy.request);

    const response: HttpResponse = {
      status: proxy.status,
      statusText: proxy.statusText,
      headers: proxy.headers,
      body: proxy.body,
      duration: proxy.duration,
      size: proxy.size,
      timing: proxy.timing,
      request: effectiveSent,
      preLogs: proxy.preLogs,
      postLogs: proxy.postLogs,
      postTests: proxy.postTests,
      varsSet: proxy.varsSet,
      preActions: proxy.preActions,
      postActions: proxy.postActions,
      actionVars: proxy.actionVars,
    };

    // Assertion results: prefer what the backend returns (built-in + DB/Redis evaluated after the response);
    // when no assertion is configured, keep a single "status code < 400" fallback so the Tests tab is never blank
    const assertions: AssertionResult[] =
      proxy.assertions && proxy.assertions.length > 0
        ? proxy.assertions
        : [
            {
              name: t("assert.statusCode"),
              passed: proxy.status < 400,
              message: tFormat("assert.actual", proxy.status),
            },
          ];

    set({
      response,
      responseError: null,
      assertionResults: assertions,
      loading: false,
      responseTab: "body",
    });

    // Apply variables written by scripts (pre / post) and database actions back to the current environment
    // (same semantics as pm.environment.set), so later requests can reference them via {{name}}.
    const persistedVars = {
      ...(proxy.varsSet ?? {}),
      ...(proxy.actionVars ?? {}),
    };
    if (Object.keys(persistedVars).length > 0) {
      const { activeEnvId, environments, globalVariables } = get();
      const activeEnv = environments.find((e) => e.id === activeEnvId);
      if (activeEnv) {
        get().updateEnvironment(activeEnv.id, {
          variables: { ...activeEnv.variables, ...persistedVars },
        });
      } else {
        // Fall back to global variables when no environment is active, so written variables are not lost
        set({ globalVariables: { ...globalVariables, ...persistedVars } });
      }
    }

    get().addHistoryEntry({
      requestId: req.id,
      name: req.name,
      method: req.method,
      url: effectiveSent.url,
      status: proxy.status,
      duration: proxy.duration,
      size: proxy.size,
      responseBody: proxy.body,
      timing: proxy.timing,
    });
  },
  setResponseTab: (t) => set({ responseTab: t }),
});
