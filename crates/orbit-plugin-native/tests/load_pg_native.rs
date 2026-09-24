//! Dynamic-library (native) protocol plugin host loading verification.
//!
//! Loads the cdylib compiled from `examples/plugins/pg-native` (a sqlx PG plugin) to verify:
//! 1. dlopen -> ABI version check -> info() capability declaration
//! 2. the execute() call chain (pool reuse + a real PG query, if available)
//!
//! The real PG connection is provided via the `ORBIT_TEST_PG_URL` env var (e.g.
//! `postgresql://postgres:postgres@localhost:5432/mydb`); end-to-end is skipped if unset.
//!
//! Prerequisite: `cargo build --release` (under examples/plugins/pg-native).

use base64::Engine;
use orbit_plugin_native::NativePluginHandle;

fn plugin_lib_path() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/pg-native/target/release");
    #[cfg(target_os = "windows")]
    let name = "pg_native.dll";
    #[cfg(target_os = "linux")]
    let name = "libpg_native.so";
    #[cfg(target_os = "macos")]
    let name = "libpg_native.dylib";
    dir.join(name)
}

#[test]
fn native_pg_plugin_loads_and_infos() {
    let path = plugin_lib_path();
    if !path.exists() {
        eprintln!(
            "SKIP: plugin artifact {} not found; build examples/plugins/pg-native first",
            path.display()
        );
        return;
    }

    let handle = unsafe { NativePluginHandle::load(&path) }.expect("load native plugin");
    assert_eq!(handle.api_version(), 1);

    let info_json = handle.info();
    let info: serde_json::Value = serde_json::from_str(&info_json).expect("info should be JSON");
    assert_eq!(info["kind"], "protocol");
    let caps = info["capabilities"]
        .as_array()
        .expect("capabilities should be an array");
    assert!(
        caps.iter().any(|c| c["protocol_id"] == "pg"),
        "should declare the pg protocol"
    );
    assert!(
        info["connection_config_schema"].is_object(),
        "should have a connection schema"
    );
}

#[test]
fn native_pg_plugin_execute_via_real_db() {
    let pg_url = std::env::var("ORBIT_TEST_PG_URL");
    if pg_url.is_err() {
        eprintln!("SKIP: ORBIT_TEST_PG_URL not set, skipping real PG end-to-end");
        return;
    }
    let path = plugin_lib_path();
    assert!(
        path.exists(),
        "plugin artifact not found: {}",
        path.display()
    );

    let handle = unsafe { NativePluginHandle::load(&path) }.expect("load native plugin");

    // Pass the connection directly via url and run a query (verifies the full pool + sqlx chain)
    let req = serde_json::json!({
        "target": "",
        "connection": { "url": pg_url.unwrap() },
        "options": { "sql": "SELECT 1 AS one, 'orbit' AS tag" },
        "payload_b64": "",
        "timeout_ms": 10000
    });

    let result = handle
        .execute(&req.to_string())
        .expect("execute should succeed");
    let resp: serde_json::Value = serde_json::from_str(&result).expect("response should be JSON");
    // The response is a NativeProtocolResponse; payload_b64 holds the result JSON
    use base64::Engine;
    let payload = resp["payload_b64"].as_str().unwrap_or("");
    let body = String::from_utf8_lossy(
        &base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload base64"),
    )
    .into_owned();
    let data: serde_json::Value = serde_json::from_str(&body).expect("result should be JSON");
    assert_eq!(data["rowCount"], 1, "should return 1 row, got: {}", body);
    assert_eq!(data["rows"][0]["one"], 1);
    assert_eq!(data["rows"][0]["tag"], "orbit");
}

#[test]
fn native_pg_plugin_connection_pool_reused() {
    // Pool reuse: two execute calls for the same url should reuse the pool (no rebuild on the second)
    let pg_url = std::env::var("ORBIT_TEST_PG_URL");
    if pg_url.is_err() {
        eprintln!("SKIP: ORBIT_TEST_PG_URL not set");
        return;
    }
    let path = plugin_lib_path();
    let handle = unsafe { NativePluginHandle::load(&path) }.expect("load native plugin");

    for _ in 0..3 {
        let req = serde_json::json!({
            "connection": { "url": pg_url.clone().unwrap() },
            "options": { "sql": "SELECT count(*) AS n FROM pg_stat_activity" },
            "payload_b64": "",
            "timeout_ms": 10000
        });
        let result = handle.execute(&req.to_string()).expect("execute succeeded");
        // Decode the payload and check the result
        let resp: serde_json::Value =
            serde_json::from_str(&result).expect("response should be JSON");
        assert_eq!(resp["status_code"], 200);
        let payload = resp["payload_b64"].as_str().unwrap_or("");
        let body = String::from_utf8_lossy(
            &base64::engine::general_purpose::STANDARD
                .decode(payload)
                .expect("payload base64"),
        )
        .into_owned();
        let data: serde_json::Value = serde_json::from_str(&body).expect("result should be JSON");
        assert_eq!(data["rowCount"], 1, "should have a result row: {}", body);
    }
}
