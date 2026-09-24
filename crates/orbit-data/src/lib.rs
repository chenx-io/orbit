//! Orbit data management layer: domain models, storage abstraction and data service.
//!
//! Role: data ownership moves from the frontend store down into Rust,
//! so the same data logic is reused across Tauri / Web / WASM UI shapes.
//!
//! Composition:
//! - [`model`]: pure serde data models aligned 1:1 with the frontend snapshot structure (zero-cost migration of existing data)
//! - [`storage`]: storage abstraction (`Storage` trait) + file / memory backends
//! - [`service`]: `DataService` snapshot lifecycle + domain CRUD + import/export entry point
//! - [`error`]: error types
//!
//! Snapshot = unit of sync: one JSON document carries all user data and is isomorphic locally and remotely,
//! so future cloud sync is just uploading / downloading / merging the same document.

pub mod error;
pub mod export;
pub mod model;
pub mod service;
pub mod storage;
pub mod sync;

pub use error::DataError;
pub use export::{
    build_api_spec, collect_export_requests, collect_referenced_models, export_document,
    ExportRange, ExportRequest,
};
pub use model::*;
pub use service::DataService;
pub use storage::{FileStorage, MemoryStorage, Storage};
pub use sync::{FileSyncProvider, NoopSyncProvider, SyncError, SyncProvider};

/// Current snapshot version (aligned with the frontend `SNAPSHOT_VERSION`)
pub const SNAPSHOT_VERSION: u32 = 2;
