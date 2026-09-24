// WASM plugin management (real backend).
import type { StateCreator } from "zustand";
import type {
  CodecCatalogEntry,
  PluginDescriptor,
  PluginLoadResult,
  PluginScanReport,
  ProtocolCatalogEntry,
} from "@/data/types";
import {
  pluginList as apiList,
  pluginScan as apiScan,
  pluginLoad as apiLoad,
  pluginNativeLoad as apiNativeLoad,
  pluginUnload as apiUnload,
  pluginInstallZip as apiInstallZip,
  pluginEnable as apiEnable,
  pluginDisable as apiDisable,
  pluginProtocols,
  pluginCodecs,
  pluginProtocolCatalog,
  pluginCodecCatalog,
} from "@/lib/bridge/plugin";
import type { AppState } from "../types";

export interface PluginSlice {
  plugins: PluginDescriptor[];
  /** Registered protocol ids (built-in + dynamic plugins), used by the protocol picker */
  protocolIds: string[];
  /** Registered codec names (built-in + dynamic plugins) */
  codecNames: string[];
  /** Protocol catalog (built-in + plugins, with dynamic form schemas) */
  protocolCatalog: ProtocolCatalogEntry[];
  /** Codec catalog (built-in + plugins) */
  codecCatalog: CodecCatalogEntry[];
  pluginsLoading: boolean;
  pluginsError: string | null;

  /** Fetch the plugin list, protocol/codec catalogs and the dynamic registry */
  loadPlugins: () => Promise<void>;
  /** Scan the plugin directory and load them (fail-isolated) */
  scanPlugins: (dir: string) => Promise<PluginScanReport>;
  /** Load a single wasm component */
  loadPlugin: (id: string, wasmBase64: string) => Promise<PluginLoadResult>;
  /** Load a native (dynamic library) protocol plugin */
  loadNativePlugin: (id: string, path: string) => Promise<PluginLoadResult>;
  /** Unload a plugin */
  unloadPlugin: (id: string) => Promise<void>;
  /** Install a zip plugin package */
  installZip: (id: string, zipBase64: string) => Promise<PluginLoadResult>;
  /** Enable a plugin */
  enablePlugin: (id: string) => Promise<void>;
  /** Disable a plugin */
  disablePlugin: (id: string) => Promise<void>;
}

export const createPluginSlice: StateCreator<AppState, [], [], PluginSlice> = (
  set,
  get,
) => ({
  plugins: [],
  protocolIds: [],
  codecNames: [],
  protocolCatalog: [],
  codecCatalog: [],
  pluginsLoading: false,
  pluginsError: null,

  loadPlugins: async () => {
    set({ pluginsLoading: true, pluginsError: null });
    try {
      const plugins = await apiList();
      const [protocolIds, codecNames, protocolCatalog, codecCatalog] =
        await Promise.all([
          pluginProtocols(),
          pluginCodecs(),
          pluginProtocolCatalog(),
          pluginCodecCatalog(),
        ]);
      set({
        plugins,
        protocolIds,
        codecNames,
        protocolCatalog,
        codecCatalog,
        pluginsLoading: false,
      });
    } catch (e) {
      set({ pluginsLoading: false, pluginsError: String(e) });
    }
  },

  scanPlugins: async (dir) => {
    const report = await apiScan(dir);
    await get().loadPlugins();
    return report;
  },

  loadPlugin: async (id, wasmBase64) => {
    const result = await apiLoad(id, wasmBase64);
    await get().loadPlugins();
    return result;
  },

  loadNativePlugin: async (id, path) => {
    const result = await apiNativeLoad(id, path);
    await get().loadPlugins();
    return result;
  },

  unloadPlugin: async (id) => {
    await apiUnload(id);
    await get().loadPlugins();
  },

  installZip: async (id, zipBase64) => {
    const result = await apiInstallZip(id, zipBase64);
    await get().loadPlugins();
    return result;
  },

  enablePlugin: async (id) => {
    await apiEnable(id);
    await get().loadPlugins();
  },

  disablePlugin: async (id) => {
    await apiDisable(id);
    await get().loadPlugins();
  },
});
