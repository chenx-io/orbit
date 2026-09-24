// WASM plugin management bridge.
// - Tauri：plugin_list / plugin_scan / plugin_load / plugin_unload / plugin_protocols / plugin_codecs；
// - Browser: /api/plugins/* REST.
// Behaviorally consistent with the orbit-server HTTP API and the Tauri commands.

import { apiDelete, apiGet, apiPost, isTauri, tauriInvoke } from "./client";
import type {
  CodecCatalogEntry,
  PluginDescriptor,
  PluginLoadResult,
  PluginScanReport,
  ProtocolCatalogEntry,
} from "@/data/types";

/* eslint-disable @typescript-eslint/no-explicit-any */

export type {
  CodecCatalogEntry,
  PluginDescriptor,
  PluginLoadResult,
  PluginScanReport,
  ProtocolCatalogEntry,
} from "@/data/types";

export async function pluginList(): Promise<PluginDescriptor[]> {
  if (isTauri()) {
    return tauriInvoke<PluginDescriptor[]>("plugin_list");
  }
  return apiGet<PluginDescriptor[]>("/api/plugins");
}

export async function pluginScan(dir: string): Promise<PluginScanReport> {
  if (isTauri()) {
    return tauriInvoke<PluginScanReport>("plugin_scan", { dir });
  }
  return apiPost<PluginScanReport>("/api/plugins/scan", { dir });
}

export async function pluginLoad(
  id: string,
  wasmBase64: string,
): Promise<PluginLoadResult> {
  if (isTauri()) {
    return tauriInvoke<PluginLoadResult>("plugin_load", { id, wasmBase64 });
  }
  return apiPost<PluginLoadResult>("/api/plugins/load", {
    id,
    wasm_base64: wasmBase64,
  });
}

/** Load a native (dynamic library) protocol plugin */
export async function pluginNativeLoad(
  id: string,
  path: string,
): Promise<PluginLoadResult & { protocols?: string[] }> {
  if (isTauri()) {
    return tauriInvoke<PluginLoadResult & { protocols?: string[] }>(
      "plugin_native_load",
      {
        id,
        path,
      },
    );
  }
  return apiPost<PluginLoadResult & { protocols?: string[] }>(
    "/api/plugins/native-load",
    {
      id,
      path,
    },
  );
}

export async function pluginUnload(id: string): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("plugin_unload", { id });
    return;
  }
  await apiDelete<void>(`/api/plugins/${encodeURIComponent(id)}`);
}

/** Install a zip plugin package */
export async function pluginInstallZip(
  id: string,
  zipBase64: string,
): Promise<PluginLoadResult & { status?: string }> {
  if (isTauri()) {
    return tauriInvoke<PluginLoadResult & { status?: string }>(
      "plugin_install",
      {
        id,
        zipBase64,
      },
    );
  }
  return apiPost<PluginLoadResult & { status?: string }>(
    "/api/plugins/install",
    {
      id,
      zip_base64: zipBase64,
    },
  );
}

/** Enable a plugin (reload from the install dir and register) */
export async function pluginEnable(id: string): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("plugin_enable", { id });
    return;
  }
  await apiPost<void>(`/api/plugins/${encodeURIComponent(id)}/enable`, {});
}

/** Disable a plugin (unregister + destroy; the directory is kept) */
export async function pluginDisable(id: string): Promise<void> {
  if (isTauri()) {
    await tauriInvoke<void>("plugin_disable", { id });
    return;
  }
  await apiPost<void>(`/api/plugins/${encodeURIComponent(id)}/disable`, {});
}

/** Registered protocol ids (built-in + dynamic plugins) */
export async function pluginProtocols(): Promise<string[]> {
  if (isTauri()) {
    return tauriInvoke<string[]>("plugin_protocols");
  }
  return apiGet<string[]>("/api/plugins/protocols");
}

/** Registered codec names (built-in + dynamic plugins) */
export async function pluginCodecs(): Promise<string[]> {
  if (isTauri()) {
    return tauriInvoke<string[]>("plugin_codecs");
  }
  return apiGet<string[]>("/api/plugins/codecs");
}

/** Protocol catalog (built-in + plugin protocols, with dynamic form schemas) */
export async function pluginProtocolCatalog(): Promise<ProtocolCatalogEntry[]> {
  if (isTauri()) {
    return tauriInvoke<ProtocolCatalogEntry[]>("plugin_protocol_catalog");
  }
  return apiGet<ProtocolCatalogEntry[]>("/api/protocols");
}

/** Codec catalog (built-in + plugin codecs) */
export async function pluginCodecCatalog(): Promise<CodecCatalogEntry[]> {
  if (isTauri()) {
    return tauriInvoke<CodecCatalogEntry[]>("plugin_codec_catalog");
  }
  return apiGet<CodecCatalogEntry[]>("/api/codecs");
}
