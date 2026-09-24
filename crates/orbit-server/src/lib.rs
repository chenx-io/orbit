//! # orbit-server
//!
//! axum Web API service. Provides:
//! - POST /api/run — run a load test
//! - GET  /api/stream — SSE real-time metrics stream
//! - GET  /api/health — health check
//! - Mock service — built-in mock server

pub mod datasource_api;
pub mod distributed_api;
pub mod events;
pub mod http_api;
pub mod load;
pub mod mock;
pub mod reports;
pub mod session;

use crate::http_api::api_routes;
use crate::mock::{MockInterface, MockServer};
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub event_tx: broadcast::Sender<String>,
    pub mock_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Mock interfaces (API layer, including the expectation list)
    pub mock_rules: Arc<AsyncRwLock<Vec<MockInterface>>>,
    /// Interface Arc shared with the running mock server (editing in place takes effect immediately)
    pub mock_live: Arc<AsyncRwLock<Vec<MockInterface>>>,
    /// Current mock port (used to emit the event on stop)
    pub mock_port: Arc<Mutex<Option<u16>>>,
    /// Load-test cancel flag: set by POST /api/stop; the running run_handler exits as soon as possible
    pub load_abort: Arc<AtomicBool>,
    /// Distributed load-test controller (dual-mode agent access + task orchestration)
    pub distributed: orbit_distributed::Controller,
    /// Long-lived session manager (interactive debugging for non-HTTP protocols)
    pub sessions: session::SessionManager,
    /// WASM plugin manager (protocol/codec component loading + dynamic registration)
    pub plugins: Arc<tokio::sync::Mutex<orbit_plugin::PluginManager>>,
    /// Plugin install root (`~/.orbit/plugins`), target of zip install/uninstall
    pub plugins_root: std::path::PathBuf,
    /// Cookie Jar (session persistence): cookie store shared by single-send debugging; requests to the same domain automatically carry cookies accumulated from Set-Cookie
    pub cookie_jar: Arc<tokio::sync::Mutex<orbit_engine::cookie_jar::CookieJar>>,
    /// Data service: the Web channel uniformly goes through the Rust data layer (authoritative snapshot store, isomorphic for Tauri/Web)
    pub data: Arc<orbit_data::DataService<orbit_data::FileStorage>>,
    /// Structured event bus (audit / metering subscriptions: SSE + optional webhook)
    pub event_bus: broadcast::Sender<events::OrbitEvent>,
    /// Event webhook receiver endpoint (optional, from `ServerConfig.event_sink`)
    pub event_sink: Option<String>,
    /// Datasource registry (DB/Redis assertion queries; globally shared, lazy connect + idle reclamation)
    pub data_sources: Arc<orbit_datasource::DataSourceRegistry>,
}

/// Compute the plugin root directory: prefer `$ORBIT_PLUGIN_DIR`, otherwise `~/.orbit/plugins`.
pub fn plugins_root_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ORBIT_PLUGIN_DIR") {
        let p = std::path::PathBuf::from(d);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    if let Ok(d) = std::env::var("CHENX_PLUGIN_DIR") {
        let p = std::path::PathBuf::from(d);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    plugins_root_for_home(&home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
}

/// Compute the default plugin root directory from home (`<home>/.orbit/plugins`)
fn plugins_root_for_home(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".orbit").join("plugins")
}

/// Data root directory: `~/.orbit` (snapshot file `orbit_data.json`; conceptually the same as Tauri's app_data_dir).
pub fn data_root_dir() -> std::path::PathBuf {
    data_root_for_home(&home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
}

/// Compute the default data root directory from home (`<home>/.orbit`)
fn data_root_for_home(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".orbit")
}

/// Current user home (also supports Windows `USERPROFILE`)
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
}

pub fn migrate_legacy_home_dir() {
    if let Some(home) = home_dir() {
        migrate_legacy_home_dir_at(&home);
    }
}

/// Migration logic (home injected for testability): `<home>/.chenx` → `<home>/.orbit`, renaming the data file as well.
fn migrate_legacy_home_dir_at(home: &std::path::Path) {
    let old = home.join(".chenx");
    let new = home.join(".orbit");
    if !old.exists() || new.exists() {
        return;
    }
    if std::fs::rename(&old, &new).is_ok() {
        tracing::info!("[migrate] migrated {} → {}", old.display(), new.display());
    } else {
        match copy_dir_recursive(&old, &new) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&old);
                tracing::info!(
                    "[migrate] copied and cleaned up {} → {}",
                    old.display(),
                    new.display()
                );
            }
            Err(e) => tracing::warn!("[migrate] failed to migrate {}: {}", old.display(), e),
        }
    }
    // After the directory migration, rename the leftover legacy data file (chenx_data.json) to the new name
    let legacy_file = new.join("chenx_data.json");
    let cur_file = new.join("orbit_data.json");
    if legacy_file.exists() && !cur_file.exists() {
        let _ = std::fs::rename(&legacy_file, &cur_file);
    }
}

/// Recursively copy a directory (fallback used during migration)
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Seed the default mock interfaces consistent with the frontend
fn default_mock_rules() -> Vec<MockInterface> {
    vec![]
}

pub async fn run_server(
    port: u16,
    server_config: orbit_config::ServerConfig,
) -> anyhow::Result<()> {
    migrate_legacy_home_dir();

    let (event_tx, _) = broadcast::channel::<String>(64);
    let (event_bus, _) = broadcast::channel::<events::OrbitEvent>(1024);

    let seed = default_mock_rules();
    let mock_live: Vec<MockInterface> = seed.iter().filter(|r| r.enabled).cloned().collect();

    // Data service + datasource registry (synced from the snapshot on startup; afterwards incrementally synced by the data save/load endpoints)
    let data_service = Arc::new(
        orbit_data::DataService::open(
            orbit_data::FileStorage::new(data_root_dir().join("orbit_data.json")),
            orbit_data::SNAPSHOT_VERSION,
        )
        .await
        .map_err(|e| anyhow::anyhow!("failed to initialize data service: {}", e))?,
    );
    let data_sources = Arc::new(orbit_datasource::DataSourceRegistry::new());
    data_sources
        .register_all(&data_service.data_sources())
        .await;

    let state = AppState {
        event_tx,
        mock_task: Arc::new(Mutex::new(None)),
        mock_rules: Arc::new(AsyncRwLock::new(seed)),
        mock_live: Arc::new(AsyncRwLock::new(mock_live)),
        mock_port: Arc::new(Mutex::new(None)),
        load_abort: Arc::new(AtomicBool::new(false)),
        distributed: orbit_distributed::Controller::new(),
        sessions: session::SessionManager::new(),
        plugins: Arc::new(tokio::sync::Mutex::new(
            orbit_plugin::PluginManager::new()
                .map_err(|e| anyhow::anyhow!("plugin manager init: {}", e))?,
        )),
        plugins_root: {
            let p = plugins_root_dir();
            std::fs::create_dir_all(&p)?;
            p
        },
        cookie_jar: Arc::new(tokio::sync::Mutex::new(
            orbit_engine::cookie_jar::CookieJar::new(),
        )),
        // Data service: authoritative snapshot store for the Web channel (~/.orbit/orbit_data.json)
        data: data_service.clone(),
        data_sources,
        event_bus,
        event_sink: server_config.event_sink,
    };

    // Datasource idle reclamation (default period 60s): close connection pools unused for too long
    {
        let ds = state.data_sources.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                ds.sweep_idle().await;
            }
        });
    }

    // Prewarm the JS sandbox (process-wide shared): avoids initializing it on the first request with scripts (TTFB of several seconds)
    orbit_js::prewarm_sandbox();

    // Distributed: heartbeat timeout scan (offline detection)
    {
        let dist = state.distributed.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                dist.sweep_offline(10_000);
            }
        });
    }

    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/run", post(run_handler))
        .route("/api/stop", post(stop_handler))
        .route("/api/stream", get(stream_handler))
        .route("/api/validate", post(validate_handler))
        .route("/api/mock/start", post(start_mock_handler))
        .route("/api/mock/stop", post(stop_mock_handler))
        .merge(distributed_api::distributed_routes())
        .merge(api_routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("🚀 Orbit server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_handler() -> &'static str {
    r#"{"status":"ok","version":"0.1.0"}"#
}

#[derive(serde::Deserialize)]
struct RunRequest {
    yaml: String,
    #[allow(unused)]
    #[serde(default = "default_env")]
    env: String,
    #[serde(default = "default_vus")]
    vus: u32,
    #[serde(default = "default_duration")]
    duration: String,
}

fn default_env() -> String {
    String::new()
}
fn default_vus() -> u32 {
    1
}
fn default_duration() -> String {
    "10s".to_string()
}

async fn run_handler(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Json<serde_json::Value> {
    // Reset the cancel flag (a previous stop may have left it set) and clone one for this run
    state.load_abort.store(false, Ordering::Relaxed);
    let abort = state.load_abort.clone();

    // Each load test uses its own LoadRunner (separate Engine + metrics bus) to avoid metrics accumulating across runs
    let runner = load::LoadRunner::new();
    let bus = runner.bus.clone();
    let event_tx = state.event_tx.clone();

    // Notify the frontend: load test started
    let _ = event_tx.send("starting".into());

    // Real-time metrics polling: broadcast the current metrics snapshot to all SSE subscribers every 500ms while running
    let done = Arc::new(AtomicBool::new(false));
    let done_poller = done.clone();
    let event_tx_poller = event_tx.clone();
    let poller = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            // Take the snapshot and broadcast first, then check done — ensures the last 500ms of data is not lost
            let snap = load::live_frontend_summary(&bus);
            let _ = event_tx_poller.send(serde_json::to_string(&snap).unwrap_or_default());
            if done_poller.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    // Parse the YAML and run the load test (cancellable)
    let plan = match orbit_config::from_str(&req.yaml) {
        Ok(p) => p,
        Err(e) => {
            done.store(true, Ordering::Relaxed);
            poller.abort();
            let _ = event_tx.send(
                serde_json::to_string(&serde_json::json!({
                    "status": "error", "error": e.to_string()
                }))
                .unwrap_or_default(),
            );
            return Json(serde_json::json!({ "status": "error", "error": e.to_string() }));
        }
    };

    let run_id = uuid::Uuid::new_v4().simple().to_string();
    emit_event(
        &state,
        events::OrbitEvent::LoadRunStarted {
            run_id: run_id.clone(),
            vus: req.vus,
            user: None,
        },
    );

    let result = runner
        .run_plan_with_events(plan, req.vus, &req.duration, Some(abort), event_tx.clone())
        .await;

    // Stop the poller and broadcast the final summary
    done.store(true, Ordering::Relaxed);
    poller.abort();
    if let Ok(ref val) = result {
        let _ = event_tx.send(serde_json::to_string(val).unwrap_or_default());
    }
    emit_event(
        &state,
        events::OrbitEvent::LoadRunCompleted { run_id, user: None },
    );

    match result {
        Ok(v) => Json(v),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

async fn stream_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut rx = state.event_tx.subscribe();
    let stream = async_stream::stream! {
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
    };
    Sse::new(stream)
}

/// Structured event SSE (/api/events): audit / metering subscriptions.
async fn events_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut rx = state.event_bus.subscribe();
    let stream = async_stream::stream! {
        while let Ok(ev) = rx.recv().await {
            let data = serde_json::to_string(&ev).unwrap_or_default();
            yield Ok(Event::default().data(data));
        }
    };
    Sse::new(stream)
}

/// Broadcast a structured event (subscribers = /api/events SSE + the enhancement-package webhook).
pub fn emit_event(state: &AppState, event: events::OrbitEvent) {
    let _ = state.event_bus.send(event);
}

#[derive(serde::Deserialize)]
struct ValidateRequest {
    yaml: String,
}

async fn validate_handler(Json(req): Json<ValidateRequest>) -> Json<serde_json::Value> {
    match orbit_config::from_str(&req.yaml) {
        Ok(plan) => Json(serde_json::json!({
            "valid": true,
            "name": plan.name,
            "scenarios": plan.scenarios.len(),
        })),
        Err(e) => Json(serde_json::json!({"valid": false, "error": e.to_string()})),
    }
}

// ── Mock management ──

#[derive(serde::Deserialize)]
struct MockStartRequest {
    port: u16,
}

async fn start_mock_handler(
    State(state): State<AppState>,
    Json(req): Json<MockStartRequest>,
) -> Json<serde_json::Value> {
    // Stop any previously running mock instance.
    if let Some(h) = state.mock_task.lock().unwrap().take() {
        h.abort();
    }

    // Use the shared mock_live Arc: the running server reads the registered (and live-addable) interfaces directly
    let server = MockServer::with_rules(req.port, state.mock_live.clone());

    let handle = tokio::spawn(async move {
        let _ = server.start().await;
    });
    *state.mock_task.lock().unwrap() = Some(handle);
    *state.mock_port.lock().unwrap() = Some(req.port);
    emit_event(&state, events::OrbitEvent::MockStarted { port: req.port });

    Json(serde_json::json!({ "status": "started", "port": req.port }))
}

async fn stop_mock_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let port = state.mock_port.lock().unwrap().take();
    if let Some(h) = state.mock_task.lock().unwrap().take() {
        h.abort();
    }
    if let Some(port) = port {
        emit_event(&state, events::OrbitEvent::MockStopped { port });
    }
    Json(serde_json::json!({ "status": "stopped" }))
}

/// POST /api/stop — stop the load test currently in progress
async fn stop_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.load_abort.store(true, Ordering::Relaxed);
    eprintln!("[orbit-server] load_abort set — stress test will stop soon");
    Json(serde_json::json!({ "status": "stopping" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Create an isolated temporary home directory (auto-cleaned when the test ends)
    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit-server-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn migrate_renames_chenx_dir_and_data_file() {
        let home = temp_home("migrate");
        let old = home.join(".chenx");
        std::fs::create_dir_all(old.join("plugins")).unwrap();
        std::fs::write(old.join("chenx_data.json"), r#"{"k":1}"#).unwrap();
        std::fs::write(old.join("plugins/demo.wasm"), b"wasm").unwrap();

        migrate_legacy_home_dir_at(&home);

        let new = home.join(".orbit");
        assert!(new.exists(), ".orbit should have been created by migration");
        assert!(!old.exists(), ".chenx should have been migrated away");
        assert!(
            new.join("orbit_data.json").exists(),
            "chenx_data.json should be renamed to orbit_data.json"
        );
        assert!(!new.join("chenx_data.json").exists());
        assert_eq!(
            std::fs::read_to_string(new.join("orbit_data.json")).unwrap(),
            r#"{"k":1}"#
        );
        assert!(
            new.join("plugins/demo.wasm").exists(),
            "subdirectory contents should be migrated along with the directory"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn migrate_skips_when_new_exists() {
        let home = temp_home("skip");
        std::fs::create_dir_all(home.join(".chenx")).unwrap();
        std::fs::create_dir_all(home.join(".orbit")).unwrap();
        std::fs::write(home.join(".orbit/orbit_data.json"), r#"{"new":1}"#).unwrap();

        migrate_legacy_home_dir_at(&home);

        assert!(
            home.join(".chenx").exists(),
            "do not migrate the old directory when the new directory already exists"
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".orbit/orbit_data.json")).unwrap(),
            r#"{"new":1}"#
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn migrate_noop_when_legacy_absent() {
        let home = temp_home("noop");
        migrate_legacy_home_dir_at(&home);
        assert!(!home.join(".orbit").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn migrate_keeps_existing_orbit_data() {
        // When migrating the old directory, do not overwrite an existing file under the new name
        let home = temp_home("keep");
        std::fs::create_dir_all(home.join(".chenx")).unwrap();
        std::fs::write(home.join(".chenx/chenx_data.json"), r#"{"old":1}"#).unwrap();
        std::fs::create_dir_all(home.join(".orbit")).unwrap();
        std::fs::write(home.join(".orbit/orbit_data.json"), r#"{"new":1}"#).unwrap();

        migrate_legacy_home_dir_at(&home);

        assert_eq!(
            std::fs::read_to_string(home.join(".orbit/orbit_data.json")).unwrap(),
            r#"{"new":1}"#,
            "do not overwrite when a file under the new name already exists"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn plugins_and_data_root_for_home() {
        let home = PathBuf::from("/fake/home");
        assert_eq!(
            plugins_root_for_home(&home),
            home.join(".orbit").join("plugins")
        );
        assert_eq!(data_root_for_home(&home), home.join(".orbit"));
    }

    #[test]
    fn default_runtime_settings() {
        assert_eq!(default_env(), "");
        assert_eq!(default_vus(), 1);
        assert_eq!(default_duration(), "10s");
        assert!(default_mock_rules().is_empty());
    }
}
