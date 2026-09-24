//! Datasource config model (pure serde definitions, no connection logic).
//!
//! Carries connection info (kind / url / pool parameters / safety switches) shared by UI snapshots, test plan YAML,
//! server config files and the `orbit-datasource` runtime registry.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Datasource kind (extension point: append a variant here for a new database engine and implement the matching driver in the registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSourceKind {
    #[default]
    MySql,
    Postgres,
    Sqlite,
    Redis,
}

impl DataSourceKind {
    /// Standard URL scheme prefix (used for connection string parsing and display).
    pub fn scheme(&self) -> &'static str {
        match self {
            DataSourceKind::MySql => "mysql",
            DataSourceKind::Postgres => "postgres",
            DataSourceKind::Sqlite => "sqlite",
            DataSourceKind::Redis => "redis",
        }
    }

    /// Display name
    pub fn label(&self) -> &'static str {
        match self {
            DataSourceKind::MySql => "MySQL",
            DataSourceKind::Postgres => "PostgreSQL",
            DataSourceKind::Sqlite => "SQLite",
            DataSourceKind::Redis => "Redis",
        }
    }

    /// Whether this is a relational database (uses SQL queries).
    pub fn is_sql(&self) -> bool {
        !matches!(self, DataSourceKind::Redis)
    }

    /// Whether this is a key-value cache (uses commands).
    pub fn is_redis(&self) -> bool {
        matches!(self, DataSourceKind::Redis)
    }
}

impl std::fmt::Display for DataSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.scheme())
    }
}

impl std::str::FromStr for DataSourceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "mysql" | "mariadb" => Ok(DataSourceKind::MySql),
            "postgres" | "postgresql" | "pg" => Ok(DataSourceKind::Postgres),
            "sqlite" | "sqlite3" => Ok(DataSourceKind::Sqlite),
            "redis" => Ok(DataSourceKind::Redis),
            other => Err(format!("unsupported datasource kind: {other}")),
        }
    }
}

/// Datasource connection config (global datasource management entity).
///
/// Passwords should reference runtime environment variables as `{{env:VAR}}`; when stored in plain text they are only kept locally,
/// and listings / exports / logs always go through [`mask_password`] for masking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSourceConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: DataSourceKind,
    /// Connection string (supports `{{env:VAR}}` / `${VAR}` interpolation), e.g.:
    /// `mysql://user:pass@host:3306/db` / `sqlite:/path/orbit.db` / `redis://host:6379/0`
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Maximum number of pool connections
    #[serde(default = "d_max_connections")]
    pub max_connections: u32,
    /// Minimum number of idle pool connections
    #[serde(default = "d_min_idle")]
    pub min_idle: u32,
    /// Connect timeout (milliseconds)
    #[serde(default = "d_connect_timeout")]
    pub connect_timeout_ms: u64,
    /// Timeout for acquiring a connection from the pool (milliseconds)
    #[serde(default = "d_acquire_timeout")]
    pub acquire_timeout_ms: u64,
    /// Timeout for a single query (milliseconds)
    #[serde(default = "d_query_timeout")]
    pub query_timeout_ms: u64,
    /// Idle reclamation threshold (seconds): connections idle longer than this and unreferenced are closed by a background task
    #[serde(default = "d_idle_ttl")]
    pub idle_ttl_secs: u64,
    /// Read-only protection: when on, only read-only SQL / read-only Redis commands are allowed
    #[serde(default = "default_true")]
    pub readonly: bool,
    /// Whether enabled (a disabled connection takes part in no query)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Owning workspace id (for snapshot persistence; empty = globally shared)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Note
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn d_max_connections() -> u32 {
    8
}
fn d_min_idle() -> u32 {
    1
}
fn d_connect_timeout() -> u64 {
    5000
}
fn d_acquire_timeout() -> u64 {
    5000
}
fn d_query_timeout() -> u64 {
    10000
}
fn d_idle_ttl() -> u64 {
    300
}

impl DataSourceConfig {
    /// Produce a masked copy of the config (password shown as `••••••`) for listing / export / log display.
    pub fn masked(&self) -> Self {
        let mut out = self.clone();
        if out.password.is_some() {
            out.password = Some("••••••".to_string());
        }
        out
    }
}

impl Default for DataSourceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: DataSourceKind::default(),
            url: String::new(),
            username: None,
            password: None,
            max_connections: d_max_connections(),
            min_idle: d_min_idle(),
            connect_timeout_ms: d_connect_timeout(),
            acquire_timeout_ms: d_acquire_timeout(),
            query_timeout_ms: d_query_timeout(),
            idle_ttl_secs: d_idle_ttl(),
            readonly: true,
            enabled: true,
            workspace_id: None,
            note: None,
        }
    }
}
