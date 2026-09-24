//! Java/Moment-style date format string -> jiff strftime converter.
//!
//! Users write format strings in Java SimpleDateFormat / Moment.js style (e.g. `yyyy-MM-dd HH:mm:ss`),
//! and this module converts them to jiff strftime directives before handing them to `Zoned::strftime`.
//!
//! Detection rules:
//! - If the format string contains `%` followed immediately by a known strftime directive letter -> treated as strftime passthrough (compatible with the old `format=%Y-%m-%d`).
//! - Otherwise convert as Java style; on the Java path a bare `%` is escaped to `%%` (to avoid misinterpretation by strftime).

use super::DynamicError;

/// Known jiff strftime directive letters (used for the "contains % means strftime" check).
const STRTIME_DIRECTIVES: &[char] = &[
    'Y', 'y', 'm', 'd', 'H', 'I', 'M', 'S', 'f', 'N', 'z', 'Z', 'a', 'A', 'u', 'w', 'b', 'B', 'p',
    'P', 'T', 'R', 'F', 'j', 'U', 'W', 'G', 'g', 'V', 'e', 'Q', 'q', 'C', 'k', 'l', 'n', 't', 'D',
    's', 'x', 'X', 'c', 'r', '%',
];

/// Java-style tokens (in descending length order, greedy matching; longer tokens must precede their shorter prefixes).
const JAVA_TOKENS: &[(&str, &str)] = &[
    ("yyyy", "%Y"),
    ("YYYY", "%Y"),
    ("EEEE", "%A"),
    ("MMMM", "%B"),
    ("MMM", "%b"),
    ("EEE", "%a"),
    ("SSS", "%3f"),
    ("XXX", "%:z"),
    ("XX", "%z"),
    ("yy", "%y"),
    ("YY", "%y"),
    ("HH", "%H"),
    ("hh", "%I"),
    ("mm", "%M"),
    ("ss", "%S"),
    ("dd", "%d"),
    ("MM", "%m"),
    ("X", "%z"),
    ("Z", "%z"),
    ("E", "%a"),
    ("H", "%-H"),
    ("h", "%-I"),
    ("M", "%-m"),
    ("d", "%-d"),
    ("s", "%-S"),
    ("m", "%-M"),
    ("a", "%p"),
    ("u", "%u"),
    ("w", "%w"),
];

fn is_strftime(fmt: &str) -> bool {
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            // A trailing `%` is not a directive
            if i + 1 < chars.len() && STRTIME_DIRECTIVES.contains(&chars[i + 1]) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Detect whether a format string is strftime syntax (for external tests/tools).
#[cfg(test)]
pub fn looks_like_strftime(fmt: &str) -> bool {
    is_strftime(fmt)
}

/// Convert a Java-style format string to a jiff strftime directive string.
///
/// Rules:
/// - Content inside quotes `'...'` is passed through literally (`''` means a literal single quote).
/// - Known token mappings (see [`JAVA_TOKENS`]).
/// - A bare `%` is escaped to `%%`; unknown tokens are passed through literally (lenient policy).
pub fn java_to_strftime(fmt: &str) -> String {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut i = 0;
    let n = chars.len();
    while i < n {
        // Quoted literal
        if chars[i] == '\'' {
            i += 1;
            while i < n {
                if chars[i] == '\'' {
                    if i + 1 < n && chars[i + 1] == '\'' {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        // Known token (longest match)
        let rest: String = chars[i..].iter().collect();
        let mut matched: Option<&'static str> = None;
        let mut matched_len = 0usize;
        for (token, jiff_fmt) in JAVA_TOKENS {
            if rest.starts_with(token) && token.len() > matched_len {
                matched = Some(jiff_fmt);
                matched_len = token.len();
            }
        }
        if let Some(jiff_fmt) = matched {
            out.push_str(jiff_fmt);
            i += matched_len;
            continue;
        }
        // Bare % escape
        if chars[i] == '%' {
            out.push_str("%%");
            i += 1;
            continue;
        }
        // Everything else passed through literally
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Format a `Zoned` with the given format string.
///
/// Auto-detect: strftime passthrough vs Java conversion. A conversion failure (should not happen in theory) returns `InvalidArgs`.
pub fn format_datetime(dt: &jiff::Zoned, fmt: &str) -> Result<String, DynamicError> {
    if is_strftime(fmt) {
        Ok(dt.strftime(fmt).to_string())
    } else {
        let jiff_fmt = java_to_strftime(fmt);
        Ok(dt.strftime(&jiff_fmt).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_zoned() -> jiff::Zoned {
        // 2024-07-15 17:30:45 Monday (Asia/Shanghai)
        let ts: jiff::Timestamp = "2024-07-15T09:30:45Z".parse().unwrap();
        ts.in_tz("Asia/Shanghai").unwrap()
    }

    #[test]
    fn java_basic() {
        assert_eq!(java_to_strftime("yyyy-MM-dd HH:mm:ss"), "%Y-%m-%d %H:%M:%S");
        assert_eq!(java_to_strftime("yyyy-MM-dd"), "%Y-%m-%d");
        assert_eq!(java_to_strftime("HH:mm"), "%H:%M");
    }

    #[test]
    fn strftime_passthrough() {
        assert!(looks_like_strftime("%Y-%m-%d"));
        assert!(!looks_like_strftime("yyyy-MM-dd"));
        assert!(!looks_like_strftime("yyyy年MM月dd日"));
    }

    #[test]
    fn quoted_literals() {
        // 'T' literal; '' = empty literal (Java semantics: opening quote + closing quote)
        assert_eq!(
            java_to_strftime("yyyy-MM-dd'T'HH:mm:ss"),
            "%Y-%m-%dT%H:%M:%S"
        );
        assert_eq!(java_to_strftime("yyyy''MM"), "%Y%m");
        // '' inside quotes means a literal single quote
        assert_eq!(java_to_strftime("'can''t'"), "can't");
    }

    #[test]
    fn tokens() {
        assert_eq!(java_to_strftime("a hh u w"), "%p %I %u %w");
        assert_eq!(java_to_strftime("MMMM MMM EEEE EEE"), "%B %b %A %a");
        assert_eq!(java_to_strftime("SSS"), "%3f");
        assert_eq!(java_to_strftime("XXX"), "%:z");
        assert_eq!(java_to_strftime("YYYY"), "%Y");
    }

    #[test]
    fn java_with_literal_percent() {
        // A bare % still takes the Java path (not misdetected as strftime), % is escaped to %%
        assert_eq!(java_to_strftime("yyyy年MM月dd日 %"), "%Y年%m月%d日 %%");
    }

    #[test]
    fn unknown_token_passthrough() {
        assert_eq!(java_to_strftime("yyyy/Q"), "%Y/Q");
    }

    #[test]
    fn format_now() {
        let zdt = fixed_zoned();
        assert_eq!(
            format_datetime(&zdt, "yyyy-MM-dd HH:mm:ss").unwrap(),
            "2024-07-15 17:30:45"
        );
        assert_eq!(format_datetime(&zdt, "yyyy-MM-dd").unwrap(), "2024-07-15");
        // 12-hour clock + AM/PM
        assert_eq!(format_datetime(&zdt, "hh:mm a").unwrap(), "05:30 PM");
        // Weekday number (Monday=1)
        assert_eq!(format_datetime(&zdt, "u").unwrap(), "1");
        // Month name
        assert_eq!(format_datetime(&zdt, "MMMM").unwrap(), "July");
        assert_eq!(format_datetime(&zdt, "MMM").unwrap(), "Jul");
        // Weekday name
        assert_eq!(format_datetime(&zdt, "EEEE").unwrap(), "Monday");
        assert_eq!(format_datetime(&zdt, "EEE").unwrap(), "Mon");
        // strftime passthrough (old format)
        assert_eq!(format_datetime(&zdt, "%Y-%m-%d").unwrap(), "2024-07-15");
        // Time zone offset
        assert_eq!(format_datetime(&zdt, "XXX").unwrap(), "+08:00");
    }
}
