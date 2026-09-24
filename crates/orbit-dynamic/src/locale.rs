//! Language locale support
//!
//! - Expression-level: `{{$person.fullName(locale=ja)}}` (priority)
//! - Global default: `orbit_dynamic::set_default_locale(Locale::En)` (fallback)
//! - Default `zh` (preserves existing behavior)
//!
//! Adding a language: add a variant to `Locale` + branches in `parse`/`as_str`, then create
//! a `data/<lang>.rs` dataset and register it in [`crate::data::dataset`]. In the generators,
//! the composition logic (address format, name order, etc.) is an exhaustive `match`, and the compiler will point out every spot to fill in.

use std::sync::atomic::{AtomicU8, Ordering};

/// Language locale
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    /// Chinese
    #[default]
    Zh,
    /// English
    En,
    /// Japanese
    Ja,
}

impl Locale {
    /// Parse from a string (case-insensitive): `zh`/`zh-cn` -> Zh; `en`/`en-us` -> En; `ja`/`ja-jp`/`jp` -> Ja
    pub fn parse(s: &str) -> Option<Locale> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zh" | "zh-cn" | "zh_cn" | "cn" | "chinese" => Some(Locale::Zh),
            "en" | "en-us" | "en_us" | "us" | "english" => Some(Locale::En),
            "ja" | "ja-jp" | "ja_jp" | "jp" | "japanese" => Some(Locale::Ja),
            _ => None,
        }
    }

    /// Canonical name
    pub fn as_str(self) -> &'static str {
        match self {
            Locale::Zh => "zh",
            Locale::En => "en",
            Locale::Ja => "ja",
        }
    }
}

static DEFAULT_LOCALE: AtomicU8 = AtomicU8::new(0);

/// Set the global default locale (process-wide; fallback for expressions without a `locale` argument)
pub fn set_default_locale(locale: Locale) {
    DEFAULT_LOCALE.store(locale as u8, Ordering::Relaxed);
}

/// Current global default locale (defaults to `zh`)
pub fn default_locale() -> Locale {
    match DEFAULT_LOCALE.load(Ordering::Relaxed) {
        1 => Locale::En,
        2 => Locale::Ja,
        _ => Locale::Zh,
    }
}
