use orbit_metrics::LocalMetricsBus;
use orbit_server::mock::MockInterface;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub struct AppState {
    pub mock_server: Arc<RwLock<MockServerHandle>>,
    /// Load-test cancel flag: set when "Stop" is clicked; a running engine exits as soon as possible.
    pub load_abort: Arc<AtomicBool>,
    /// Load-test run epoch: incremented on each `run_load_test`. Background tasks of an old epoch
    /// validate the epoch before writing shared state, so an old task can't clobber the new state after a reset.
    pub load_epoch: Arc<AtomicU64>,
    /// Load-test live metrics snapshot (JSON string). Written by a background polling task, read by the frontend `load_progress`
    /// command polling, replacing the `load-progress` event push (avoids a Tauri emit contending with the window
    /// message loop over the lock, which would freeze the window while dragging).
    pub load_progress: Arc<RwLock<Option<String>>>,
    /// Load-test final result (JSON string). Written after the engine finishes; the frontend polls and wraps up.
    pub load_done: Arc<RwLock<Option<String>>>,
    /// Automation-scenario step progress buffer. Written by a background task during a run; the frontend `scenario_progress`
    /// command polls and drains it (replacing the `load-progress` event push).
    pub scenario_progress: Arc<Mutex<VecDeque<String>>>,
    /// Metrics bus of the most recent load test (keeps summary and raw samples for the page's "Export Report" to generate files in each format).
    pub last_load_bus: Arc<Mutex<Option<Arc<LocalMetricsBus>>>>,
    /// WASM plugin manager (protocol/codec component loading + dynamic registration)
    pub plugins: Arc<Mutex<orbit_plugin::PluginManager>>,
    /// Plugin install root (`~/.orbit/plugins`), target of zip install/uninstall
    pub plugins_root: std::path::PathBuf,
    /// Data service: authoritative store, read/written in frontend snapshot-sync mode, queried directly on export.
    ///
    /// Wrapped in `Arc` so it can be cloned into background tasks (the AI tool host needs a 'static handle);
    /// all existing `state.data.xxx()` calls are unaffected thanks to auto-deref.
    pub data: std::sync::Arc<orbit_data::DataService<orbit_data::FileStorage>>,
    /// Data source registry (DB/Redis assertion queries; synced from snapshot at startup, incrementally maintained by commands at runtime)
    pub data_sources: std::sync::Arc<orbit_datasource::DataSourceRegistry>,
    /// Shared Cookie Jar (one-off debug / AI request runs share the same session persistence)
    pub cookie_jar: std::sync::Arc<Mutex<orbit_engine::cookie_jar::CookieJar>>,
    /// AI assistant runtime (credentials / sessions / event buffer / pending-approval channel)
    pub ai: crate::commands::ai::AiState,
}

pub struct MockServerHandle {
    pub running: bool,
    pub port: u16,
    /// Running Mock service task handle (used to stop it)
    pub abort_handle: Option<tokio::task::JoinHandle<()>>,
    /// Registered Mock interfaces (including expectations list)
    pub rules: Arc<RwLock<Vec<MockInterface>>>,
    /// Interface Arc shared by the running Mock service (in-place edits take effect immediately)
    pub live: Arc<RwLock<Vec<MockInterface>>>,
}

impl MockServerHandle {
    pub fn new() -> Self {
        // No built-in rules by default; the user adds them in the UI
        MockServerHandle {
            running: false,
            port: 0,
            abort_handle: None,
            rules: Arc::new(RwLock::new(Vec::new())),
            live: Arc::new(RwLock::new(Vec::new())),
        }
    }
}
