//! Network / phone generator

use super::{arg_i64, random_choice, random_range, random_string, resolve_locale, Args};
use crate::DynamicError;
use rand::Rng;

pub fn gen_internet(method: &str, args: &Args) -> Result<String, DynamicError> {
    let domain = args
        .get("domain")
        .cloned()
        .unwrap_or_else(|| "example.com".into());

    match method {
        "email" => {
            let name = random_choice(&["james", "mary", "robert", "patricia", "john", "jennifer"])
                .to_string();
            let num: u32 = rand::thread_rng().gen_range(100..999);
            Ok(format!("{}{}@{}", name, num, domain))
        }
        "url" => {
            let protocol = args
                .get("protocol")
                .cloned()
                .unwrap_or_else(|| "https".into());
            let path = random_choice(&["/api/v1", "/users", "/products", "/orders", "/health"]);
            Ok(format!("{}://{}{}", protocol, domain, path))
        }
        "ip" => {
            let mut rng = rand::thread_rng();
            Ok(format!(
                "{}.{}.{}.{}",
                rng.gen_range(1..255),
                rng.gen_range(0..255),
                rng.gen_range(0..255),
                rng.gen_range(1..255)
            ))
        }
        "ipv6" => {
            let mut rng = rand::thread_rng();
            Ok(format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
                rng.gen_range(0..65535),
            ))
        }
        "userName" => {
            let name = random_choice(&["james", "mary", "robert", "patricia", "john", "jennifer"]);
            let num: u32 = rand::thread_rng().gen_range(10..99);
            Ok(format!("{}{}", name, num))
        }
        "password" => {
            let len = arg_i64(args, "length", 12) as usize;
            Ok(random_string(
                len,
                b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$",
            ))
        }
        "domainName" => Ok(domain),
        "port" => Ok(random_range(1024, 65535).to_string()),
        "httpMethod" => Ok(random_choice(&["GET", "POST", "PUT", "DELETE", "PATCH"]).to_string()),
        "userAgent" => Ok(random_choice(&[
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
        ])
        .to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "internet".into(),
            method: method.into(),
        }),
    }
}

pub fn gen_phone(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    match method {
        "mobile" => match locale {
            crate::locale::Locale::En => {
                // US-style mobile number: +1-555-xxx-xxxx
                let a = random_range(200, 999);
                let b = random_range(1000, 9999);
                Ok(format!("+1-555-{}-{}", a, b))
            }
            crate::locale::Locale::Ja => {
                // Japanese mobile number: 090-xxxx-xxxx
                let a = random_range(1000, 9999);
                let b = random_range(1000, 9999);
                Ok(format!("090-{}-{}", a, b))
            }
            crate::locale::Locale::Zh => {
                let prefixes = [
                    "130", "131", "132", "133", "134", "135", "136", "137", "138", "139", "150",
                    "151", "152", "153", "155", "156", "157", "158", "159", "180", "181", "182",
                    "183", "184", "185", "186", "187", "188", "189",
                ];
                let prefix = random_choice(&prefixes);
                Ok(format!("{}{}", prefix, random_range(10000000, 99999999)))
            }
        },
        "number" => {
            let style = arg_i64(args, "style", 0);
            if style == 1 {
                let area = random_range(200, 999);
                let a = random_range(200, 999);
                let b = random_range(1000, 9999);
                Ok(format!("({}) {}-{}", area, a, b))
            } else {
                Ok(random_range(10000000000, 19999999999).to_string())
            }
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "phone".into(),
            method: method.into(),
        }),
    }
}
