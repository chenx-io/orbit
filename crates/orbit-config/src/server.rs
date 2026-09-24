//! Server-side config (open-source extension points): external service endpoints and behavior switches.
//!
//! Intended integration path = "drop in a plugin + edit config, no code changes":
//! - The event sink (audit / metering) is wired up by an add-on package subscription or a webhook
//!
//! The open-source library only defines structures and loads config; it contains no licensing / gating / metering logic.
//! The app runs fully offline: no accounts, members or remote sync capability is provided.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Open-source server-side config (purely local).
///
/// Load path: `~/.orbit/config.toml` (all defaults when absent).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Extra plugin directories (defaults to `ORBIT_PLUGIN_DIR` / `~/.orbit/plugins`)
    #[serde(default)]
    pub plugin_dirs: Vec<PathBuf>,
    /// Event webhook URL (audit / metering subscription; empty = SSE subscription only)
    #[serde(default)]
    pub event_sink: Option<String>,
}

impl ServerConfig {
    /// Parse from TOML text (convenient for tests and external injection).
    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        toml::from_str(input).map_err(|e| format!("failed to parse ServerConfig: {e}"))
    }

    /// Load from `~/.orbit/config.toml`; returns the default config when the file is missing.
    pub fn load() -> Self {
        Self::load_from(home_dir().join(".orbit").join("config.toml"))
    }

    /// Load from a given path (convenient for tests injecting a home dir).
    pub fn load_from(path: PathBuf) -> Self {
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_toml_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Server config loading (run directory / argument / environment variable conventions):
    /// 1) explicit `config_path` -> 2) run directory `orbit-server.toml` -> 3) `~/.orbit/config.toml` as fallback;
    ///
    /// `ORBIT_*` environment variable overrides are applied afterwards.
    pub fn load_for_server(config_path: Option<PathBuf>) -> Result<Self, String> {
        let path = config_path
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|d| d.join("orbit-server.toml"))
            })
            .unwrap_or_else(|| home_dir().join(".orbit").join("config.toml"));
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("failed to read config {}: {}", path.display(), e))?;
            Self::from_toml_str(&text)?
        } else {
            Self::default()
        };
        cfg.apply_env_overrides();
        Ok(cfg)
    }

    /// Environment variable overrides (server deployment convention): `ORBIT_EVENT_SINK`.
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("ORBIT_EVENT_SINK") {
            if !v.trim().is_empty() {
                self.event_sink = Some(v.trim().to_string());
            }
        }
    }
}

/// Current user home (also accepts Windows `USERPROFILE`).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let cfg = ServerConfig::from_toml_str(
            r#"
plugin_dirs = ["/opt/orbit/enterprise-plugins"]
event_sink = "https://audit.example.com/events"
"#,
        )
        .expect("valid TOML should parse");
        assert_eq!(cfg.plugin_dirs.len(), 1);
        assert_eq!(
            cfg.plugin_dirs[0],
            std::path::PathBuf::from("/opt/orbit/enterprise-plugins")
        );
        assert_eq!(
            cfg.event_sink.as_deref(),
            Some("https://audit.example.com/events")
        );
    }

    #[test]
    fn unknown_sections_are_ignored() {
        // Legacy [auth]/[sync]/[cloud]/[online] sections left in old configs are ignored rather than failing parsing
        let cfg = ServerConfig::from_toml_str(
            r#"
plugin_dirs = []

[auth]
mode = "enforce"

[online]
db_url = "sqlite:///opt/orbit/orbit.db"
"#,
        )
        .expect("unknown config sections should be ignored");
        assert!(cfg.plugin_dirs.is_empty());
        assert!(cfg.event_sink.is_none());
    }

    #[test]
    fn empty_config_uses_defaults() {
        let cfg = ServerConfig::from_toml_str("").expect("empty TOML should parse");
        assert!(cfg.plugin_dirs.is_empty());
        assert!(cfg.event_sink.is_none());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir().join(format!(
            "orbit-server-config-missing-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = ServerConfig::load_from(path);
        assert!(cfg.plugin_dirs.is_empty());
        assert!(cfg.event_sink.is_none());
    }

    #[test]
    fn env_overrides_apply() {
        std::env::set_var("ORBIT_EVENT_SINK", "https://audit.example.com/events");
        let cfg = ServerConfig::load_for_server(None).unwrap();
        assert_eq!(
            cfg.event_sink.as_deref(),
            Some("https://audit.example.com/events")
        );
        std::env::remove_var("ORBIT_EVENT_SINK");
    }
}
