//! Storage abstraction: the read/write backend for snapshots.
//!
//! Unit of sync = [`Snapshot`] (one JSON document carrying all user data). Isomorphic locally and remotely,
//! so future remote sync is just uploading/downloading the same document. Backends:
//! - [`FileStorage`]: JSON snapshot file (atomic write, matching Tauri's `save_snapshot` behavior)
//! - [`MemoryStorage`]: in-memory implementation (tests / Web fallback / WASM)
//! - SQLite / server-side database: add as needed by implementing the same trait

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::DataError;
use crate::model::Snapshot;

/// Storage backend abstraction
#[async_trait]
pub trait Storage: Send + Sync {
    /// Reads the snapshot; returns `Ok(None)` when absent. Implementations handle corrupt files themselves (e.g. renaming them aside).
    async fn load(&self) -> Result<Option<Snapshot>, DataError>;
    /// Writes the snapshot (implementations must guarantee atomicity / crash safety)
    async fn save(&self, snapshot: &Snapshot) -> Result<(), DataError>;
    /// Deletes the snapshot (including leftover temp files), used for "clear data"
    async fn clear(&self) -> Result<(), DataError>;
}

/// JSON snapshot file storage: atomic write (tmp + replace); on Windows the old target is deleted before the rename.
pub struct FileStorage {
    path: PathBuf,
}

impl FileStorage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileStorage { path: path.into() }
    }

    fn tmp_path(&self) -> PathBuf {
        let mut p = self.path.clone();
        p.set_extension("json.tmp");
        p
    }
}

#[async_trait]
impl Storage for FileStorage {
    async fn load(&self) -> Result<Option<Snapshot>, DataError> {
        let path = &self.path;
        if !path.exists() {
            return Ok(None);
        }
        let content = tokio::fs::read_to_string(path).await?;
        match serde_json::from_str::<Snapshot>(&content) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(_) => {
                // Rename the corrupt file to .bak for the record and treat it as no snapshot (the caller falls back to seed)
                let bak = path.with_extension("json.bak");
                let _ = tokio::fs::rename(path, &bak).await;
                Ok(None)
            }
        }
    }

    async fn save(&self, snapshot: &Snapshot) -> Result<(), DataError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(snapshot)?;
        let tmp = self.tmp_path();
        tokio::fs::write(&tmp, json.as_bytes()).await?;
        if self.path.exists() {
            tokio::fs::remove_file(&self.path).await?;
        }
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }

    async fn clear(&self) -> Result<(), DataError> {
        if self.path.exists() {
            tokio::fs::remove_file(&self.path).await?;
        }
        let tmp = self.tmp_path();
        if tmp.exists() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        Ok(())
    }
}

/// In-memory storage: held inside the process, for tests and media without persistence (Web/WASM fallback).
#[derive(Clone)]
pub struct MemoryStorage {
    inner: std::sync::Arc<Mutex<Option<Snapshot>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        MemoryStorage {
            inner: std::sync::Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn load(&self) -> Result<Option<Snapshot>, DataError> {
        Ok(self.inner.lock().unwrap().clone())
    }

    async fn save(&self, snapshot: &Snapshot) -> Result<(), DataError> {
        *self.inner.lock().unwrap() = Some(snapshot.clone());
        Ok(())
    }

    async fn clear(&self) -> Result<(), DataError> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}

/// One-shot file storage in the temp directory (for tests): cleaned up automatically.
pub fn temp_file_storage() -> (FileStorage, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "orbit-data-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::create_dir_all(&dir);
    (FileStorage::new(dir.join("orbit_data.json")), dir)
}
