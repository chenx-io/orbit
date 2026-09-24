//! Cookie Jar (session persistence)
//!
//! `Set-Cookie` responses are stored automatically (per domain) and later requests to the same domain get the `Cookie` header -
//! matching browser / Postman cookie jar semantics (Domain suffix match + Path prefix + Secure / expiry constraints).
//!
//! Wired in at the unified pipeline layer, so single-shot (HTTP server / Tauri), load testing (FlowRunner, one jar per VU)
//! and the CLI (via Engine -> FlowRunner) all gain session persistence; each VU has its own session during load tests (the k6 approach).

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single cookie
#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// Owning domain (no leading dot, no port; from the Domain attribute or the response URL host)
    pub domain: String,
    /// Path attribute (defaults to "/")
    pub path: String,
    /// Sent over HTTPS only
    pub secure: bool,
    /// Expiry time (epoch seconds); None = session cookie (never cleaned in-process, dies with the jar)
    pub expires: Option<i64>,
}

/// Cookie collection organized by domain
#[derive(Debug, Default)]
pub struct CookieJar {
    cookies: HashMap<String, Vec<Cookie>>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Absorb Set-Cookie from response headers (url is the final URL of the owning request, used as the default domain when Domain is absent).
    /// Several set-cookies may be merged by an upstream into one header value (comma-joined), so they are always split before parsing.
    pub fn absorb(&mut self, url: &str, set_cookie_values: &[String]) {
        let Some(default_domain) = url_host(url) else {
            return;
        };
        for raw in set_cookie_values {
            for part in split_set_cookies(raw) {
                if let Some(c) = parse_set_cookie(&part, &default_domain) {
                    self.upsert(c);
                }
            }
        }
    }

    /// Build the `Cookie` header value for the given URL (None when nothing matches).
    /// For the same name only the most specific (longest Path) cookie is kept; several cookies are joined with `; `.
    pub fn header_for(&self, url: &str) -> Option<String> {
        let (host, path, scheme) = url_host_path(url)?;
        let now = now_secs();
        let mut matched: Vec<&Cookie> = Vec::new();
        for list in self.cookies.values() {
            for c in list {
                if cookie_matches(&host, &path, &scheme, c, now) {
                    matched.push(c);
                }
            }
        }
        if matched.is_empty() {
            return None;
        }
        matched.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let mut seen = HashSet::new();
        let mut parts = Vec::new();
        for c in matched {
            if !seen.insert(c.name.clone()) {
                continue;
            }
            parts.push(format!("{}={}", c.name, c.value));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }

    fn upsert(&mut self, cookie: Cookie) {
        let list = self.cookies.entry(cookie.domain.clone()).or_default();
        if let Some(idx) = list.iter().position(|c| c.name == cookie.name) {
            list[idx] = cookie;
        } else {
            list.push(cookie);
        }
        // Drop expired entries and ones just set to expire
        let now = now_secs();
        list.retain(|c| c.expires.map(|e| e > now).unwrap_or(true));
    }

    /// Current cookie count (for debugging / tests)
    pub fn len(&self) -> usize {
        self.cookies.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Merge the existing Cookie header with the jar-generated one: on a name clash the existing (user-configured) value wins and the jar only fills gaps.
pub fn merge_cookie_header(existing: &str, jar: &str) -> String {
    if existing.trim().is_empty() {
        return jar.to_string();
    }
    if jar.trim().is_empty() {
        return existing.to_string();
    }
    let parse_pairs = |s: &str| -> Vec<(String, String)> {
        s.split(';')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| match p.find('=') {
                Some(i) if i > 0 => (p[..i].trim().to_string(), p[i + 1..].to_string()),
                _ => (p.to_string(), String::new()),
            })
            .collect()
    };
    let mut map: HashMap<String, String> = parse_pairs(existing).into_iter().collect();
    for (k, v) in parse_pairs(jar) {
        map.entry(k).or_insert(v);
    }
    map.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Current time (epoch seconds)
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Extract the host from a URL (without port)
fn url_host(url: &str) -> Option<String> {
    url_host_path(url).map(|(h, _, _)| h)
}

/// Extract (host, path, scheme) from a URL
fn url_host_path(url: &str) -> Option<(String, String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().to_string();
    let scheme = parsed.scheme().to_ascii_lowercase();
    Some((host, path, scheme))
}

/// Domain suffix match + Path prefix + Secure / expiry constraints (RFC 6265 semantics)
fn cookie_matches(host: &str, req_path: &str, scheme: &str, c: &Cookie, now: i64) -> bool {
    if let Some(exp) = c.expires {
        if exp <= now {
            return false;
        }
    }
    if c.secure && scheme != "https" {
        return false;
    }
    if host != c.domain && !host.ends_with(&format!(".{}", c.domain)) {
        return false;
    }
    path_matches(req_path, &c.path)
}

/// RFC 6265 path-match: the request path starts with the cookie path and the boundary is `/` or the paths are identical
fn path_matches(req_path: &str, cookie_path: &str) -> bool {
    let cp = cookie_path;
    if cp == "/" {
        return true;
    }
    if !req_path.starts_with(cp) {
        return false;
    }
    if req_path.len() == cp.len() {
        return true;
    }
    if cp.ends_with('/') {
        return true;
    }
    req_path.as_bytes().get(cp.len()) == Some(&b'/')
}

/// Split a header value that may merge several Set-Cookies (a backend returning them comma-joined).
/// Work around the comma inside an Expires date ("Wed, 21 Oct 2026 07:28:00 GMT"):
/// when a segment starts with "digit month" and the previous segment contains Expires=, it is treated as the date part and merged back into the previous segment.
fn split_set_cookies(header_value: &str) -> Vec<String> {
    let raw_parts: Vec<&str> = header_value.split(',').collect();
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, p) in raw_parts.iter().enumerate() {
        let trimmed = p.trim();
        if i == 0 {
            cur.push_str(trimmed);
            continue;
        }
        // Shape "21 Oct 2026 ..." = an Expires date fragment -> merge back into the previous segment
        let bytes = trimmed.as_bytes();
        let is_date_fragment = !bytes.is_empty()
            && bytes[0].is_ascii_digit()
            && cur.to_ascii_lowercase().contains("expires=");
        if is_date_fragment {
            cur.push_str(", ");
            cur.push_str(trimmed);
        } else {
            if !cur.trim().is_empty() {
                parts.push(cur.trim().to_string());
            }
            cur = trimmed.to_string();
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

/// Parse a single Set-Cookie value; returns None when malformed
fn parse_set_cookie(raw: &str, default_domain: &str) -> Option<Cookie> {
    let segs: Vec<&str> = raw.split(';').map(|s| s.trim()).collect();
    let first = segs.first()?;
    let eq = first.find('=')?;
    if eq == 0 {
        return None;
    }
    let name = first[..eq].trim().to_string();
    let value = first[eq + 1..].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let mut domain: Option<String> = None;
    let mut path = "/".to_string();
    let mut secure = false;
    let mut expires: Option<i64> = None;
    let mut max_age: Option<i64> = None;

    for seg in &segs[1..] {
        if seg.is_empty() {
            continue;
        }
        let eq2 = seg.find('=');
        let attr = eq2
            .map(|i| &seg[..i])
            .unwrap_or(seg)
            .trim()
            .to_ascii_lowercase();
        let val = eq2.map(|i| seg[i + 1..].trim()).unwrap_or("");
        match attr.as_str() {
            "domain" => {
                let mut d = val.to_ascii_lowercase();
                if let Some(stripped) = d.strip_prefix('.') {
                    d = stripped.to_string();
                }
                if !d.is_empty() {
                    domain = Some(d);
                }
            }
            "path" => {
                if !val.is_empty() {
                    path = if val.starts_with('/') {
                        val.to_string()
                    } else {
                        format!("/{val}")
                    };
                }
            }
            "secure" => secure = true,
            "max-age" => {
                if let Ok(n) = val.parse::<i64>() {
                    max_age = Some(n);
                }
            }
            "expires" => {
                if let Some(t) = parse_http_date(val) {
                    expires = Some(t);
                }
            }
            _ => {}
        }
    }

    // RFC 6265: Max-Age takes precedence over Expires; Max-Age <= 0 means expire immediately
    if let Some(ma) = max_age {
        expires = Some(if ma > 0 { now_secs() + ma } else { 0 });
    }

    Some(Cookie {
        name,
        value,
        domain: domain.unwrap_or_else(|| default_domain.to_string()),
        path,
        secure,
        expires,
    })
}

/// Parse an HTTP date (RFC 1123 / RFC 850 / asctime) into epoch seconds
fn parse_http_date(s: &str) -> Option<i64> {
    jiff::fmt::rfc2822::parse(s.trim())
        .map(|zdt| zdt.timestamp().as_second())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match_basic() {
        let mut jar = CookieJar::new();
        jar.absorb(
            "https://example.com/login",
            &["session=abc123; Path=/; HttpOnly".to_string()],
        );
        assert_eq!(
            jar.header_for("https://example.com/api/users").as_deref(),
            Some("session=abc123")
        );
        // Other domains do not match
        assert!(jar.header_for("https://other.com/x").is_none());
        // Subdomain matching (without a Domain attribute only the exact host matches, see the next case)
    }

    #[test]
    fn domain_attribute_matches_subdomain() {
        let mut jar = CookieJar::new();
        jar.absorb(
            "https://api.example.com/login",
            &["token=xyz; Domain=example.com; Path=/".to_string()],
        );
        assert_eq!(
            jar.header_for("https://www.example.com/x").as_deref(),
            Some("token=xyz")
        );
    }

    #[test]
    fn path_matching() {
        let mut jar = CookieJar::new();
        jar.absorb("https://example.com/login", &["p=1; Path=/api".to_string()]);
        assert!(jar.header_for("https://example.com/api/users").is_some());
        assert!(jar.header_for("https://example.com/apix").is_none());
        assert!(jar.header_for("https://example.com/other").is_none());
    }

    #[test]
    fn expires_and_max_age() {
        let mut jar = CookieJar::new();
        // Max-Age=0 -> expire immediately, not stored
        jar.absorb("https://example.com/", &["gone=1; Max-Age=0".to_string()]);
        assert!(jar.header_for("https://example.com/").is_none());
        // Max-Age takes precedence over Expires
        jar.absorb(
            "https://example.com/",
            &["a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT; Max-Age=1".to_string()],
        );
        assert!(jar.header_for("https://example.com/").is_some());
    }

    #[test]
    fn overwrite_same_name() {
        let mut jar = CookieJar::new();
        jar.absorb("https://example.com/", &["s=v1; Path=/".to_string()]);
        jar.absorb("https://example.com/", &["s=v2; Path=/".to_string()]);
        assert_eq!(
            jar.header_for("https://example.com/").as_deref(),
            Some("s=v2")
        );
    }

    #[test]
    fn split_multiple_set_cookies() {
        let parts =
            split_set_cookies("a=1; Path=/, b=2; Path=/; Expires=Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(parts.len(), 2);
        assert!(parts[0].starts_with("a=1"));
        assert!(parts[1].starts_with("b=2"));
    }
}
