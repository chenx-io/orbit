//! Date generator (time zone / offset / random range); weekday/month names are taken from the dataset by locale

use jiff::civil;
use jiff::{Span, Timestamp, Zoned};

use crate::data::dataset;

use super::{arg_i64, random_range, resolve_locale, Args};
use crate::DynamicError;

/// Weekday index (jiff `%u`: Monday=1)
fn weekday_idx(dt: &Zoned) -> usize {
    (dt.strftime("%u").to_string().parse::<usize>().unwrap_or(1) - 1) % 7
}

pub fn gen_date(method: &str, args: &Args) -> Result<String, DynamicError> {
    let now = now_in_tz(args);
    let locale = resolve_locale(args);
    let names = dataset(locale);
    let fmt_now = || {
        args.get("format")
            .cloned()
            .unwrap_or_else(|| "yyyy-MM-dd HH:mm:ss".to_string())
    };

    // Pure-name mode (EEEE/EEE/%A/%a, MMMM/MMM/%B/%b) takes the weekday/month name for the given language from the dataset
    let fmt = |dt: &Zoned, f: &str| -> Result<String, DynamicError> {
        match f.trim() {
            "EEEE" | "%A" => Ok(names.weekdays[weekday_idx(dt)].to_string()),
            "EEE" | "E" | "%a" => Ok(names.weekdays_short[weekday_idx(dt)].to_string()),
            "MMMM" | "%B" => Ok(names.months[(dt.month() as usize - 1) % 12].to_string()),
            "MMM" | "%b" => Ok(names.months_short[(dt.month() as usize - 1) % 12].to_string()),
            _ => crate::format::format_datetime(dt, f),
        }
    };

    match method {
        "now" => fmt(&now, &fmt_now()),
        "time" => {
            let f = args
                .get("format")
                .cloned()
                .unwrap_or_else(|| "HH:mm:ss".to_string());
            fmt(&now, &f)
        }
        "timestamp" => Ok(now.timestamp().as_second().to_string()),
        "timestampMs" => Ok(now.timestamp().as_millisecond().to_string()),
        "today" => {
            let f = args
                .get("format")
                .cloned()
                .unwrap_or_else(|| "yyyy-MM-dd".to_string());
            fmt(&now, &f)
        }
        "year" => Ok(now.year().to_string()),
        "month" => Ok(now.month().to_string()),
        "day" => Ok(now.day().to_string()),
        "hour" => Ok(now.hour().to_string()),
        "minute" => Ok(now.minute().to_string()),
        "second" => Ok(now.second().to_string()),
        "iso" => Ok(now.timestamp().to_string()),
        "isoDate" => {
            let f = args
                .get("format")
                .cloned()
                .unwrap_or_else(|| "yyyy-MM-dd".to_string());
            fmt(&now, &f)
        }
        "timeZone" => Ok(now
            .time_zone()
            .iana_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| now.strftime("%z").to_string())),
        "weekday" => {
            let f = args
                .get("format")
                .cloned()
                .unwrap_or_else(|| "EEEE".to_string());
            fmt(&now, &f)
        }
        "monthName" => {
            let f = args
                .get("format")
                .cloned()
                .unwrap_or_else(|| "MMMM".to_string());
            fmt(&now, &f)
        }
        "offset" => {
            let amount = arg_i64(args, "amount", 0);
            let unit = args.get("unit").cloned().unwrap_or_else(|| "days".into());
            let target = apply_offset(&now, &unit, amount).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "past" => {
            // Backward-compatible with the old days parameter; new parameters are unit/amount
            let amount = if args.contains_key("amount") {
                arg_i64(args, "amount", 30)
            } else {
                arg_i64(args, "days", 30)
            };
            let unit = args.get("unit").cloned().unwrap_or_else(|| "days".into());
            let target = apply_offset(&now, &unit, amount.checked_neg().unwrap_or(amount))
                .unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "future" => {
            let amount = if args.contains_key("amount") {
                arg_i64(args, "amount", 30)
            } else {
                arg_i64(args, "days", 30)
            };
            let unit = args.get("unit").cloned().unwrap_or_else(|| "days".into());
            let target = apply_offset(&now, &unit, amount).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "random" => {
            let start = args
                .get("start")
                .and_then(|s| parse_bound(s, &now))
                .unwrap_or_else(|| apply_offset(&now, "years", -50).unwrap_or(now.clone()));
            let end = args
                .get("end")
                .and_then(|s| parse_bound(s, &now))
                .unwrap_or_else(|| apply_offset(&now, "years", 50).unwrap_or(now.clone()));
            let (a, b) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let target = random_between(&a, &b).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "between" => {
            let start = args
                .get("start")
                .and_then(|s| parse_bound(s, &now))
                .unwrap_or_else(|| apply_offset(&now, "years", -50).unwrap_or(now.clone()));
            let end = args
                .get("end")
                .and_then(|s| parse_bound(s, &now))
                .unwrap_or_else(|| apply_offset(&now, "years", 50).unwrap_or(now.clone()));
            let (a, b) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let target = random_between(&a, &b).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "pastRandom" => {
            let days = arg_i64(args, "days", 30);
            let start = apply_offset(&now, "days", -days).unwrap_or(now.clone());
            let target = random_between(&start, &now).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        "futureRandom" => {
            let days = arg_i64(args, "days", 30);
            let end = apply_offset(&now, "days", days).unwrap_or(now.clone());
            let target = random_between(&now, &end).unwrap_or(now.clone());
            fmt(&target, &fmt_now())
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "date".into(),
            method: method.into(),
        }),
    }
}

/// Get the current time (Zoned). Supports a `tz` argument to specify the time zone:
/// - IANA name: `tz=Asia/Shanghai` / `tz=UTC` / `tz=America/New_York`
/// - Fixed offset: `tz=+08:00` / `tz=-05:30` / `tz=+0800` / `tz=-5`
/// - Default / parse failure: fall back to the local time zone
fn now_in_tz(args: &Args) -> Zoned {
    let ts = jiff::Timestamp::now();
    match args.get("tz").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => ts.to_zoned(jiff::tz::TimeZone::system()),
        Some(name) => {
            // 1) IANA name (including "UTC")
            if let Ok(tz) = jiff::tz::TimeZone::get(name) {
                return ts.to_zoned(tz);
            }
            // 2) Fixed offset: +08:00 / -0530 / +8 / Z
            if let Some(secs) = parse_offset_seconds(name) {
                if let Ok(offset) = jiff::tz::Offset::from_seconds(secs) {
                    return ts.to_zoned(jiff::tz::TimeZone::fixed(offset));
                }
            }
            // 3) Fall back to local
            ts.to_zoned(jiff::tz::TimeZone::system())
        }
    }
}

/// Parse a fixed offset string into seconds: supports `+08:00` / `-0530` / `+8` / `Z` / `z`.
/// Returns None on failure.
fn parse_offset_seconds(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'+') => (1i32, &s[1..]),
        Some(b'-') => (-1i32, &s[1..]),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    // Normalize to 4 digits (HHMM) or 2 digits (HH)
    let (h, m) = match digits.len() {
        4 => (
            digits[..2].parse::<i32>().ok()?,
            digits[2..].parse::<i32>().ok()?,
        ),
        2 => (digits.parse::<i32>().ok()?, 0),
        1 => (digits.parse::<i32>().ok()?, 0),
        _ => return None,
    };
    if h > 23 || m > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

/// Offset by unit: years/months/days/hours/minutes/seconds (amount may be negative)
fn apply_offset(dt: &Zoned, unit: &str, amount: i64) -> Option<Zoned> {
    match unit {
        "years" => dt.checked_add(Span::new().years(amount)).ok(),
        "months" => dt.checked_add(Span::new().months(amount)).ok(),
        "days" => dt.checked_add(Span::new().days(amount)).ok(),
        "hours" => dt.checked_add(Span::new().hours(amount)).ok(),
        "minutes" => dt.checked_add(Span::new().minutes(amount)).ok(),
        "seconds" => dt.checked_add(Span::new().seconds(amount)).ok(),
        _ => None,
    }
}

/// Parse a relative bound: "-30d" / "+1y" / "7M" (units y/M/d/h/m/s)
fn parse_relative_bound(s: &str, now: &Zoned) -> Option<Zoned> {
    let s = s.trim();
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'+') => (1i64, &s[1..]),
        Some(b'-') => (-1i64, &s[1..]),
        _ => (1i64, s),
    };
    let split = rest.find(|c: char| c.is_ascii_alphabetic())?;
    let num: i64 = rest[..split].trim().parse().ok()?;
    let amount = sign * num;
    match &rest[split..] {
        "y" => now.checked_add(Span::new().years(amount)).ok(),
        "M" => now.checked_add(Span::new().months(amount)).ok(),
        "d" => now.checked_add(Span::new().days(amount)).ok(),
        "h" => now.checked_add(Span::new().hours(amount)).ok(),
        "m" => now.checked_add(Span::new().minutes(amount)).ok(),
        "s" => now.checked_add(Span::new().seconds(amount)).ok(),
        _ => None,
    }
}

/// Bound parsing: relative string -> RFC3339(Timestamp) -> zone-less civil DateTime -> space-separated -> date only
/// Bounds without an explicit offset are interpreted in `now`'s time zone (local by default, or the tz argument).
fn parse_bound(s: &str, now: &Zoned) -> Option<Zoned> {
    let s = s.trim();
    if let Some(dt) = parse_relative_bound(s, now) {
        return Some(dt);
    }
    let tz = now.time_zone().clone();
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Some(ts.to_zoned(tz));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(cd) = civil::DateTime::strptime(fmt, s) {
            return cd.to_zoned(tz.clone()).ok();
        }
        if let Ok(d) = civil::Date::strptime(fmt, s) {
            return d.at(0, 0, 0, 0).to_zoned(tz.clone()).ok();
        }
    }
    None
}

/// Random within a range (second precision; output uses start's time zone to stay consistent with the bounds)
fn random_between(start: &Zoned, end: &Zoned) -> Option<Zoned> {
    let s = start.timestamp().as_second();
    let e = end.timestamp().as_second();
    let ts = random_range(s, e);
    let tz = start.time_zone().clone();
    Timestamp::from_second(ts).ok().map(|t| t.to_zoned(tz))
}
