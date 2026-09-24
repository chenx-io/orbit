//! Cookie extractor

use crate::context::ExtractContext;
use crate::error::ExtractError;
use crate::traits::{ExtractionKind, Extractor};

/// Cookie extractor: extracts a cookie from the Set-Cookie header in the response headers
///
/// - `attr: None` -> extract the cookie value
/// - `attr: Some("domain")` -> extract the cookie's domain attribute
/// - `attr: Some("path")` -> extract the cookie's path attribute
/// - `attr: Some("expires")` -> extract the cookie's expires attribute
pub struct CookieExtractor {
    pub name: String,
    pub attr: Option<String>,
}

impl CookieExtractor {
    pub fn new(name: String, attr: Option<String>) -> Self {
        Self { name, attr }
    }
}

impl Extractor for CookieExtractor {
    fn kind(&self) -> ExtractionKind {
        ExtractionKind::Cookie
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        // There may be several Set-Cookie headers (one cookie per line)
        // or they may be merged into one (comma separated, but expires also contains commas - needs special care)
        let set_cookie_headers: Vec<&String> = ctx
            .headers()
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v)
            .collect();

        for header_val in &set_cookie_headers {
            // Try to match the target cookie in this header
            // Format: name=value; Domain=...; Path=...; Expires=...; Secure; HttpOnly
            if let Some(result) =
                parse_set_cookie_value(header_val, &self.name, self.attr.as_deref())
            {
                return Ok(result);
            }
        }

        // Try splitting multiple cookies by comma (handles merged Set-Cookie headers)
        for header_val in &set_cookie_headers {
            for part in split_cookies(header_val) {
                if let Some(result) =
                    parse_set_cookie_value(&part, &self.name, self.attr.as_deref())
                {
                    return Ok(result);
                }
            }
        }

        Ok(String::new())
    }
}

/// Parse a single Set-Cookie entry
fn parse_set_cookie_value(
    header_val: &str,
    cookie_name: &str,
    attr: Option<&str>,
) -> Option<String> {
    let trimmed = header_val.trim();
    // Take the part before the first semicolon = name=value
    let first_part = trimmed.split(';').next().unwrap_or(trimmed).trim();

    // Parse name=value
    if let Some(eq_pos) = first_part.find('=') {
        let name = first_part[..eq_pos].trim();
        if name.eq_ignore_ascii_case(cookie_name) {
            let value = first_part[eq_pos + 1..].trim();

            if let Some(attr_name) = attr {
                // Extract the cookie attribute
                return Some(extract_cookie_attr(trimmed, attr_name).unwrap_or_default());
            } else {
                // Return the cookie value
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Extract an attribute from the Set-Cookie header
fn extract_cookie_attr(header_val: &str, attr_name: &str) -> Option<String> {
    let parts: Vec<&str> = header_val.split(';').collect();
    for part in &parts[1..] {
        // Skip leading whitespace
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            let key = part[..eq_pos].trim();
            let val = part[eq_pos + 1..].trim();
            if key.eq_ignore_ascii_case(attr_name) {
                return Some(val.to_string());
            }
        } else if part.eq_ignore_ascii_case(attr_name) {
            // Value-less attributes such as Secure, HttpOnly
            return Some("true".to_string());
        }
    }
    None
}

/// Smartly split multiple cookies (handles the comma inside Expires)
fn split_cookies(header_val: &str) -> Vec<String> {
    let mut cookies = Vec::new();
    let mut current = String::new();
    let mut in_expires = false;

    for part in header_val.split(',') {
        let trimmed = part.trim();
        if !in_expires {
            // Check whether the previous part starts with Expires
            if current.trim().to_lowercase().contains("expires=") {
                // The previous part was expires, the current part may be the rest of the date
                current.push(',');
                current.push_str(part);
                // Check whether the date ends (contains GMT or UTC or a numeric time)
                if trimmed.contains("GMT")
                    || trimmed.contains("UTC")
                    || trimmed.ends_with(|c: char| c.is_ascii_digit())
                {
                    in_expires = false;
                }
                continue;
            }
            if !current.is_empty() {
                cookies.push(std::mem::take(&mut current));
            }
            current = part.to_string();
        }
    }

    if !current.is_empty() {
        cookies.push(current);
    }

    cookies
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn extract_cookie(headers: &HashMap<String, String>, name: &str, attr: Option<&str>) -> String {
        CookieExtractor::new(name.into(), attr.map(|s| s.into()))
            .extract(&ExtractContext::new("", headers))
            .unwrap()
    }

    #[test]
    fn test_extract_cookie_value() {
        let headers = headers(&[("set-cookie", "session=abc123; Path=/; HttpOnly")]);
        assert_eq!(extract_cookie(&headers, "session", None), "abc123");
    }

    #[test]
    fn test_extract_cookie_attr() {
        let headers = headers(&[(
            "set-cookie",
            "session=abc123; Path=/app; Domain=example.com; Secure",
        )]);
        assert_eq!(extract_cookie(&headers, "session", Some("path")), "/app");
        assert_eq!(
            extract_cookie(&headers, "session", Some("domain")),
            "example.com"
        );
        assert_eq!(extract_cookie(&headers, "session", Some("secure")), "true");
    }

    #[test]
    fn test_extract_cookie_not_found() {
        let headers = headers(&[("set-cookie", "token=xyz; Path=/")]);
        assert_eq!(extract_cookie(&headers, "session", None), "");
    }

    #[test]
    fn test_extract_cookie_case_insensitive() {
        let headers = headers(&[("Set-Cookie", "SID=hello123; HttpOnly")]);
        assert_eq!(extract_cookie(&headers, "sid", None), "hello123");
    }

    #[test]
    fn test_extract_cookie_multiple() {
        // In HTTP, multiple headers with the same name may be merged into a comma-separated value
        let headers = headers(&[("set-cookie", "a=1; Path=/, b=2; Path=/")]);
        assert_eq!(extract_cookie(&headers, "a", None), "1");
        assert_eq!(extract_cookie(&headers, "b", None), "2");
    }
}
