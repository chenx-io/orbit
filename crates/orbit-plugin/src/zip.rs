//! Plugin zip package install: validate -> zip-slip-safe extraction -> size limit -> extract to `<plugins-root>/<id>/`.
//!
//! The zip top level is the plugin directory (`<plugin-id>/manifest.json + <entry>.wasm [+ assets/]`).
//! Security constraints:
//! - Path traversal (zip-slip) protection: after normalization must not escape the target directory;
//! - Total extracted size limit 20MB;
//! - Plugin id allowlist `[a-z0-9.-]`.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::manifest::PluginManifest;

/// Default plugin package size limit (20MB, consistent with the design doc)
pub const MAX_UNPACK_BYTES: u64 = 20 * 1024 * 1024;

/// Validate whether a plugin id matches the allowlist `[a-z0-9.-]` (no path separators)
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// Normalize a zip entry path into a safe path under the target directory; returns None if it escapes.
///
/// zip-slip protection: after normalization (drop `.` / resolve `..`) the path must stay under `dest`.
fn safe_join(dest: &Path, entry_name: &str) -> Option<PathBuf> {
    let normalized: PathBuf = Path::new(entry_name)
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if normalized
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None; // `..` escapes
    }
    if normalized.as_os_str().is_empty() {
        return None; // directory entry
    }
    Some(dest.join(normalized))
}

/// Extract zip bytes into the target directory; returns the extracted manifest (for later loading).
///
/// `root` is `<plugins-root>`; the extraction target is `root/<plugin-id>/` (determined by the zip top-level directory).
/// Safety: size limit + zip-slip protection; clears existing content under the target directory first.
pub fn install_zip(root: &Path, bytes: &[u8]) -> Result<(String, PluginManifest), String> {
    if bytes.len() > MAX_UNPACK_BYTES as usize {
        return Err(format!(
            "plugin zip exceeds size limit {}MB",
            MAX_UNPACK_BYTES / 1024 / 1024
        ));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("zip parse failed: {}", e))?;

    // 1) Find the top-level plugin directory (topmost path segment of entries containing manifest.json)
    let mut top_dir: Option<String> = None;
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        if name.ends_with("manifest.json") {
            top_dir = name
                .trim_end_matches("manifest.json")
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(_, leaf)| leaf.to_string())
                .or_else(|| {
                    let t = name.trim_end_matches("manifest.json").trim_end_matches('/');
                    if !t.is_empty() && !t.contains('/') {
                        Some(t.to_string())
                    } else {
                        None
                    }
                });
            break;
        }
    }
    let Some(plugin_id) = top_dir else {
        return Err(
            "manifest.json not found in zip (the top level should be the plugin directory)".into(),
        );
    };
    if !valid_plugin_id(&plugin_id) {
        return Err(format!(
            "invalid plugin id '{}' (must match [a-z0-9.-])",
            plugin_id
        ));
    }

    // 2) Extraction target: the zip top level is `<root>/<plugin_id>/`; entry names already carry that prefix, so extract into root
    let dest_dir = root.join(&plugin_id);
    if dest_dir.exists() {
        fs::remove_dir_all(&dest_dir)
            .map_err(|e| format!("clean up old plugin directory: {}", e))?;
    }
    fs::create_dir_all(&dest_dir).map_err(|e| format!("create plugin directory: {}", e))?;
    let dest = root.to_path_buf();

    // 3) Sequential extraction (cumulative size controlled)
    let mut total: u64 = 0;
    let mut manifest_text: Option<String> = None;
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {}: {}", i, e))?;
        let entry_name = f.name().to_string();
        let target = match safe_join(&dest, &entry_name) {
            Some(p) => p,
            None => {
                let _ = fs::remove_dir_all(&dest);
                return Err(format!(
                    "zip entry '{}' contains path traversal (zip-slip), rejected",
                    entry_name
                ));
            }
        };
        if f.is_dir() {
            fs::create_dir_all(&target).ok();
            continue;
        }
        total += f.size();
        if total > MAX_UNPACK_BYTES {
            let _ = fs::remove_dir_all(&dest);
            return Err(format!(
                "plugin extraction exceeds size limit {}MB",
                MAX_UNPACK_BYTES / 1024 / 1024
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).ok();
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .map_err(|e| format!("read zip entry '{}': {}", entry_name, e))?;
        fs::write(&target, &buf).map_err(|e| format!("write '{}': {}", target.display(), e))?;
        if entry_name.ends_with("manifest.json") {
            manifest_text = Some(String::from_utf8_lossy(&buf).to_string());
        }
    }

    // 4) Validate manifest
    let manifest_text =
        manifest_text.ok_or_else(|| "manifest.json not found after extraction".to_string())?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("manifest.json parse failed: {}", e))?;
    if manifest.id != plugin_id {
        let _ = fs::remove_dir_all(&dest);
        return Err(format!(
            "manifest.id '{}' does not match the top-level directory '{}'",
            manifest.id, plugin_id
        ));
    }
    if !manifest.is_valid() {
        let _ = fs::remove_dir_all(&dest);
        return Err(
            "invalid manifest (kind/entry/connectionConfigSchema validation failed)".into(),
        );
    }
    Ok((plugin_id, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("orbit-zip-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, content) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(content).unwrap();
        }
        w.finish().unwrap();
        out.into_inner()
    }

    const VALID_MANIFEST: &str = r#"{
        "id": "com.example.echo",
        "name": "Echo",
        "version": "1.0.0",
        "type": "protocol",
        "entry": "echo.wasm",
        "connectionConfigSchema": { "type": "object", "properties": { "prefix": { "type": "string" } } }
    }"#;

    #[test]
    fn test_install_valid_zip() {
        let root = tmp();
        let bytes = make_zip(&[
            ("com.example.echo/manifest.json", VALID_MANIFEST.as_bytes()),
            ("com.example.echo/echo.wasm", b"\0asm"),
        ]);
        let (id, m) = install_zip(&root, &bytes).unwrap();
        assert_eq!(id, "com.example.echo");
        assert_eq!(m.entry, "echo.wasm");
        assert!(root.join("com.example.echo/manifest.json").exists());
        assert!(root.join("com.example.echo/echo.wasm").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn test_reject_zip_slip() {
        let root = tmp();
        // ../escape escapes the root
        let bytes = make_zip(&[
            ("com.example.echo/../../evil.sh", b"boom"),
            ("com.example.echo/manifest.json", VALID_MANIFEST.as_bytes()),
        ]);
        let err = install_zip(&root, &bytes).unwrap_err();
        assert!(err.contains("path traversal"), "err: {}", err);
        assert!(!root.join("evil.sh").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn test_reject_missing_manifest() {
        let root = tmp();
        let bytes = make_zip(&[("com.example.echo/echo.wasm", b"\0asm")]);
        let err = install_zip(&root, &bytes).unwrap_err();
        assert!(err.contains("manifest.json"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn test_reject_bad_id() {
        let root = tmp();
        let bytes = make_zip(&[("Bad/Id/manifest.json", VALID_MANIFEST.as_bytes())]);
        let err = install_zip(&root, &bytes).unwrap_err();
        assert!(err.contains("invalid"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn test_valid_plugin_id() {
        assert!(valid_plugin_id("com.example.dubbo-client"));
        assert!(valid_plugin_id("a1-b2"));
        assert!(!valid_plugin_id("Bad/Id"));
        assert!(!valid_plugin_id("has space"));
        assert!(!valid_plugin_id(""));
        assert!(!valid_plugin_id("UPPER"));
    }
}
