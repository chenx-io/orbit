mod commands;
mod db;
mod state;

use db::Database;
use state::AppState;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Create the system tray: icon + context menu (show main window / quit); left-click shows the window.
/// The tray icon reuses the app's default icon (the new logo embedded into the exe at build time).
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "Show Main Window", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id("orbit-tray")
        .icon(icon)
        .tooltip("Orbit — All-in-one API testing toolkit")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Show and focus the main window (tray "Show Main Window" / left-click)
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// One-time migration of the data files under the legacy identifier directory (`io.chenx-io.chenx`) into the current `app_data_dir`,
/// and renames the old file names `chenx_data.json/.db` to `orbit_data.json/.db`.
fn migrate_legacy_app_data(app: &tauri::App) {
    let new_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    let old_dir = new_dir
        .parent()
        .map(|p| p.join("io.chenx-io.chenx"))
        .unwrap_or_default();
    if old_dir == new_dir {
        return;
    }
    let _ = std::fs::create_dir_all(&new_dir);
    let mut migrated = 0;
    for (legacy, cur) in [
        ("chenx_data.json", "orbit_data.json"),
        ("chenx_data.db", "orbit_data.db"),
    ] {
        let dst = new_dir.join(cur);
        if dst.exists() {
            continue;
        }
        // Current dir already has the legacy name (from a previous identifier migration) -> rename
        let local_legacy = new_dir.join(legacy);
        if local_legacy.exists() {
            if std::fs::rename(&local_legacy, &dst).is_ok() {
                migrated += 1;
            }
            continue;
        }
        // Legacy identifier directory -> copy to the new name
        let src = old_dir.join(legacy);
        if src.exists() && std::fs::copy(&src, &dst).is_ok() {
            migrated += 1;
        }
    }
    if migrated > 0 {
        tracing::info!(
            "[migrate] data files migrated ({} -> {})",
            old_dir.display(),
            new_dir.display()
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Migrate old app_data_dir (io.chenx-io.chenx) and ~/.chenx data to the new location
            migrate_legacy_app_data(app);
            orbit_server::migrate_legacy_home_dir();

            // if cfg!(debug_assertions) {
            //     app.handle().plugin(
            //         tauri_plugin_log::Builder::default()
            //             .level(log::LevelFilter::Info)
            //             .build(),
            //     )?;
            // }

            // Initialize database (registered separately: used by store.rs db_* commands via State<'_, Database>)
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            let db_path = app_dir.join("orbit_data.db");
            let database = Database::new(&db_path).expect("failed to initialize database");
            app.manage(database);

            // Data service: authoritative snapshot storage (same file as the old save_snapshot; seamless data migration)
            let data_service = tauri::async_runtime::block_on(orbit_data::DataService::open(
                orbit_data::FileStorage::new(app_dir.join("orbit_data.json")),
                orbit_data::SNAPSHOT_VERSION,
            ))
            .expect("failed to initialize data service");
            // Data source registry: sync data sources from snapshot at startup (for DB/Redis assertion queries)
            let data_sources = std::sync::Arc::new(orbit_datasource::DataSourceRegistry::new());
            tauri::async_runtime::block_on(data_sources.register_all(&data_service.data_sources()));

            // Cookie Jar (session persistence): shared instance for one-off debug / AI runs
            let cookie_jar = Arc::new(Mutex::new(orbit_engine::cookie_jar::CookieJar::new()));

            // Build app state
            let app_state = AppState {
                mock_server: Arc::new(RwLock::new(state::MockServerHandle::new())),
                load_abort: Arc::new(AtomicBool::new(false)),
                load_epoch: Arc::new(AtomicU64::new(0)),
                load_progress: Arc::new(RwLock::new(None)),
                load_done: Arc::new(RwLock::new(None)),
                scenario_progress: Arc::new(Mutex::new(VecDeque::new())),
                last_load_bus: Arc::new(Mutex::new(None)),
                plugins: Arc::new(Mutex::new(
                    orbit_plugin::PluginManager::new().expect("plugin manager init"),
                )),
                plugins_root: {
                    let home = std::env::var_os("HOME")
                        .map(std::path::PathBuf::from)
                        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let p = home.join(".orbit").join("plugins");
                    std::fs::create_dir_all(&p).ok();
                    p
                },
                // Data service: authoritative snapshot storage (same file as the old save_snapshot; seamless data migration)
                data: Arc::new(data_service),
                // Data source registry: sync data source configs from the snapshot at startup
                data_sources: data_sources.clone(),
                cookie_jar: cookie_jar.clone(),
                // AI assistant: credentials and sessions live in <app_data_dir>/ai (not in the snapshot)
                ai: commands::ai::AiState::new(&app_dir),
            };

            app.manage(app_state);
            // Registry managed separately (for proxy request DB/Redis assertion injection)
            app.manage::<std::sync::Arc<orbit_datasource::DataSourceRegistry>>(
                data_sources.clone(),
            );
            // Idle reclamation background task (default 60s interval)
            {
                let ds = data_sources;
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        ds.sweep_idle().await;
                    }
                });
            }

            // Prewarm the JS sandbox (process-wide shared): QuickJS + bootstrap init takes hundreds of ms to seconds,
            // done ahead of time on a background thread to avoid a TTFB spike on the first scripted request.
            orbit_js::prewarm_sandbox();

            // Distributed Controller: heartbeat offline sweep.
            // Note: we no longer push `distributed-event` - a Tauri background thread emit would
            // contend for the same webview lock as the window message loop, freezing the app on drag/resize.
            // The frontend now polls `distributed_agents` / `distributed_task_progress` instead.
            let dist = orbit_distributed::Controller::new();
            {
                let dist_sweeper = dist.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        dist_sweeper.sweep_offline(10_000);
                    }
                });
            }
            app.manage(dist);

            // Long-connection session manager: events pushed via session-event (interactive debugging for non-HTTP protocols)
            let mut session_mgr = orbit_server::session::SessionManager::new();
            let app_handle = app.handle().clone();
            session_mgr.set_global_sink(move |ev| {
                let _ = app_handle.emit("session-event", ev);
            });
            app.manage(session_mgr);

            // Cookie Jar (session persistence): shared cookie store for one-off debug; same-domain requests attach them automatically
            // (same Arc as AppState.cookie_jar; sessions are shared when AI runs requests)
            app.manage::<Arc<tokio::sync::Mutex<orbit_engine::cookie_jar::CookieJar>>>(cookie_jar);

            // System tray (icon + menu; closing the window keeps the default behavior - quit the app)
            setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Dynamic values
            commands::dynamic::generate_dynamic_value,
            commands::dynamic::resolve_dynamic_values,
            // HTTP Proxy
            commands::proxy::execute_request,
            // Mock server
            commands::mock::start_mock_server,
            commands::mock::stop_mock_server,
            commands::mock::get_mock_rules,
            commands::mock::save_mock_interface,
            commands::mock::delete_mock_interface,
            // Import
            commands::import::parse_import,
            commands::import::read_text_file,
            // gRPC set (proto import / reflection import / message template / message schema)
            commands::grpc::grpc_import_proto,
            commands::grpc::grpc_reflection,
            commands::grpc::grpc_message_template,
            commands::grpc::grpc_message_schema,
            // Export
            commands::export::export_request,
            commands::export::export_collection,
            // Data (authoritative store, snapshot sync + direct export query; module commands + optimistic lock)
            commands::data::data_load_snapshot,
            commands::data::data_save_snapshot,
            commands::data::data_clear,
            commands::data::data_list_history,
            commands::data::data_add_history,
            commands::data::data_clear_history,
            commands::data::data_list_collections,
            commands::data::data_list_requests,
            commands::data::data_list_models,
            commands::data::data_list_environments,
            commands::data::data_list_scenarios,
            commands::data::data_upsert_collection,
            commands::data::data_remove_collection,
            commands::data::data_upsert_request,
            commands::data::data_remove_request,
            commands::data::data_upsert_model,
            commands::data::data_remove_model,
            commands::data::data_upsert_environment,
            commands::data::data_remove_environment,
            commands::data::data_upsert_scenario,
            commands::data::data_remove_scenario,
            // Workspace (project boundary CRUD / stats / activation)
            commands::data::data_list_workspaces,
            commands::data::data_active_workspace_id,
            commands::data::data_add_workspace,
            commands::data::data_rename_workspace,
            commands::data::data_remove_workspace,
            commands::data::data_set_active_workspace,
            commands::data::data_workspace_stats,
            commands::data::data_set_active_env,
            commands::data::data_set_global_variables,
            commands::data::data_set_global_secrets,
            // Assertion
            commands::assertion::validate_response_against_model,
            // Data sources (for DB/Redis assertions)
            commands::datasource::ds_list,
            commands::datasource::ds_upsert,
            commands::datasource::ds_remove,
            commands::datasource::ds_test,
            commands::datasource::ds_preview_query,
            // Load test
            commands::load::run_load_test,
            commands::load::stop_load_test,
            commands::load::run_scenario,
            commands::load::load_progress,
            commands::load::scenario_progress,
            commands::load::export_load_report,
            // Distributed
            commands::distributed::distributed_agents,
            commands::distributed::distributed_add_agent,
            commands::distributed::distributed_agent_action,
            commands::distributed::distributed_run,
            commands::distributed::distributed_stop,
            commands::distributed::distributed_result,
            commands::distributed::distributed_execute,
            commands::distributed::distributed_controller_status,
            commands::distributed::distributed_controller_start,
            commands::distributed::distributed_controller_stop,
            commands::distributed::distributed_task_progress,
            // Long-connection sessions
            commands::session::session_open,
            commands::session::session_send,
            commands::session::session_close,
            commands::session::session_messages,
            commands::session::grpc_reflect,
            // Reports & Baselines
            commands::reports::save_report,
            commands::reports::list_reports,
            commands::reports::load_report,
            commands::reports::delete_report,
            commands::reports::export_saved_report,
            commands::reports::set_baseline,
            commands::reports::unset_baseline,
            commands::reports::list_baselines,
            // Scenario run reports
            commands::scenario_reports::save_scenario_report,
            commands::scenario_reports::list_scenario_reports,
            commands::scenario_reports::load_scenario_report,
            commands::scenario_reports::delete_scenario_report,
            commands::scenario_reports::clear_scenario_reports,
            // Store - Collections
            commands::store::db_get_collections,
            commands::store::db_create_collection,
            commands::store::db_update_collection,
            commands::store::db_delete_collection,
            // Store - Requests
            commands::store::db_get_requests,
            commands::store::db_create_request,
            commands::store::db_update_request,
            commands::store::db_delete_request,
            // Store - Environments
            commands::store::db_get_environments,
            commands::store::db_create_environment,
            commands::store::db_update_environment,
            commands::store::db_delete_environment,
            // Store - History
            commands::store::db_get_history,
            commands::store::db_add_history_entry,
            commands::store::db_clear_history,
            commands::store::db_delete_history_entry,
            // Store - Models
            commands::store::db_get_models,
            commands::store::db_create_model,
            commands::store::db_update_model,
            commands::store::db_delete_model,
            // Persistence - local snapshot (full-database JSON)
            commands::persistence::save_snapshot,
            commands::persistence::load_snapshot,
            commands::persistence::clear_snapshot,
            commands::persistence::export_snapshot,
            commands::persistence::restore_mock_rules,
            // WASM Plugin management
            commands::plugin::plugin_list,
            commands::plugin::plugin_scan,
            commands::plugin::plugin_load,
            commands::plugin::plugin_native_load,
            commands::plugin::plugin_unload,
            commands::plugin::plugin_protocols,
            commands::plugin::plugin_codecs,
            commands::plugin::plugin_protocol_catalog,
            commands::plugin::plugin_codec_catalog,
            commands::plugin::plugin_install,
            commands::plugin::plugin_enable,
            commands::plugin::plugin_disable,
            // AI assistant (BYOK): preferences / credentials / sessions / turns / approval / event polling
            commands::ai::ai_config_get,
            commands::ai::ai_config_save,
            commands::ai::ai_credential_list,
            commands::ai::ai_credential_save,
            commands::ai::ai_credential_remove,
            commands::ai::ai_list_models,
            commands::ai::ai_test_connection,
            commands::ai::ai_session_list,
            commands::ai::ai_session_load,
            commands::ai::ai_session_save,
            commands::ai::ai_session_delete,
            commands::ai::ai_read_definition_file,
            commands::ai::ai_start_turn,
            commands::ai::ai_abort_turn,
            commands::ai::ai_approve_tool,
            commands::ai::ai_drain_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
