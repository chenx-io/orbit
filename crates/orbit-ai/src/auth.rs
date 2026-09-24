//! BYOK credential store: local persistence of user-supplied model API keys.
//!
//! Storage location: `<app_data_dir>/ai/credentials.yaml` (**never in snapshots, never in the repo**).
//! Why it is separate from snapshots: API keys must not spread through exported snapshot files, nor be committed through git projections.
//!
//! Security conventions:
//! - API keys are read only on the Rust side when issuing model requests;
//! - what is returned to the frontend is always [`CredentialView`] (**redacted**, exposing only hints of the form `sk-…abcd`);
//! - on update, passing `None` for `api_key` means keep the existing value (the frontend never needs to read the key back).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AiError, AiResult};
use crate::fsx::{ensure_dir, read_optional, write_atomic};

/// A single custom request header.
///
/// Why it exists: many vendors/gateways require extra headers besides the API key (e.g. OpenRouter's
/// `HTTP-Referer` / `X-Title`, tenant identifiers on some enterprise gateways), or need a non-standard header to carry the key
/// (`api-key: xxx`). Such differences must not be hard-coded into the Provider adapter layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderPair {
    /// Header name.
    pub key: String,
    /// Header value.
    #[serde(default)]
    pub value: String,
}

/// Normalize custom headers: trim whitespace, drop empty keys, and keep the last of identically named (case-insensitive) entries.
fn normalize_headers(list: Vec<HeaderPair>) -> Vec<HeaderPair> {
    let mut out: Vec<HeaderPair> = Vec::new();
    for item in list {
        let key = item.key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = item.value.trim().to_string();
        match out.iter_mut().find(|h| h.key.eq_ignore_ascii_case(&key)) {
            Some(existing) => existing.value = value,
            None => out.push(HeaderPair { key, value }),
        }
    }
    out
}

/// Auth header style: use the vendor's standard header (Anthropic `x-api-key` / Azure-style `api-key`),
/// or `Authorization: Bearer`.
///
/// The official Anthropic SDK offers exactly these two: `apiKey` -> `x-api-key`, `authToken` -> `Authorization: Bearer`
/// (mutually exclusive per request). Many gateways/proxies accept only Bearer, so the user must be able to choose; neither can be hard-coded.
pub const AUTH_STYLE_API_KEY: &str = "apiKey";
/// See [`AUTH_STYLE_API_KEY`].
pub const AUTH_STYLE_BEARER: &str = "bearer";

/// The default auth style for this protocol shape.
///
/// The official Anthropic API mainly uses `x-api-key` (only OAuth tokens go through Bearer); the OpenAI-compatible ecosystem always uses Bearer.
fn default_auth_style(kind: &str) -> &'static str {
    match kind {
        "anthropic" => AUTH_STYLE_API_KEY,
        _ => AUTH_STYLE_BEARER,
    }
}

/// Normalize the auth style: empty / invalid values always fall back to the default for this protocol shape.
pub fn normalize_auth_style(kind: &str, style: &str) -> String {
    let trimmed = style.trim();
    if trimmed.eq_ignore_ascii_case(AUTH_STYLE_BEARER) {
        return AUTH_STYLE_BEARER.to_string();
    }
    if trimmed.eq_ignore_ascii_case(AUTH_STYLE_API_KEY) {
        return AUTH_STYLE_API_KEY.to_string();
    }
    default_auth_style(kind).to_string()
}

/// The credential body on disk (contains the plaintext key, **must never be serialized to the frontend**).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCredential {
    /// Credential id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub label: String,
    /// Protocol shape: `openai` / `anthropic`.
    pub kind: String,
    /// Base URL。
    pub base_url: String,
    /// API key (plaintext, stored only in the local file).
    pub api_key: String,
    /// Auth header style: `apiKey` / `bearer`; empty string = use the default for `kind`
    /// (older credential files lack this field; `normalize_auth_style` covers it, so no migration is needed).
    #[serde(default)]
    pub auth_style: String,
    /// The default model for this credential (can be overridden per session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Extra custom request headers (vendor/gateway differences, e.g. OpenRouter's `HTTP-Referer`).
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    /// Models the user has starred (a subset picked from "fetch model list", for quick switching).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Last updated time (Unix milliseconds).
    #[serde(default)]
    pub updated_at: i64,
}

/// Redacted view exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialView {
    /// Credential id.
    pub id: String,
    /// Display name.
    pub label: String,
    /// Protocol shape.
    pub kind: String,
    /// Base URL。
    pub base_url: String,
    /// Default model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Auth header style (already normalized to `apiKey` / `bearer`, echoed directly by the frontend).
    pub auth_style: String,
    /// Models the user has starred.
    #[serde(default)]
    pub models: Vec<String>,
    /// Extra custom request headers (echoed back when editing).
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    /// Whether a key is configured.
    pub has_key: bool,
    /// Key hint (e.g. `sk-…1a2b`).
    pub key_hint: String,
    /// Last updated time.
    pub updated_at: i64,
}

/// Input for creating / updating a credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    /// Credential id (`None` = create new).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Display name.
    #[serde(default)]
    pub label: String,
    /// Protocol shape: `openai` / `anthropic`.
    pub kind: String,
    /// Base URL (empty = use the default address for this protocol).
    #[serde(default)]
    pub base_url: String,
    /// API key; `None` / empty string = keep the existing value (on update).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Default model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Auth header style; `None` = keep the existing value (on update) / use the protocol default (on create).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_style: Option<String>,
    /// Starred model list; `None` = keep the existing value, `Some([])` = clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    /// Custom request headers; `None` = keep the existing value, `Some([])` = clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<HeaderPair>>,
}

/// Credential file (top-level container, easy to extend later with other AI-related secrets).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CredentialFile {
    credentials: Vec<AiCredential>,
}

/// Credential store.
#[derive(Debug, Clone, Default)]
pub struct CredentialStore {
    credentials: Vec<AiCredential>,
}

/// Validate that the protocol shape is supported.
pub fn validate_kind(kind: &str) -> AiResult<()> {
    match kind {
        "openai" | "anthropic" => Ok(()),
        other => Err(AiError::Invalid(format!(
            "unsupported provider type `{other}` (expected openai / anthropic)"
        ))),
    }
}

/// Normalize the model list: trim whitespace, dedupe, preserve order.
pub fn normalize_models(models: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in models {
        let m = m.trim().to_string();
        if !m.is_empty() && !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// Build a redacted hint: keep the prefix and the last 4 characters, elide the middle.
pub fn key_hint(api_key: &str) -> String {
    let chars: Vec<char> = api_key.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    if chars.len() <= 8 {
        return "•".repeat(chars.len());
    }
    let head: String = chars.iter().take(3).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}…{tail}")
}

/// Default credential file path: `<app_data_dir>/ai/credentials.yaml`.
pub fn default_credentials_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("ai").join("credentials.yaml")
}

impl CredentialStore {
    /// Load from file; a missing file counts as an empty store.
    pub fn load(path: &Path) -> AiResult<Self> {
        match read_optional(path)? {
            None => Ok(Self::default()),
            Some(text) => {
                let file: CredentialFile = serde_yaml::from_str(&text).map_err(|e| {
                    AiError::Invalid(format!(
                        "failed to parse credential file ({}): {e}. Delete the file and configure again.",
                        path.display()
                    ))
                })?;
                Ok(Self {
                    credentials: file.credentials,
                })
            }
        }
    }

    /// Save to file (atomic write).
    pub fn save(&self, path: &Path) -> AiResult<()> {
        if let Some(parent) = path.parent() {
            ensure_dir(parent)?;
        }
        let file = CredentialFile {
            credentials: self.credentials.clone(),
        };
        let text = serde_yaml::to_string(&file)?;
        write_atomic(path, text.as_bytes())
    }

    /// All credentials (redacted).
    pub fn views(&self) -> Vec<CredentialView> {
        self.credentials
            .iter()
            .map(|c| CredentialView {
                id: c.id.clone(),
                // Pass through as-is: **the display name is decided by the frontend** (an empty name is inferred from the Base URL, see
                // `web/src/lib/ai/credentials.ts`). If we fell back to the id here, the frontend could no longer
                // tell "no name set" apart from "named cred-xxxx" and could only show a gibberish-looking id.
                label: c.label.clone(),
                kind: c.kind.clone(),
                base_url: c.base_url.clone(),
                default_model: c.default_model.clone(),
                auth_style: normalize_auth_style(&c.kind, &c.auth_style),
                models: c.models.clone(),
                headers: c.headers.clone(),
                has_key: !c.api_key.is_empty(),
                key_hint: key_hint(&c.api_key),
                updated_at: c.updated_at,
            })
            .collect()
    }

    /// Get a credential by id (**contains the plaintext key**, only for issuing requests internally).
    pub fn get(&self, id: &str) -> Option<&AiCredential> {
        self.credentials.iter().find(|c| c.id == id)
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.credentials.is_empty()
    }

    /// Create or update, returning the credential id.
    pub fn upsert(&mut self, input: CredentialInput) -> AiResult<String> {
        validate_kind(&input.kind)?;
        let key = input.api_key.as_deref().unwrap_or("").trim().to_string();
        let base_url = input.base_url.trim().to_string();
        // Explicitly requested auth style (None = reuse the existing value on update / use the protocol default on create)
        let requested_style = input.auth_style.clone();
        let id = match input.id.clone() {
            Some(id) if !id.trim().is_empty() => id,
            _ => format!("cred-{}", uuid::Uuid::new_v4()),
        };

        match self.credentials.iter_mut().find(|c| c.id == id) {
            Some(existing) => {
                if !key.is_empty() {
                    existing.api_key = key;
                }
                existing.label = input.label.trim().to_string();
                // Auth style: if not passed explicitly, reuse the existing value (which also fills empty values from old files with an explicit one);
                // Take the value out before assigning to avoid a simultaneous mutable/immutable borrow of the same field
                let new_kind = input.kind;
                let style_source = requested_style
                    .clone()
                    .unwrap_or_else(|| existing.auth_style.clone());
                existing.kind = new_kind.clone();
                existing.base_url = base_url;
                existing.auth_style = normalize_auth_style(&new_kind, &style_source);
                existing.default_model = input.default_model.filter(|m| !m.trim().is_empty());
                if let Some(models) = input.models {
                    existing.models = normalize_models(models);
                }
                if let Some(headers) = input.headers {
                    existing.headers = normalize_headers(headers);
                }
                existing.updated_at = jiff::Timestamp::now().as_millisecond();
            }
            None => {
                if key.is_empty() {
                    return Err(AiError::MissingCredential(
                        "creating a credential requires an API key".into(),
                    ));
                }
                let auth_style =
                    normalize_auth_style(&input.kind, requested_style.as_deref().unwrap_or(""));
                self.credentials.push(AiCredential {
                    id: id.clone(),
                    label: input.label.trim().to_string(),
                    kind: input.kind,
                    base_url,
                    api_key: key,
                    auth_style,
                    default_model: input.default_model.filter(|m| !m.trim().is_empty()),
                    models: normalize_models(input.models.unwrap_or_default()),
                    headers: normalize_headers(input.headers.unwrap_or_default()),
                    updated_at: jiff::Timestamp::now().as_millisecond(),
                });
            }
        }
        Ok(id)
    }

    /// Remove; returns whether a row was actually deleted.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.credentials.len();
        self.credentials.retain(|c| c.id != id);
        before != self.credentials.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("orbit-ai-auth-{}", uuid::Uuid::new_v4()))
            .join("ai")
            .join("credentials.yaml")
    }

    fn input(id: Option<&str>, key: Option<&str>) -> CredentialInput {
        CredentialInput {
            id: id.map(str::to_string),
            label: "DeepSeek".into(),
            kind: "openai".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: key.map(str::to_string),
            default_model: Some("deepseek-v4-pro".into()),
            auth_style: None,
            models: None,
            headers: None,
        }
    }

    #[test]
    fn custom_headers_round_trip_and_are_normalized() {
        let mut store = CredentialStore::default();
        let mut first = input(None, Some("sk-1"));
        first.headers = Some(vec![
            HeaderPair {
                key: " HTTP-Referer ".into(),
                value: " https://orbit.dev ".into(),
            },
            // Empty keys and duplicate names (differing in case) must both be normalized away
            HeaderPair {
                key: "   ".into(),
                value: "x".into(),
            },
            HeaderPair {
                key: "x-title".into(),
                value: "old".into(),
            },
            HeaderPair {
                key: "X-Title".into(),
                value: "Orbit".into(),
            },
        ]);
        let id = store.upsert(first).unwrap();
        let c = store.get(&id).unwrap();
        assert_eq!(c.headers.len(), 2);
        assert_eq!(c.headers[0].key, "HTTP-Referer");
        assert_eq!(c.headers[0].value, "https://orbit.dev");
        assert_eq!(c.headers[1].key, "x-title");
        assert_eq!(
            c.headers[1].value, "Orbit",
            "duplicate headers keep the last one"
        );

        // Not passing headers -> keep the existing value
        store.upsert(input(Some(&id), None)).unwrap();
        assert_eq!(store.get(&id).unwrap().headers.len(), 2);

        // Explicitly empty array -> clear
        let mut clear = input(Some(&id), None);
        clear.headers = Some(vec![]);
        store.upsert(clear).unwrap();
        assert!(store.get(&id).unwrap().headers.is_empty());
    }

    #[test]
    fn credential_view_exposes_headers_for_editing() {
        let mut store = CredentialStore::default();
        let mut first = input(None, Some("sk-1"));
        first.headers = Some(vec![HeaderPair {
            key: "X-Tenant".into(),
            value: "acme".into(),
        }]);
        store.upsert(first).unwrap();
        let view = &store.views()[0];
        assert_eq!(view.headers.len(), 1);
        assert_eq!(view.headers[0].key, "X-Tenant");
    }

    #[test]
    fn save_load_roundtrip_keeps_plaintext_for_runtime_use() {
        let path = temp_path();
        let mut store = CredentialStore::default();
        let id = store.upsert(input(None, Some("sk-abcdef123456"))).unwrap();
        store.save(&path).unwrap();

        let loaded = CredentialStore::load(&path).unwrap();
        assert_eq!(loaded.get(&id).unwrap().api_key, "sk-abcdef123456");
        assert_eq!(
            loaded.get(&id).unwrap().base_url,
            "https://api.deepseek.com/v1"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn views_never_expose_full_key() {
        let mut store = CredentialStore::default();
        store.upsert(input(None, Some("sk-abcdef123456"))).unwrap();
        let views = store.views();
        assert_eq!(views.len(), 1);
        assert!(views[0].has_key);
        assert_eq!(views[0].key_hint, "sk-…3456");
        assert!(!views[0].key_hint.contains("abcdef"));
    }

    #[test]
    fn updating_without_key_keeps_existing_secret() {
        let mut store = CredentialStore::default();
        let id = store.upsert(input(None, Some("sk-original-1234"))).unwrap();
        let mut patch = input(Some(&id), None);
        patch.label = "renamed".into();
        store.upsert(patch).unwrap();
        let c = store.get(&id).unwrap();
        assert_eq!(c.api_key, "sk-original-1234");
        assert_eq!(c.label, "renamed");
    }

    #[test]
    fn creating_without_key_is_rejected() {
        let mut store = CredentialStore::default();
        assert!(matches!(
            store.upsert(input(None, None)),
            Err(AiError::MissingCredential(_))
        ));
        assert!(matches!(
            store.upsert(input(None, Some("   "))),
            Err(AiError::MissingCredential(_))
        ));
    }

    #[test]
    fn unsupported_kind_is_rejected() {
        let mut store = CredentialStore::default();
        let mut bad = input(None, Some("k"));
        bad.kind = "gemini".into();
        assert!(store.upsert(bad).is_err());
    }

    #[test]
    fn remove_reports_whether_row_existed() {
        let mut store = CredentialStore::default();
        let id = store.upsert(input(None, Some("k-123456789"))).unwrap();
        assert!(store.remove(&id));
        assert!(!store.remove(&id));
        assert!(store.is_empty());
    }

    #[test]
    fn missing_file_yields_empty_store() {
        let path = temp_path();
        let store = CredentialStore::load(&path).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn corrupted_file_reports_actionable_error() {
        let path = temp_path();
        write_atomic(&path, b": not yaml [").unwrap();
        let err = CredentialStore::load(&path).unwrap_err();
        match err {
            AiError::Invalid(msg) => assert!(msg.contains("failed to parse credential file")),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn key_hint_masks_short_keys_completely() {
        assert_eq!(key_hint("abc"), "•••");
        assert_eq!(key_hint(""), "");
        assert_eq!(key_hint("sk-12345678"), "sk-…5678");
    }

    #[test]
    fn selected_models_are_persisted_and_optional_on_update() {
        let path = temp_path();
        let mut store = CredentialStore::default();
        let mut first = input(None, Some("sk-abcdef123456"));
        first.models = Some(vec![
            " deepseek-chat ".into(),
            "deepseek-reasoner".into(),
            "".into(),
        ]);
        let id = store.upsert(first).unwrap();
        store.save(&path).unwrap();

        // Trim whitespace + drop empty strings
        let loaded = CredentialStore::load(&path).unwrap();
        assert_eq!(
            loaded.get(&id).unwrap().models,
            vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()]
        );
        assert_eq!(loaded.views()[0].models.len(), 2);

        // Not passing models on update -> keep the existing value
        let mut patch = input(Some(&id), None);
        patch.label = "renamed".into();
        store.upsert(patch).unwrap();
        assert_eq!(store.get(&id).unwrap().models.len(), 2);

        // Explicitly passing an empty array -> clear
        let mut clear = input(Some(&id), None);
        clear.models = Some(vec![]);
        store.upsert(clear).unwrap();
        assert!(store.get(&id).unwrap().models.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn auth_style_is_normalized_persisted_and_legacy_friendly() {
        let path = temp_path();
        let mut store = CredentialStore::default();

        // New and not explicitly specified -> use the protocol default (openai -> bearer)
        let id = store.upsert(input(None, Some("sk-abcdef123456"))).unwrap();
        assert_eq!(store.get(&id).unwrap().auth_style, AUTH_STYLE_BEARER);
        assert_eq!(store.views()[0].auth_style, AUTH_STYLE_BEARER);

        // Explicitly switch to apiKey (Azure-style `api-key` header)
        let mut patch = input(Some(&id), None);
        patch.auth_style = Some(AUTH_STYLE_API_KEY.into());
        store.upsert(patch).unwrap();
        assert_eq!(store.get(&id).unwrap().auth_style, AUTH_STYLE_API_KEY);

        // Not passing -> keep the existing value
        let mut patch = input(Some(&id), None);
        patch.label = "renamed".into();
        store.upsert(patch).unwrap();
        assert_eq!(store.get(&id).unwrap().auth_style, AUTH_STYLE_API_KEY);

        // Invalid value -> fall back to the protocol default
        let mut patch = input(Some(&id), None);
        patch.auth_style = Some("whatever".into());
        store.upsert(patch).unwrap();
        assert_eq!(store.get(&id).unwrap().auth_style, AUTH_STYLE_BEARER);

        // Save/load round trip
        store.save(&path).unwrap();
        let loaded = CredentialStore::load(&path).unwrap();
        assert_eq!(loaded.views()[0].auth_style, AUTH_STYLE_BEARER);

        // Old credential files have no authStyle field: fall back to the protocol default on read (Anthropic -> x-api-key), no migration needed
        write_atomic(
            &path,
            b"credentials:\n- id: cred-old\n  label: old\n  kind: anthropic\n  baseUrl: https://api.anthropic.com/v1\n  apiKey: sk-ant-api\n",
        )
        .unwrap();
        let loaded = CredentialStore::load(&path).unwrap();
        assert_eq!(loaded.views()[0].auth_style, AUTH_STYLE_API_KEY);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn normalize_models_dedupes_and_keeps_order() {
        assert_eq!(
            normalize_models(vec!["b".into(), "a".into(), "b".into(), " ".into()]),
            vec!["b".to_string(), "a".to_string()]
        );
    }

    #[test]
    fn default_path_is_under_ai_dir() {
        let p = default_credentials_path(Path::new("C:/data"));
        assert!(p.ends_with("ai/credentials.yaml") || p.ends_with("ai\\credentials.yaml"));
    }
}
