//! Orbit dynamic-library (native) network protocol plugin host loader.
//!
//! Loads `cdylib` plugins via `libloading` and calls exported functions through the stable abi_stable ABI.
//! Plugins own their internal state (e.g. sqlx connection pools); the host does not touch networking/encryption/pooling.
//!
//! Loaded plugins must implement the exported functions defined by `orbit-plugin-api-native`:
//! `orbit_native_api_version` / `orbit_native_info` / `orbit_native_execute` / `orbit_native_shutdown`。

use abi_stable::std_types::RString;
use std::path::Path;
use std::sync::Arc;

/// Dynamic-library plugin handle: holds the loaded library + function pointers
pub struct NativePluginHandle {
    /// Keeps the library from being unloaded (function-pointer lifetimes depend on it; the loader does not unload either)
    _lib: Arc<libloading::Library>,
    api_version: u32,
    info: extern "C" fn() -> RString,
    execute: extern "C" fn(RString) -> RString,
}

// libloading::Library itself is !Send+!Sync, but a dlopen handle is safe to call across threads as long as the library is not unloaded.
// The plugin handle must be shared across VU threads in concurrent load tests (the factory requires Send+Sync), so it is marked explicitly.
// Safety: the library is not unloaded during the process lifetime (an Arc keeps a reference), and function pointers only call the plugin's stateless/thread-safe execute.
unsafe impl Send for NativePluginHandle {}
unsafe impl Sync for NativePluginHandle {}

impl NativePluginHandle {
    /// Load a dynamic-library plugin
    ///
    /// # Safety
    /// `dlopen` and function pointer calls require unsafe; assumes the plugin implements the agreed ABI.
    pub unsafe fn load(path: &Path) -> Result<Self, String> {
        let lib =
            Arc::new(libloading::Library::new(path).map_err(|e| {
                format!("failed to load dynamic library {}: {}", path.display(), e)
            })?);

        unsafe {
            let api_version: libloading::Symbol<extern "C" fn() -> u32> = lib
                .get(b"orbit_native_api_version")
                .map_err(|e| format!("missing orbit_native_api_version: {}", e))?;
            let version = api_version();
            if version != orbit_plugin_api_native::API_VERSION {
                return Err(format!(
                    "plugin ABI version mismatch: expected {}, got {}",
                    orbit_plugin_api_native::API_VERSION,
                    version
                ));
            }

            let info: libloading::Symbol<extern "C" fn() -> RString> = lib
                .get(b"orbit_native_info")
                .map_err(|e| format!("missing orbit_native_info: {}", e))?;
            let execute: libloading::Symbol<extern "C" fn(RString) -> RString> = lib
                .get(b"orbit_native_execute")
                .map_err(|e| format!("missing orbit_native_execute: {}", e))?;

            let info_ptr = *info;
            let execute_ptr = *execute;

            Ok(Self {
                _lib: lib,
                api_version: version,
                info: info_ptr,
                execute: execute_ptr,
            })
        }
    }

    /// Plugin metadata JSON
    pub fn info(&self) -> String {
        let s = (self.info)();
        (*s).to_string()
    }

    /// Execute one request (input JSON, output JSON; errors are in the JSON `error` field)
    pub fn execute(&self, req_json: &str) -> Result<String, String> {
        let resp = (self.execute)(RString::from(req_json.to_string()));
        let text = (*resp).to_string();
        // If the returned JSON is an error (contains an error field), parse it as Err
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
                return Err(e.to_string());
            }
        }
        Ok(text)
    }

    pub fn api_version(&self) -> u32 {
        self.api_version
    }
}
