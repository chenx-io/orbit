//! End-to-end verification: a native (dynamic library) protocol plugin runs a real PG query through the session execution chain.
//!
//! Covers the user-reported issue: create a collection (pg protocol + connection info) -> create a message -> send SQL ->
//! the session should return results through run_plugin -> NativeProtocolClient -> sqlx query.
//!
//! Prerequisites:
//!   1. `cargo build --release` (under examples/plugins/pg-native)
//!   2. Set `ORBIT_TEST_PG_URL` (e.g. postgresql://postgres:postgres@localhost:5432/mydb)
//!
//! Skipped if not satisfied.

use orbit_plugin::PluginManager;
use orbit_server::session::{OpenSessionRequest, SessionManager};

/// Split `postgresql://user:pass@host:port/db` into the collection connection form fields
fn parse_pg_url(url: &str) -> serde_json::Value {
    // postgresql://user:pass@host:port/db
    let rest = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))
        .unwrap_or(url);
    let (auth, hostpart) = match rest.find('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("", rest),
    };
    let (userpass, _) = match auth.find(':') {
        Some(i) => (&auth[..i], &auth[i + 1..]),
        None => (auth, ""),
    };
    let (hostport, db) = match hostpart.find('/') {
        Some(i) => (&hostpart[..i], &hostpart[i + 1..]),
        None => (hostpart, ""),
    };
    let (host, port) = match hostport.find(':') {
        Some(i) => (
            &hostport[..i],
            hostport[i + 1..].parse::<u64>().unwrap_or(5432),
        ),
        None => (hostport, 5432),
    };
    serde_json::json!({
        "host": host,
        "port": port,
        "username": userpass,
        "password": userpass,
        "database": if db.is_empty() { "postgres" } else { db },
    })
}

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

#[tokio::test]
async fn session_runs_native_pg_plugin() {
    let pg_url = std::env::var("ORBIT_TEST_PG_URL").ok();
    let (Some(pg_url), true) = (pg_url, plugin_lib_path().exists()) else {
        eprintln!("SKIP: requires ORBIT_TEST_PG_URL + pg-native.dll");
        return;
    };

    // 1) Load the native plugin -> globally register the pg protocol
    let mut pm = PluginManager::new().expect("plugin manager");
    pm.load_native("pg-native", &plugin_lib_path())
        .expect("load_native");

    // 2) Open a pg session (carrying connection config, simulating the collection connection: host/port/user/pass/db)
    let sm = SessionManager::new();
    // Extract host/port/user/pass/db from the URL (simulating the collection connection form fields)
    let conn_fields = parse_pg_url(&pg_url);
    let open_resp = sm
        .open(OpenSessionRequest {
            protocol: "pg".into(),
            url: "".into(),
            service: None,
            message_format: None,
            streaming: None,
            framing: None,
            message_type: None,
            close_after: None,
            payload: None,
            payload_type: None,
            pre_script: None,
            post_script: None,
            max_events: None,
            query: None,
            variables: None,
            operation_name: None,
            headers: None,
            env_vars: None,
            connection: Some(conn_fields),
        })
        .await
        .expect("open session");
    assert_eq!(open_resp.protocol, "pg");
    assert!(open_resp.can_send, "pg session should support sending");
    let sid = open_resp.session_id.clone();

    // 3) Send SQL (SELECT now()) -> run_plugin -> NativeProtocolClient -> sqlx
    //    verify the timestamp column maps correctly (no longer garbled)
    let _seq = sm
        .send(&sid, b"SELECT now() AS c".to_vec(), None)
        .await
        .expect("send SQL");

    // 4) Check the received response (there should be a recv message in the session log)
    let msgs = sm.messages(&sid);
    let recv = msgs.iter().find(|m| m.direction == "recv");
    assert!(
        recv.is_some(),
        "should receive a PG response, log: {:?}",
        msgs
    );
    let text = recv.unwrap().text.clone().unwrap_or_default();
    assert!(
        text.contains("rowCount"),
        "response should contain a JSON result, actual: {}",
        text
    );
    // timestamp should be formatted as a readable string (ISO style), not binary garbage
    assert!(
        text.contains("\"c\":\"20") || text.contains("\"c\":\"2"),
        "timestamp column should be formatted as readable time, actual: {}",
        text
    );

    let _ = sm.close(&sid).await;
}
