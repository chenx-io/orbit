//! Interpolation placeholder regexes (statically compiled to avoid recompiling on hot paths)

use std::sync::LazyLock;

/// Arithmetic expression placeholder `${=expr}` (statically compiled regex, avoids recompiling on hot paths)
pub(crate) static ARITH_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\$\{=([^}]+)\}").unwrap());

/// Plain variable placeholder `${var}` / `${env:var}`
pub(crate) static VAR_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\$\{([^}]+)\}").unwrap());

/// Application-level double-brace placeholder `{{var}}` (variable syntax of the frontend interface manager).
/// Note that `{{$...}}` denotes a dynamic value (handled by orbit_dynamic); after matching, dispatch by
/// whether the name starts with `$`.
pub(crate) static DOUBLE_VAR_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\{\{([^{}]+)\}\}").unwrap());
