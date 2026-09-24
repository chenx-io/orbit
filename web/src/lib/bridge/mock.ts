// Mock service: CRUD for interfaces + expectation models (real backend).
import type { MockInterface } from "@/data/types";
import { apiGet, apiPost, isTauri, tauriInvoke } from "./client";

/* eslint-disable @typescript-eslint/no-explicit-any */

// Backend snake_case payload → frontend camelCase MockInterface
function normalizeMockInterface(r: any): MockInterface {
  return {
    requestId: r.request_id ?? "",
    workspaceId: r.workspace_id ?? undefined,
    method: r.method,
    path: r.path,
    enabled: r.enabled ?? true,
    expectations: (r.expectations ?? []).map((e: any) => ({
      id: e.id ?? "",
      name: e.name ?? "",
      enabled: e.enabled ?? true,
      conditions: (e.conditions ?? []).map((c: any) => ({
        location: (c.location ??
          "query") as MockInterface["expectations"][number]["conditions"][number]["location"],
        name: c.name ?? "",
        op: (c.op ??
          "equals") as MockInterface["expectations"][number]["conditions"][number]["op"],
        value: c.value ?? "",
      })),
      ipCondition: {
        enabled: e.ip_condition?.enabled ?? false,
        ip: e.ip_condition?.ip ?? "",
      },
      status: e.status ?? 200,
      headers: e.headers ?? {},
      body: e.body ?? "",
      delayMs: e.delay_ms ?? 0,
    })),
  };
}

// Frontend camelCase MockInterface → backend snake_case payload
function toApiInterface(r: MockInterface): any {
  return {
    request_id: r.requestId || null,
    workspace_id: r.workspaceId || null,
    method: r.method,
    path: r.path,
    enabled: r.enabled,
    expectations: r.expectations.map((e) => ({
      id: e.id,
      name: e.name,
      enabled: e.enabled,
      conditions: e.conditions.map((c) => ({
        location: c.location,
        name: c.name,
        op: c.op,
        value: c.value,
      })),
      ip_condition: { enabled: e.ipCondition.enabled, ip: e.ipCondition.ip },
      status: e.status,
      headers: e.headers,
      body: e.body,
      delay_ms: e.delayMs,
    })),
  };
}

export async function startMockServer(
  port: number,
  workspaceId?: string,
): Promise<boolean> {
  if (isTauri()) {
    return tauriInvoke<boolean>("start_mock_server", {
      port,
      workspaceId: workspaceId ?? null,
    });
  }
  await apiPost<unknown>("/api/mock/start", { port, rules: [] });
  return true;
}

export async function stopMockServer(): Promise<boolean> {
  if (isTauri()) {
    return tauriInvoke<boolean>("stop_mock_server");
  }
  await apiPost<unknown>("/api/mock/stop", {});
  return true;
}

export async function getMockRules(
  workspaceId?: string,
): Promise<MockInterface[]> {
  if (isTauri()) {
    const rules = await tauriInvoke<any[]>("get_mock_rules", {
      workspaceId: workspaceId ?? null,
    });
    return rules.map(normalizeMockInterface);
  }
  // No arguments = full set (snapshot persistence); with arguments = filtered by workspace (UI)
  const url = workspaceId
    ? `/api/mock/rules?workspaceId=${encodeURIComponent(workspaceId)}`
    : "/api/mock/rules";
  const rules = await apiGet<any[]>(url);
  return rules.map(normalizeMockInterface);
}

/** Save (insert or update) an interface and its expectation list */
export async function saveMockInterface(
  iface: MockInterface,
): Promise<MockInterface> {
  if (isTauri()) {
    const saved = await tauriInvoke<any>("save_mock_interface", {
      interface: toApiInterface(iface),
    });
    return normalizeMockInterface(saved);
  }
  const saved = await apiPost<any>(
    "/api/mock/interface",
    toApiInterface(iface),
  );
  return normalizeMockInterface(saved);
}

/** Delete an interface (by method + path + workspace) */
export async function deleteMockInterface(
  method: string,
  path: string,
  workspaceId?: string,
): Promise<boolean> {
  if (isTauri()) {
    return tauriInvoke<boolean>("delete_mock_interface", {
      method,
      path,
      workspaceId: workspaceId ?? null,
    });
  }
  return apiPost<boolean>("/api/mock/interface/delete", {
    method,
    path,
    workspace_id: workspaceId ?? "ws-default",
  });
}

/** Restore mock rules (Tauri writes into MockServerHandle; the browser bulk-restores into orbit-server, skipping individual failures) */
export async function restoreMockRules(rules: MockInterface[]): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("restore_mock_rules", {
      rules: rules.map(toApiInterface),
    });
    return;
  }
  for (const r of rules) {
    try {
      await saveMockInterface(r);
    } catch (e) {
      console.warn("[persist] failed to restore a mock rule (skipping it)", e);
    }
  }
}
