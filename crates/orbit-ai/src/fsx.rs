//! Small file I/O helpers: atomic writes + directory creation.
//!
//! Credential and session files both require "either fully written or the old value kept", so they all go through
//! writing a temp file + rename (a same-directory rename is atomic on mainstream filesystems).

use std::path::Path;

use crate::error::{AiError, AiResult};

/// Make sure the directory exists.
pub fn ensure_dir(path: &Path) -> AiResult<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Write a text file atomically (parent directories are created automatically).
pub fn write_atomic(path: &Path, content: &[u8]) -> AiResult<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    // On Windows, renaming onto an existing target fails, so delete the target first
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read a text file; returns `None` when it does not exist (callers treat that as "first use").
pub fn read_optional(path: &Path) -> AiResult<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AiError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_roundtrip_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("orbit-ai-fsx-{}", uuid::Uuid::new_v4()));
        let file = dir.join("nested").join("a.txt");
        write_atomic(&file, b"hello").unwrap();
        assert_eq!(read_optional(&file).unwrap().as_deref(), Some("hello"));
        // An overwrite leaves no stray tmp file
        write_atomic(&file, b"world").unwrap();
        assert_eq!(read_optional(&file).unwrap().as_deref(), Some("world"));
        assert!(!file.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none_not_error() {
        let dir = std::env::temp_dir().join(format!("orbit-ai-fsx-{}", uuid::Uuid::new_v4()));
        ensure_dir(&dir).unwrap();
        assert!(read_optional(&dir.join("nope.json")).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
