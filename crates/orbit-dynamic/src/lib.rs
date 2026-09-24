//! # orbit-dynamic
//!
//! Dynamic value engine - supports Apifox-style `{{$category.method}}` expressions.
//!
//! ## Syntax
//!
//! ```text
//! {{$category.method}}
//! {{$category.method(args)}}
//! {{$category.method|pipe1|pipe2}}
//! ```
//!
//! ## Multi-language (locale) support
//!
//! Names, geographic locations, products, companies, foods, vehicles, music, colors, lorem, date names, etc.
//! data supports multiple languages (currently zh / en / ja), specified in two ways (expression-level takes priority):
//!
//! - Expression-level: `{{$person.fullName(locale=ja)}}` -> `佐藤健太`
//! - Global default: `orbit_dynamic::set_default_locale(Locale::En)` (process-wide)
//! - Default `zh` (preserves existing behavior)
//!
//! **Adding a language**: add a variant to the `Locale` enum -> create a `data/<lang>.rs` dataset ->
//! add a branch in `data::dataset`; generators need no changes, exhaustive `match` in the composition logic
//! will let the compiler point out every spot to fill in.
//!
//! ## Supported categories
//!
//! - `string`: UUID, random string, alphanumeric
//! - `number`: random integer, float
//! - `date`: current time, timestamp, offset time, random range, weekday/month (Java-style format string; under zh outputs Chinese weekday/month names)
//! - `person`: name (zh/en), email
//! - `internet`: email, URL, IP
//! - `phone`: mobile number (zh Chinese number / en US-style number)
//! - `location`: city, address (zh/en)
//! - `commerce`: price, product name (zh/en)
//! - `company`: company name (zh/en)
//! - `finance`: bank card number (currency / transaction type zh/en)
//! - `helpers`: enum, regex, increment
//! - `datatype`: boolean
//! - `image`: random image URL
//! - `lorem`: random text (zh/en)
//! - `color`: color (zh/en)
//! - `food`: food (zh/en)
//! - `vehicle`: vehicle (zh/en)
//! - `music`: music (zh/en)

mod data;
mod format;
mod generators;
mod locale;

pub use locale::{default_locale, set_default_locale, Locale};

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Dynamic value parse error
#[derive(Debug, thiserror::Error)]
pub enum DynamicError {
    #[error("Unknown category: {0}")]
    UnknownCategory(String),
    #[error("Unknown method: {category}.{method}")]
    UnknownMethod { category: String, method: String },
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
}

/// Parse and replace all dynamic value expressions in a string
///
/// ```rust
/// use orbit_dynamic::resolve;
/// let result = resolve("Hello {{$person.fullName}}, your ID is {{$string.uuid}}");
/// assert!(result.is_ok());
/// ```
static DYNAMIC_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Method names may contain **digits** (first char still a letter): methods like `internet.ipv6`
    // used to never match because of `[a-zA-Z]+` - the generator existed and docs mentioned it, but the expression was **never parsed**,
    // so it was sent out verbatim. Caught by `orbit-ai`'s syntax anti-drift test (`syntax::tests`).
    Regex::new(r"\{\{\$([a-z]+)\.([a-zA-Z][a-zA-Z0-9]*)(?:\(([^)]*)\))?(?:\|([^}]+))?\}\}")
        .expect("valid dynamic value regex")
});

pub fn resolve(input: &str) -> Result<String, DynamicError> {
    let mut result = input.to_string();

    for cap in DYNAMIC_PATTERN.captures_iter(input) {
        let full_match = &cap[0];
        let category = &cap[1];
        let method = &cap[2];
        let args_str = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let pipes_str = cap.get(4).map(|m| m.as_str());

        let value = generate(category, method, args_str)?;

        // Apply pipes
        let final_value = if let Some(pipes) = pipes_str {
            apply_pipes(&value, pipes)?
        } else {
            value
        };

        result = result.replace(full_match, &final_value);
    }

    Ok(result)
}

/// Generate a single dynamic value
pub fn generate(category: &str, method: &str, args: &str) -> Result<String, DynamicError> {
    let args_map = parse_args(args);
    generators::generate(category, method, &args_map)
}

/// Parse arguments "key=value, key2=value2" -> HashMap
/// If there is no `=` sign in the arguments, the whole string becomes the value of key "0"
fn parse_args(args: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if args.is_empty() {
        return map;
    }
    let parts = split_args(args);
    // Check if any part contains '='
    let has_key_value = parts.iter().any(|p| p.contains('='));
    if !has_key_value {
        map.insert("0".to_string(), args.to_string());
        return map;
    }
    for part in parts {
        let part = part.trim();
        if let Some(pos) = part.find('=') {
            let key = part[..pos].trim().to_string();
            let value = part[pos + 1..]
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_string();
            map.insert(key, value);
        }
    }
    map
}

/// Quote-aware argument splitting: split on `,` only outside single/double quotes (no escape handling).
/// Keeps `format='yyyy,MM,dd'` from being split incorrectly.
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in args.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                    cur.push(c);
                } else if c == ',' {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.push(c);
                }
            }
        }
    }
    out.push(cur);
    out
}

/// Apply pipe functions
fn apply_pipes(value: &str, pipes: &str) -> Result<String, DynamicError> {
    let mut result = value.to_string();
    for pipe in pipes.split('|') {
        let pipe = pipe.trim();
        let (func, args_str) = if let Some(pos) = pipe.find('(') {
            let func = &pipe[..pos];
            let args = pipe[pos + 1..].trim_end_matches(')');
            (func, args)
        } else {
            (pipe, "")
        };
        let pipe_args = parse_args(args_str);
        result = generators::apply_pipe(&result, func, &pipe_args)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: method names may contain digits (`internet.ipv6` used to be "generator exists but the expression never matches").
    #[test]
    fn test_resolve_method_with_digits() {
        let result = resolve("{{$internet.ipv6}}").unwrap();
        assert!(
            !result.contains("{{$"),
            "A method name with digits must be parsed, actual: {result}"
        );
        assert!(
            result.contains(':'),
            "IPv6 text should contain a colon: {result}"
        );
    }

    #[test]
    fn test_resolve_uuid() {
        let result = resolve("{{$string.uuid}}").unwrap();
        assert_eq!(result.len(), 36); // UUID v4 length
        assert!(result.contains('-'));
    }

    #[test]
    fn test_resolve_multiple() {
        let result = resolve("Name: {{$person.fullName}}, Email: {{$internet.email}}").unwrap();
        assert!(result.contains("Name: "));
        assert!(result.contains("Email: "));
        assert!(result.contains('@'));
    }

    #[test]
    fn test_resolve_with_args() {
        let result = resolve("{{$number.int(min=1,max=100)}}").unwrap();
        let n: i64 = result.parse().unwrap();
        assert!((1..=100).contains(&n));
    }

    #[test]
    fn test_resolve_date() {
        let result = resolve("{{$date.now}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected now: {result}"
        );
    }

    #[test]
    fn test_resolve_date_strftime_backcompat() {
        // The old strftime format still works (passthrough when it contains %)
        let result = resolve("{{$date.now(format=%Y-%m-%d)}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected: {result}"
        );
    }

    #[test]
    fn test_resolve_date_java_format() {
        let result = resolve("{{$date.now(format=yyyy-MM-dd)}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected: {result}"
        );
    }

    #[test]
    fn test_resolve_date_time() {
        let result = resolve("{{$date.time}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{2}:\d{2}:\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected: {result}"
        );
    }

    #[test]
    fn test_resolve_date_today() {
        let result = resolve("{{$date.today}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected: {result}"
        );
    }

    #[test]
    fn test_resolve_date_offset() {
        // Yesterday
        let result = resolve("{{$date.offset(unit=days, amount=-1, format=%Y-%m-%d)}}").unwrap();
        let now = jiff::Zoned::now();
        let yesterday = now.checked_sub(jiff::Span::new().days(1)).unwrap();
        assert_eq!(result, yesterday.strftime("%Y-%m-%d").to_string());

        // Month offset (checked_add months)
        let result = resolve("{{$date.offset(unit=months, amount=-1, format=%Y-%m)}}").unwrap();
        let last_month = now.checked_sub(jiff::Span::new().months(1)).unwrap();
        assert_eq!(result, last_month.strftime("%Y-%m").to_string());
    }

    #[test]
    fn test_resolve_date_past_future_legacy() {
        // Backward-compatible with the old days parameter
        let past = resolve("{{$date.past(days=7, format=%Y-%m-%d)}}").unwrap();
        let now = jiff::Zoned::now();
        let expected = now.checked_sub(jiff::Span::new().days(7)).unwrap();
        assert_eq!(past, expected.strftime("%Y-%m-%d").to_string());

        let future = resolve("{{$date.future(days=7, format=%Y-%m-%d)}}").unwrap();
        let expected = now.checked_add(jiff::Span::new().days(7)).unwrap();
        assert_eq!(future, expected.strftime("%Y-%m-%d").to_string());
    }

    #[test]
    fn test_resolve_date_between_fixed() {
        let result =
            resolve("{{$date.between(start=2024-01-01, end=2024-12-31, format=%Y-%m-%d)}}")
                .unwrap();
        assert!(
            result.starts_with("2024-"),
            "between should stay in 2024: {result}"
        );
    }

    #[test]
    fn test_resolve_date_between_relative() {
        let result = resolve("{{$date.between(start=-30d, end=+30d, format=%Y-%m-%d)}}").unwrap();
        let now = jiff::Zoned::now();
        let lo = now.checked_sub(jiff::Span::new().days(30)).unwrap();
        let hi = now.checked_add(jiff::Span::new().days(30)).unwrap();
        let parsed = jiff::civil::Date::strptime("%Y-%m-%d", &result)
            .unwrap()
            .at(0, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::system())
            .ok()
            .unwrap();
        assert!(
            parsed >= lo && parsed <= hi,
            "between result out of range: {result}"
        );
    }

    #[test]
    fn test_resolve_date_random() {
        // Default range now±50y
        let result = resolve("{{$date.random(format=%Y-%m-%d)}}").unwrap();
        let year: i32 = result[..4].parse().unwrap();
        let now_year = jiff::Zoned::now().year() as i32;
        assert!(
            (now_year - 50..=now_year + 50).contains(&year),
            "random year out of range: {result}"
        );
    }

    #[test]
    fn test_resolve_date_weekday_month() {
        let wd = resolve("{{$date.weekday(format=u)}}").unwrap();
        let n: i32 = wd.parse().unwrap();
        assert!((1..=7).contains(&n), "weekday number: {wd}");

        let mn = resolve("{{$date.monthName(format=MMMM)}}").unwrap();
        assert!(!mn.is_empty(), "monthName empty");

        let wd_name = resolve("{{$date.weekday}}").unwrap();
        assert!(!wd_name.is_empty());
    }

    #[test]
    fn test_resolve_date_quoted_comma_format() {
        // A format with a comma inside quotes is not broken apart by parse_args
        let result = resolve("{{$date.now(format='yyyy,MM,dd')}}").unwrap();
        assert!(
            regex::Regex::new(r"^\d{4},\d{2},\d{2}$")
                .unwrap()
                .is_match(&result),
            "unexpected: {result}"
        );
    }

    #[test]
    fn test_resolve_date_pipe() {
        let result = resolve("{{$date.weekday(format=EEEE, locale=en)|toUpperCase}}").unwrap();
        assert_eq!(result, result.to_uppercase());
        // Pipeline takes effect: output should be the uppercase weekday name (uppercase form of Mon/Tue...)
        let known = [
            "MONDAY",
            "TUESDAY",
            "WEDNESDAY",
            "THURSDAY",
            "FRIDAY",
            "SATURDAY",
            "SUNDAY",
        ];
        assert!(
            known.contains(&result.as_str()),
            "expected an uppercased weekday, got: {result}"
        );
    }

    #[test]
    fn test_resolve_date_tz_iana() {
        // IANA time zone: UTC differs from Shanghai by 8 hours (at the same instant)
        let utc = resolve("{{$date.now(format=%H, tz=UTC)}}").unwrap();
        let sh = resolve("{{$date.now(format=%H, tz=Asia/Shanghai)}}").unwrap();
        let utc_h: i32 = utc.parse().unwrap();
        let sh_h: i32 = sh.parse().unwrap();
        let diff = (sh_h - utc_h + 24) % 24;
        assert_eq!(diff, 8, "UTC vs Asia/Shanghai hour diff: {utc} vs {sh}");
    }

    #[test]
    fn test_resolve_date_tz_offset() {
        // Fixed offset +08:00 is equivalent to +8 and matches IANA Shanghai
        let a = resolve("{{$date.now(format=%H:%M, tz=+08:00)}}").unwrap();
        let b = resolve("{{$date.now(format=%H:%M, tz=+8)}}").unwrap();
        assert_eq!(a, b);
        let sh = resolve("{{$date.now(format=%H:%M, tz=Asia/Shanghai)}}").unwrap();
        assert_eq!(
            a, sh,
            "fixed +08:00 should equal Asia/Shanghai: {a} vs {sh}"
        );

        // Negative offset -05:00
        let _neg = resolve("{{$date.now(format=%Y-%m-%d, tz=-05:00)}}").unwrap();
        // UTC differs from +08:00 by 8 hours
        let utc = resolve("{{$date.now(format=%H, tz=UTC)}}").unwrap();
        let c = resolve("{{$date.now(format=%H, tz=+08:00)}}").unwrap();
        let utc_h: i32 = utc.parse().unwrap();
        let c_h: i32 = c.parse().unwrap();
        let diff = (c_h - utc_h + 24) % 24;
        assert_eq!(diff, 8, "UTC vs +08:00 hour diff: {utc} vs {c}");
    }

    #[test]
    fn test_resolve_date_tz_invalid_fallback() {
        // Invalid time zone falls back to local without error
        let result = resolve("{{$date.now(format=%Y-%m-%d, tz=Not/AZone)}}").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_split_args_quoted() {
        let parts = split_args("format='yyyy,MM,dd', unit=days");
        assert_eq!(parts.len(), 2);
        // Commas inside quotes are not split; compare after trim (parse_args trims internally)
        assert_eq!(parts[0].trim(), "format='yyyy,MM,dd'");
        assert_eq!(parts[1].trim(), "unit=days");
    }

    #[test]
    fn test_resolve_plain_text() {
        let result = resolve("Hello World").unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_resolve_phone() {
        let result = resolve("{{$phone.mobile(locale=zh)}}").unwrap();
        assert_eq!(result.len(), 11);
        assert!(result.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_resolve_boolean() {
        let result = resolve("{{$datatype.boolean}}").unwrap();
        assert!(result == "true" || result == "false");
    }

    #[test]
    fn test_resolve_enum() {
        let result = resolve("{{$helpers.arrayElement(['a','b','c'])}}").unwrap();
        assert!(result == "a" || result == "b" || result == "c");
    }

    #[test]
    fn test_resolve_idcard() {
        let result = resolve("{{$person.idCard}}").unwrap();
        assert_eq!(result.len(), 18);
    }

    // ─── locale support ──────────────────────────────────

    #[test]
    fn test_person_locale_zh_default() {
        // Default zh: Chinese name
        let name = resolve("{{$person.fullName(locale=zh)}}").unwrap();
        assert!(!name.is_empty());
        assert!(!name.is_ascii(), "zh name should contain CJK: {name}");
    }

    #[test]
    fn test_person_locale_en_arg() {
        let name = resolve("{{$person.fullName(locale=en)}}").unwrap();
        // English name = First Last (contains a space, all ASCII)
        assert!(name.split_whitespace().count() >= 2, "en name: {name}");
        assert!(
            name.chars().all(|c| c.is_ascii() || c == ' '),
            "en name ascii: {name}"
        );
    }

    #[test]
    fn test_person_locale_zh_arg() {
        let name = resolve("{{$person.fullName(locale=zh)}}").unwrap();
        assert!(!name.is_ascii(), "zh name: {name}");
    }

    #[test]
    fn test_person_locale_ja() {
        // Third language: Japanese name (surname+given, no space; surname 2-3 chars, given 1-3 chars, total length varies)
        let name = resolve("{{$person.fullName(locale=ja)}}").unwrap();
        let data = crate::data::dataset(Locale::Ja);
        let composed = data.person_last.iter().any(|last| {
            data.person_first
                .iter()
                .any(|first| name == format!("{last}{first}"))
        });
        assert!(
            composed,
            "ja name should be surname+given concatenated from dataset: {name}"
        );
        assert!(!name.contains(' '), "ja name should have no space: {name}");
        assert!(!name.is_ascii(), "ja name: {name}");
    }

    #[test]
    fn test_locale_parse_ja() {
        assert_eq!(Locale::parse("ja"), Some(Locale::Ja));
        assert_eq!(Locale::parse("ja-jp"), Some(Locale::Ja));
        assert_eq!(Locale::parse("JP"), Some(Locale::Ja));
    }

    #[test]
    fn test_phone_and_date_locale_ja() {
        let phone = resolve("{{$phone.mobile(locale=ja)}}").unwrap();
        assert!(phone.starts_with("090-"), "ja phone: {phone}");
        let wd = resolve("{{$date.weekday(locale=ja)}}").unwrap();
        assert!(wd.ends_with("曜日"), "ja weekday: {wd}");
        let city = resolve("{{$location.city(locale=ja)}}").unwrap();
        assert!(!city.is_ascii(), "ja city: {city}");
        let dish = resolve("{{$food.dish(locale=ja)}}").unwrap();
        assert!(!dish.is_ascii(), "ja dish: {dish}");
    }

    #[test]
    fn test_default_locale_ja_global() {
        // Global default ja (no argument-less assertions under parallel tests, to avoid races)
        set_default_locale(Locale::Ja);
        assert_eq!(default_locale(), Locale::Ja);
        set_default_locale(Locale::Zh);
        assert_eq!(default_locale(), Locale::Zh);
    }

    #[test]
    fn test_default_locale_en_global() {
        // Global default en (no argument-less assertions under parallel tests, to avoid races; only verify setter/getter and explicit override)
        set_default_locale(Locale::En);
        assert_eq!(default_locale(), Locale::En);
        // Expression-level locale=zh overrides global en
        let zh = resolve("{{$person.fullName(locale=zh)}}").unwrap();
        assert!(!zh.is_ascii(), "zh name: {zh}");
        // Restore the default to avoid affecting other tests
        set_default_locale(Locale::Zh);
        assert_eq!(default_locale(), Locale::Zh);
    }

    #[test]
    fn test_location_locale() {
        let city = resolve("{{$location.city(locale=en)}}").unwrap();
        assert!(city.is_ascii(), "en city: {city}");
        let zh = resolve("{{$location.city(locale=zh)}}").unwrap();
        assert!(!zh.is_ascii(), "zh city: {zh}");
    }

    #[test]
    fn test_food_and_vehicle_locale() {
        let dish = resolve("{{$food.dish(locale=en)}}").unwrap();
        assert!(dish.is_ascii(), "en dish: {dish}");
        let brand = resolve("{{$vehicle.brand(locale=en)}}").unwrap();
        assert!(brand.is_ascii(), "en brand: {brand}");
    }

    #[test]
    fn test_date_weekday_zh() {
        let wd = resolve("{{$date.weekday(locale=zh)}}").unwrap();
        assert!(wd.starts_with("星期"), "zh weekday: {wd}");
        let month = resolve("{{$date.monthName(locale=zh, format=MMMM)}}").unwrap();
        assert!(month.ends_with("月"), "zh month: {month}");
    }

    #[test]
    fn test_phone_locale_en() {
        let phone = resolve("{{$phone.mobile(locale=en)}}").unwrap();
        assert!(phone.starts_with("+1-"), "en phone: {phone}");
    }
}
