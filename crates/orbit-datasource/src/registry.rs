//! Global data source registry: lazy connections + connection-pool caching + health probes + idle reclamation.
//!
//! The registry is shared across requests/scenarios (Tauri state / HTTP service state / engine handle),
//! and assertions reference data sources by id. Connection failures return clear errors and never silently cache a bad pool.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use orbit_assertion::QueryResult;
use orbit_config::{DataSourceConfig, DataSourceKind};
use redis::aio::ConnectionManager;
use sqlx::any::AnyPoolOptions;
use sqlx::AnyPool;
use tokio::sync::RwLock;

use crate::error::DsError;
use crate::redis as redis_exec;
use crate::sql as sql_exec;

/// Millisecond timestamp (used for pool access / idle reclamation).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct SqlEntry {
    pool: AnyPool,
    last_used_ms: AtomicU64,
}

struct RedisEntry {
    manager: ConnectionManager,
    last_used_ms: AtomicU64,
}

#[derive(Default)]
struct RegistryState {
    configs: HashMap<String, DataSourceConfig>,
    sqls: HashMap<String, SqlEntry>,
    redises: HashMap<String, RedisEntry>,
}

/// Data source registry (`Clone` shares the same internal state).
pub struct DataSourceRegistry {
    state: Arc<RwLock<RegistryState>>,
}

/// Test connection report (JSON fields camelCase, aligned with the frontend/HTTP API).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DsTestReport {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub ok: bool,
    pub latency_ms: u64,
    /// Server info on success (version, etc.)
    pub detail: Option<String>,
    /// Failure reason
    pub error: Option<String>,
}

impl Default for DataSourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DataSourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataSourceRegistry").finish_non_exhaustive()
    }
}

impl DataSourceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RegistryState::default())),
        }
    }

    /// Batch register (overwrites same-named ids).
    pub async fn register_all(&self, configs: &[DataSourceConfig]) {
        for c in configs {
            self.register(c.clone()).await;
        }
    }

    /// Register/update a data source: remove the old pool (on type or config change), then write the new config.
    ///
    /// Connections are lazy (established on first access); registering itself triggers no network IO.
    pub async fn register(&self, cfg: DataSourceConfig) {
        let cfg = resolve_env_config(cfg);
        let mut st = self.state.write().await;
        if let Some(old) = st.configs.get(&cfg.id) {
            if old.kind != cfg.kind {
                st.sqls.remove(&cfg.id);
                st.redises.remove(&cfg.id);
            }
        }
        st.configs.insert(cfg.id.clone(), cfg);
    }

    /// Remove a data source and release its connection.
    pub async fn remove(&self, id: &str) {
        let mut st = self.state.write().await;
        st.configs.remove(id);
        st.sqls.remove(id);
        st.redises.remove(id);
    }

    /// Read a single config.
    pub async fn config(&self, id: &str) -> Option<DataSourceConfig> {
        self.state.read().await.configs.get(id).cloned()
    }

    /// List all configs.
    pub async fn list(&self) -> Vec<DataSourceConfig> {
        let st = self.state.read().await;
        let mut out: Vec<_> = st.configs.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Read and validate config (exists + enabled), returning an error-friendly clone. Type matching is decided by the caller per operation.
    async fn ensure_cfg(&self, id: &str) -> Result<DataSourceConfig, DsError> {
        let st = self.state.read().await;
        let cfg = st
            .configs
            .get(id)
            .cloned()
            .ok_or_else(|| DsError::NotFound(id.to_string()))?;
        if !cfg.enabled {
            return Err(DsError::Disabled(cfg.name));
        }
        Ok(cfg)
    }

    /// Build a "type does not support this operation" error.
    fn unsupported(&self, cfg: &DataSourceConfig) -> DsError {
        DsError::Unsupported(format!(
            "{}: type {} does not support this operation",
            cfg.name,
            cfg.kind.label()
        ))
    }

    /// Get (lazily build) the SQL connection pool.
    pub async fn sql_pool(&self, id: &str) -> Result<AnyPool, DsError> {
        {
            let st = self.state.read().await;
            if let Some(e) = st.sqls.get(id) {
                e.last_used_ms.store(now_ms(), Ordering::Relaxed);
                return Ok(e.pool.clone());
            }
        }
        let cfg = self.ensure_cfg(id).await?;
        if cfg.kind.is_redis() {
            return Err(self.unsupported(&cfg));
        }
        let pool = connect_sql(&cfg).await?;
        let mut st = self.state.write().await;
        if let Some(existing) = st.sqls.get(id) {
            existing.last_used_ms.store(now_ms(), Ordering::Relaxed);
            return Ok(existing.pool.clone());
        }
        st.sqls.insert(
            id.to_string(),
            SqlEntry {
                pool: pool.clone(),
                last_used_ms: AtomicU64::new(now_ms()),
            },
        );
        Ok(pool)
    }

    /// Get (lazily build) the Redis connection manager.
    pub async fn redis_manager(&self, id: &str) -> Result<ConnectionManager, DsError> {
        {
            let st = self.state.read().await;
            if let Some(e) = st.redises.get(id) {
                e.last_used_ms.store(now_ms(), Ordering::Relaxed);
                return Ok(e.manager.clone());
            }
        }
        let cfg = self.ensure_cfg(id).await?;
        if !cfg.kind.is_redis() {
            return Err(self.unsupported(&cfg));
        }
        let manager = connect_redis(&cfg).await?;
        let mut st = self.state.write().await;
        if let Some(existing) = st.redises.get(id) {
            existing.last_used_ms.store(now_ms(), Ordering::Relaxed);
            return Ok(existing.manager.clone());
        }
        st.redises.insert(
            id.to_string(),
            RedisEntry {
                manager: manager.clone(),
                last_used_ms: AtomicU64::new(now_ms()),
            },
        );
        Ok(manager)
    }

    /// Execute read-only SQL and return a structured result.
    pub async fn query_sql(&self, id: &str, sql: &str) -> Result<QueryResult, DsError> {
        let pool = self.sql_pool(id).await?;
        let cfg = self.ensure_cfg(id).await?;
        sql_exec::query_sql(&pool, &cfg, sql).await
    }

    /// Execute a read-only Redis command and return a stringified result.
    pub async fn redis_cmd(&self, id: &str, args: &[String]) -> Result<String, DsError> {
        let mut manager = self.redis_manager(id).await?;
        let cfg = self.ensure_cfg(id).await?;
        redis_exec::command(&mut manager, &cfg, args).await
    }

    /// Test connection: return latency and server info (never persists a bad pool).
    pub async fn test(&self, id: &str) -> DsTestReport {
        let cfg = match self.ensure_cfg(id).await {
            Ok(c) => c,
            Err(e) => return self.fail_report(id, &e),
        };
        // Dispatch between Redis and SQL
        if cfg.kind.is_redis() {
            let started = std::time::Instant::now();
            match self.redis_manager(id).await {
                Ok(mut manager) => {
                    let pong = tokio::time::timeout(
                        Duration::from_millis(cfg.query_timeout_ms.max(1)),
                        redis::cmd("PING").query_async::<String>(&mut manager),
                    )
                    .await;
                    let latency = started.elapsed().as_millis() as u64;
                    match pong {
                        Ok(Ok(_)) => DsTestReport {
                            id: id.to_string(),
                            name: cfg.name.clone(),
                            kind: cfg.kind.label().to_string(),
                            ok: true,
                            latency_ms: latency,
                            detail: Some("PONG".to_string()),
                            error: None,
                        },
                        Ok(Err(e)) => DsTestReport {
                            id: id.to_string(),
                            name: cfg.name.clone(),
                            kind: cfg.kind.label().to_string(),
                            ok: false,
                            latency_ms: latency,
                            detail: None,
                            error: Some(e.to_string()),
                        },
                        Err(_) => DsTestReport {
                            id: id.to_string(),
                            name: cfg.name.clone(),
                            kind: cfg.kind.label().to_string(),
                            ok: false,
                            latency_ms: latency,
                            detail: None,
                            error: Some("PING timed out".to_string()),
                        },
                    }
                }
                Err(e) => self.fail_report(id, &e),
            }
        } else {
            let started = std::time::Instant::now();
            match self.sql_pool(id).await {
                Ok(pool) => {
                    let version_sql = match cfg.kind {
                        DataSourceKind::Sqlite => "SELECT sqlite_version()",
                        DataSourceKind::Postgres => "SELECT version()",
                        DataSourceKind::MySql => "SELECT version()",
                        DataSourceKind::Redis => unreachable!(),
                    };
                    let probe = tokio::time::timeout(
                        Duration::from_millis(cfg.query_timeout_ms.max(1)),
                        sqlx::query(version_sql).fetch_one(&pool),
                    )
                    .await;
                    let latency = started.elapsed().as_millis() as u64;
                    match probe {
                        Ok(Ok(row)) => {
                            let detail = crate::sql::cell_text(&row, 0)
                                .ok()
                                .filter(|s| !s.is_empty());
                            DsTestReport {
                                id: id.to_string(),
                                name: cfg.name.clone(),
                                kind: cfg.kind.label().to_string(),
                                ok: true,
                                latency_ms: latency,
                                detail,
                                error: None,
                            }
                        }
                        Ok(Err(e)) => DsTestReport {
                            id: id.to_string(),
                            name: cfg.name.clone(),
                            kind: cfg.kind.label().to_string(),
                            ok: false,
                            latency_ms: latency,
                            detail: None,
                            error: Some(e.to_string()),
                        },
                        Err(_) => DsTestReport {
                            id: id.to_string(),
                            name: cfg.name.clone(),
                            kind: cfg.kind.label().to_string(),
                            ok: false,
                            latency_ms: latency,
                            detail: None,
                            error: Some("probe query timed out".to_string()),
                        },
                    }
                }
                Err(e) => self.fail_report(id, &e),
            }
        }
    }

    fn fail_report(&self, id: &str, e: &DsError) -> DsTestReport {
        DsTestReport {
            id: id.to_string(),
            name: id.to_string(),
            kind: String::new(),
            ok: false,
            latency_ms: 0,
            detail: None,
            error: Some(e.message()),
        }
    }

    /// Idle reclamation: close connections not accessed for over `idle_ttl_secs` (default call period 60s).
    pub async fn sweep_idle(&self) {
        let now = now_ms();
        let mut st = self.state.write().await;
        let mut sql_removed: Vec<String> = Vec::new();
        for (id, e) in st.sqls.iter() {
            let ttl = st
                .configs
                .get(id)
                .map(|c| c.idle_ttl_secs.max(30) * 1000)
                .unwrap_or(300_000);
            if now.saturating_sub(e.last_used_ms.load(Ordering::Relaxed)) > ttl {
                sql_removed.push(id.clone());
            }
        }
        for id in sql_removed {
            st.sqls.remove(&id);
        }
        let mut redis_removed: Vec<String> = Vec::new();
        for (id, e) in st.redises.iter() {
            let ttl = st
                .configs
                .get(id)
                .map(|c| c.idle_ttl_secs.max(30) * 1000)
                .unwrap_or(300_000);
            if now.saturating_sub(e.last_used_ms.load(Ordering::Relaxed)) > ttl {
                redis_removed.push(id.clone());
            }
        }
        for id in redis_removed {
            st.redises.remove(&id);
        }
    }
}

/// Build a SQL connection pool by type.
async fn connect_sql(cfg: &DataSourceConfig) -> Result<AnyPool, DsError> {
    sqlx::any::install_default_drivers();
    let url = build_conn(cfg);
    let opts = AnyPoolOptions::new()
        .max_connections(cfg.max_connections.max(1))
        .min_connections(cfg.min_idle)
        .acquire_timeout(Duration::from_millis(cfg.acquire_timeout_ms.max(100)));
    // Connect timeout: fall back to an outer tokio::timeout
    let ct = Duration::from_millis(cfg.connect_timeout_ms.max(100));
    let pool = tokio::time::timeout(ct, opts.connect(&url))
        .await
        .map_err(|_| DsError::Timeout {
            ctx: format!("{} connection timed out", cfg.name),
        })?
        .map_err(|e| DsError::Connect {
            name: cfg.name.clone(),
            detail: e.to_string(),
        })?;
    Ok(pool)
}

/// Build a Redis connection manager.
async fn connect_redis(cfg: &DataSourceConfig) -> Result<ConnectionManager, DsError> {
    let url = build_conn(cfg);
    let client = redis::Client::open(url.as_str()).map_err(|e| DsError::Connect {
        name: cfg.name.clone(),
        detail: format!("invalid URL: {e}"),
    })?;
    let timeout = Duration::from_millis(cfg.connect_timeout_ms.max(100));
    let manager = tokio::time::timeout(timeout, client.get_connection_manager())
        .await
        .map_err(|_| DsError::Timeout {
            ctx: format!("{} Redis connection timed out", cfg.name),
        })?
        .map_err(|e| DsError::Connect {
            name: cfg.name.clone(),
            detail: e.to_string(),
        })?;
    Ok(manager)
}

/// Append `mode=rwc` to SQLite file databases automatically (allows creating missing files); if mode exists or not sqlite, return as-is.
fn ensure_sqlite_rwc(url: &str) -> String {
    if url.starts_with("sqlite:") && !url.contains("mode=") {
        format!("{url}?mode=rwc")
    } else {
        url.to_string()
    }
}

/// Resolve `{{env:NAME}}` and `${NAME}` env references in the connection string (kept as-is when not found, leaving connection errors to surface).
fn resolve_env_config(mut cfg: DataSourceConfig) -> DataSourceConfig {
    cfg.url = resolve_env(&cfg.url);
    if let Some(u) = &cfg.username {
        cfg.username = Some(resolve_env(u));
    }
    if let Some(p) = &cfg.password {
        cfg.password = Some(resolve_env(p));
    }
    cfg
}

/// Assemble the actual connection string: resolve `{{env:}}` / `${}` references and merge in separately entered username / password
/// (kept as-is when the URL already has userinfo, to avoid overwriting).
fn build_conn(cfg: &DataSourceConfig) -> String {
    let cfg = resolve_env_config(cfg.clone());
    let url = if cfg.kind == orbit_config::DataSourceKind::Sqlite {
        ensure_sqlite_rwc(&cfg.url)
    } else {
        cfg.url
    };
    merge_credentials(url, cfg.username.as_deref(), cfg.password.as_deref())
}

/// When the connection string lacks userinfo and a username / password is set separately, inject `user:pass@`.
fn merge_credentials(url: String, username: Option<&str>, password: Option<&str>) -> String {
    let has_creds =
        username.is_some_and(|s| !s.is_empty()) || password.is_some_and(|s| !s.is_empty());
    if !has_creds {
        return url;
    }
    let Some(sep) = url.find("://") else {
        return url;
    };
    let after = &url[sep + 3..];
    let authority_end = after.find(['/', '?']).unwrap_or(after.len());
    let authority = &after[..authority_end];
    // Already has userinfo (e.g. redis://:pwd@host or user@host) -> do not overwrite
    if authority.contains('@') {
        return url;
    }
    let user = enc_seg(username.unwrap_or_default());
    let pass = enc_seg(password.unwrap_or_default());
    let mut out = String::with_capacity(url.len() + authority.len() + 8);
    out.push_str(&url[..sep + 3]);
    out.push_str(&user);
    out.push(':');
    out.push_str(&pass);
    out.push('@');
    out.push_str(after);
    out
}

/// Percent-encode the userinfo segment (keep RFC3986 unreserved chars, avoiding special chars breaking URL parsing).
fn enc_seg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn resolve_env(text: &str) -> String {
    let mut out = text.to_string();
    // {{env:NAME}}
    let mut changed = true;
    while changed {
        changed = false;
        if let Some(start) = out.find("{{env:") {
            let after = &out[start + "{{env:".len()..];
            if let Some(end) = after.find("}}") {
                let name = &after[..end];
                if let Ok(v) = std::env::var(name) {
                    out = out.replacen(&format!("{{{{env:{name}}}}}"), &v, 1);
                    changed = true;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    // ${NAME}
    let mut rest: &str = &out;
    let mut buf = String::new();
    while let Some(start) = rest.find("${") {
        buf.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            buf.push_str(rest);
            rest = "";
            break;
        };
        let name = &after[..end];
        if let Ok(v) = std::env::var(name) {
            buf.push_str(&v);
        } else {
            buf.push_str(&rest[start..=start + 2 + end]);
        }
        rest = &after[end + 1..];
    }
    buf.push_str(rest);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_config::DataSourceKind;
    use sqlx::any::AnyPoolOptions as AnyPoolOpt;

    #[test]
    fn merge_credentials_injects_when_missing() {
        let url = "postgres://db.example.com:5432/app".to_string();
        let merged = merge_credentials(url, Some("app_user"), Some("s3cret"));
        assert_eq!(merged, "postgres://app_user:s3cret@db.example.com:5432/app");
    }

    #[test]
    fn merge_credentials_keeps_existing_userinfo() {
        let url = "postgres://app_user:s3cret@h:30004/db".to_string();
        let merged = merge_credentials(url, Some("other"), Some("pwd"));
        assert_eq!(merged, "postgres://app_user:s3cret@h:30004/db");
        // The common redis `:pwd@host` form is likewise not overwritten
        let rurl = "redis://:secret@h:6379/0".to_string();
        assert_eq!(
            merge_credentials(rurl, Some("u"), Some("p")),
            "redis://:secret@h:6379/0"
        );
    }

    #[test]
    fn merge_credentials_percent_encodes() {
        let merged = merge_credentials(
            "mysql://h:3306/db".to_string(),
            Some("my@user"),
            Some("p:ss/word"),
        );
        assert_eq!(merged, "mysql://my%40user:p%3Ass%2Fword@h:3306/db");
    }

    #[test]
    fn resolve_env_works() {
        std::env::set_var("ORBIT_TEST_DS_PWD", "secret");
        assert_eq!(
            resolve_env("redis://:{{env:ORBIT_TEST_DS_PWD}}@h:1/0"),
            "redis://:secret@h:1/0"
        );
        assert_eq!(resolve_env("${ORBIT_TEST_DS_PWD}@host"), "secret@host");
        std::env::remove_var("ORBIT_TEST_DS_PWD");
        // Kept as-is when missing, avoiding panic
        assert_eq!(resolve_env("${MISSING_X}"), "${MISSING_X}");
    }

    /// Real SQLite query chain: lazy connection -> query stringification -> test connection.
    #[tokio::test]
    async fn sqlite_lazy_query_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("orbit-ds-test-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!(
            "sqlite:{}",
            dir.join("t.db").to_string_lossy().replace('\\', "/")
        );

        // Preset the table schema and data (via a separate connection; registry queries stay readonly)
        sqlx::any::install_default_drivers();
        let setup = AnyPoolOpt::new()
            .connect(&format!("{url}?mode=rwc"))
            .await
            .expect("create test sqlite");
        sqlx::query("CREATE TABLE orders (id INTEGER, status TEXT)")
            .execute(&setup)
            .await
            .unwrap();
        sqlx::query("INSERT INTO orders VALUES (1, 'PAID')")
            .execute(&setup)
            .await
            .unwrap();
        setup.close().await;

        let reg = DataSourceRegistry::new();
        reg.register(DataSourceConfig {
            id: "orders-db".into(),
            name: "orders-db".into(),
            kind: DataSourceKind::Sqlite,
            url: url.clone(),
            readonly: true,
            ..Default::default()
        })
        .await;

        let result = reg
            .query_sql("orders-db", "SELECT id, status FROM orders")
            .await;
        assert!(result.is_ok(), "{:?}", result.err());
        let result = result.unwrap();
        assert_eq!(result.columns, vec!["id", "status"]);
        assert_eq!(result.rows, vec![vec!["1".to_string(), "PAID".to_string()]]);

        // Read-only protection takes effect
        let write = reg.query_sql("orders-db", "DELETE FROM orders").await;
        assert!(matches!(write, Err(DsError::Readonly(_))));

        let report = reg.test("orders-db").await;
        assert!(report.ok, "test() should connect: {:?}", report.error);
        assert!(
            report
                .detail
                .unwrap_or_default()
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false),
            "sqlite version should be returned"
        );
    }
}
