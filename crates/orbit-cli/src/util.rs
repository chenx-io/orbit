//! Small helpers shared by commands (read plan / locate data files / print separators)

use std::path::{Path, PathBuf};

use orbit_config::TestPlan;

/// Read and parse a test plan YAML
pub fn read_plan(file: &Path) -> anyhow::Result<TestPlan> {
    let yaml = std::fs::read_to_string(file)?;
    orbit_config::from_str(&yaml).map_err(|e| anyhow::anyhow!("{}: {}", file.display(), e))
}

/// User home directory (Windows-compatible)
pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Application data root: ~/.orbit
pub fn data_root() -> PathBuf {
    home_dir().join(".orbit")
}

/// Data snapshot file: ~/.orbit/orbit_data.json (also accepts the pre-migration chenx_data.json)
pub fn data_file() -> PathBuf {
    let dir = data_root();
    let cur = dir.join("orbit_data.json");
    if cur.exists() {
        cur
    } else {
        dir.join("chenx_data.json")
    }
}

/// Return a consistent separator line
pub fn separator() -> String {
    "───────────────────────────────────────────".to_string()
}
