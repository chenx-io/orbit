//! Protocol menu injection verification: after a native (dynamic-library) protocol plugin loads, its protocol should appear in the protocol catalog
//! (`protocol_schemas`, i.e. the data source of `/api/protocols`) and the dynamic registry (`build_client_by_id`).
//!
//! Prerequisite: `cargo build --release` (under examples/plugins/pg-native); the artifact is pg_native.dll/.so.

use orbit_plugin::PluginManager;

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
fn native_plugin_protocol_appears_in_catalog() {
    let path = plugin_lib_path();
    if !path.exists() {
        eprintln!("SKIP: native plugin artifact {} not found", path.display());
        return;
    }

    let mut mgr = PluginManager::new().expect("plugin manager");
    mgr.load_native("pg-native", &path).expect("load_native");

    // 1) The descriptor kind should be native-protocol
    let desc = mgr.get("pg-native").expect("plugin should exist");
    assert_eq!(desc.kind, "native-protocol");
    assert!(desc.protocols.contains(&"pg".to_string()));

    // 2) protocol_schemas (the /api/protocols data source) should contain pg + schema
    let schemas = mgr.protocol_schemas();
    let pg = schemas.iter().find(|(pid, _, _)| pid == "pg");
    assert!(
        pg.is_some(),
        "protocol_schemas should contain the pg protocol"
    );
    let (_, conn_schema, req_schema) = pg.unwrap();
    assert!(conn_schema.is_object(), "should have a connection schema");
    assert!(req_schema.is_object(), "should have a message schema");

    // 3) The dynamic registry should build a client by pg (used by the engine/frontend protocol dropdown + single-shot/load tests)
    let client = orbit_protocol::registry::build_client_by_id("pg");
    assert!(
        client.is_some(),
        "build_client_by_id('pg') should be available"
    );
    assert_eq!(client.unwrap().name(), "pg");

    // 4) The protocol catalog id list (/api/plugins/protocols) should contain pg
    let ids = orbit_protocol::registry::list_protocol_ids();
    assert!(
        ids.contains(&"pg".to_string()),
        "protocol id list should contain pg"
    );
}
