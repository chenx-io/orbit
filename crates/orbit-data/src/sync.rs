//! Snapshot sync abstraction (open-source extension point): cloud sync is implemented by an add-on package / standalone service.
//!
//! Snapshot = unit of sync: one JSON document carries all user data and is isomorphic locally and remotely.
//! The open-source side only defines the interface contract and the `NoopSyncProvider` default; real remote sync
//! (upload / download / merge) is implemented by the add-on package as a standalone service or an `HttpSyncProvider`.

use async_trait::async_trait;

use crate::model::Snapshot;

/// Sync error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SyncError {
    #[error("sync not configured (noop mode)")]
    NotConfigured,
    #[error("remote sync failed: {0}")]
    Remote(String),
    #[error("snapshot version conflict")]
    Conflict,
}

/// Snapshot sync abstraction (defined here, implemented by the add-on package).
#[async_trait]
pub trait SyncProvider: Send + Sync {
    /// Pulls the remote snapshot (first sync or merge base).
    async fn pull(&self) -> Result<Snapshot, SyncError>;

    /// Pushes the local snapshot; `base_saved_at` is the optimistic-lock base (`Snapshot.saved_at`).
    async fn push(&self, snapshot: &Snapshot, base_saved_at: Option<i64>) -> Result<(), SyncError>;
}

/// Default implementation: a no-op when remote sync is not configured (only returns [`SyncError::NotConfigured`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSyncProvider;

#[async_trait]
impl SyncProvider for NoopSyncProvider {
    async fn pull(&self) -> Result<Snapshot, SyncError> {
        Err(SyncError::NotConfigured)
    }

    async fn push(
        &self,
        _snapshot: &Snapshot,
        _base_saved_at: Option<i64>,
    ) -> Result<(), SyncError> {
        Err(SyncError::NotConfigured)
    }
}

/// Local directory sync: mirrors the snapshot into `dir/orbit_data.json`,
/// using `Snapshot.saved_at` for optimistic-lock conflict detection (`base_saved_at` differing from the remote → Conflict).
pub struct FileSyncProvider {
    dir: std::path::PathBuf,
}

impl FileSyncProvider {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self) -> std::path::PathBuf {
        self.dir.join("orbit_data.json")
    }
}

#[async_trait]
impl SyncProvider for FileSyncProvider {
    async fn pull(&self) -> Result<Snapshot, SyncError> {
        let path = self.path();
        if !path.exists() {
            return Err(SyncError::Remote(format!(
                "no snapshot in the sync directory: {}",
                path.display()
            )));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| SyncError::Remote(e.to_string()))?;
        serde_json::from_str(&text)
            .map_err(|e| SyncError::Remote(format!("failed to parse synced snapshot: {e}")))
    }

    async fn push(&self, snapshot: &Snapshot, base_saved_at: Option<i64>) -> Result<(), SyncError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| SyncError::Remote(e.to_string()))?;
        let path = self.path();
        if let Some(base) = base_saved_at {
            if path.exists() {
                let text =
                    std::fs::read_to_string(&path).map_err(|e| SyncError::Remote(e.to_string()))?;
                let remote: Snapshot = serde_json::from_str(&text).map_err(|e| {
                    SyncError::Remote(format!("failed to parse remote snapshot: {e}"))
                })?;
                if remote.saved_at != base {
                    return Err(SyncError::Conflict);
                }
            }
        }
        let json = serde_json::to_string_pretty(snapshot)
            .map_err(|e| SyncError::Remote(format!("failed to serialize snapshot: {e}")))?;
        std::fs::write(&path, json).map_err(|e| SyncError::Remote(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample_snapshot() -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "savedAt": 1780000000000_i64,
            "source": "web",
            "sync": { "remoteUrl": null, "lastSyncedAt": null },
            "data": {
                "collections": [], "requests": {}, "models": [], "environments": [],
                "activeEnvId": null, "globalVariables": {}, "globalSecrets": {},
                "scenarios": [], "plugins": [], "history": [], "mockRules": [],
                "locale": "zh-CN", "theme": "dark",
                "ui": { "sidebarCollapsed": false },
                "executionTarget": { "mode": "local", "agentIds": null }
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn noop_provider_returns_not_configured() {
        let provider = NoopSyncProvider;
        assert!(matches!(
            provider.pull().await,
            Err(SyncError::NotConfigured)
        ));
        assert!(matches!(
            provider.push(&sample_snapshot(), None).await,
            Err(SyncError::NotConfigured)
        ));
    }

    /// Verifies the trait can be injected by an external implementation (the add-on package shape).
    struct FakeSync;

    #[async_trait]
    impl SyncProvider for FakeSync {
        async fn pull(&self) -> Result<Snapshot, SyncError> {
            Ok(sample_snapshot())
        }

        async fn push(
            &self,
            _snapshot: &Snapshot,
            _base_saved_at: Option<i64>,
        ) -> Result<(), SyncError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn trait_object_is_injectable() {
        let provider: Arc<dyn SyncProvider> = Arc::new(FakeSync);
        assert!(provider.pull().await.is_ok());
        let snap = sample_snapshot();
        assert!(provider.push(&snap, None).await.is_ok());
    }

    fn tmp_sync_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit-sync-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn file_sync_push_then_pull_roundtrip() {
        let dir = tmp_sync_dir("roundtrip");
        let provider = FileSyncProvider::new(dir.clone());
        let mut snap = sample_snapshot();
        snap.saved_at = 12345;
        provider.push(&snap, None).await.unwrap();

        let pulled = provider.pull().await.unwrap();
        assert_eq!(pulled.saved_at, 12345);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_sync_conflict_on_stale_base() {
        let dir = tmp_sync_dir("conflict");
        let provider = FileSyncProvider::new(dir.clone());
        let mut snap = sample_snapshot();
        snap.saved_at = 100;
        provider.push(&snap, None).await.unwrap();

        // Push with a stale base → conflict
        let mut newer = sample_snapshot();
        newer.saved_at = 200;
        let err = provider.push(&newer, Some(999)).await.unwrap_err();
        assert!(matches!(err, SyncError::Conflict));

        // Matching base → success
        provider.push(&newer, Some(100)).await.unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_sync_pull_from_empty_dir_errors() {
        let dir = tmp_sync_dir("empty");
        let provider = FileSyncProvider::new(dir.clone());
        assert!(provider.pull().await.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
