//! # orbit-datasource
//!
//! Data source connection management and read-only execution (DB: SQLite / PostgreSQL / MySQL; cache: Redis).
//!
//! Composition:
//! - [`registry::DataSourceRegistry`]: global registry -- lazy connections, connection-pool caching,
//!   health probes (`SELECT 1` / `PING`), test connection, automatic idle reclamation; `Clone` shares the same state
//! - [`registry::DsTestReport`]: test connection report (latency / server info / error)
//! - [`provider`]: adapts the registry to the assertion engine's [`orbit_assertion::DataSourceProvider`],
//!   letting DB / Redis assertions run read-only queries after a request
//! - Config model [`orbit_config::DataSourceConfig`] / [`orbit_config::DataSourceKind`]
//!   provided by `orbit-config` (pure serde, no IO); re-exported here for uniform referencing
//!
//! Security constraints: data sources default to `readonly`; SQL allows only `SELECT/WITH/SHOW/EXPLAIN/PRAGMA...`,
//! Redis allows only a read-only command allowlist; queries have a timeout and a row limit.

pub mod error;
pub mod provider;
pub mod registry;

mod redis;
mod sql;

pub use error::DsError;
pub use orbit_config::{DataSourceConfig, DataSourceKind};
pub use registry::{DataSourceRegistry, DsTestReport};
