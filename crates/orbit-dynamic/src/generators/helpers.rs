//! Helper generators: helpers (enum/regex/symbol replacement, etc.) + image

use super::{arg_i64, random_choice, random_range, Args};
use crate::DynamicError;

pub fn gen_helpers(method: &str, args: &Args) -> Result<String, DynamicError> {
    match method {
        "arrayElement" => {
            let raw = args
                .get("0")
                .cloned()
                .or_else(|| args.values().next().cloned())
                .unwrap_or_default();
            let items: Vec<String> = if raw.contains('[') {
                raw.trim_matches(|c: char| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if raw.contains(',') {
                raw.split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec![raw]
            };
            if items.is_empty() {
                return Ok("".to_string());
            }
            Ok(random_choice(&items))
        }
        "fromRegExp" => {
            let pattern = args
                .get("0")
                .or_else(|| args.values().next())
                .cloned()
                .unwrap_or_else(|| "[A-Z]{2}[0-9]{4}".into());
            let pattern = pattern.trim_matches('/');
            generate_from_regex(pattern)
        }
        "replaceSymbols" => {
            let template = args
                .get("0")
                .or_else(|| args.values().next())
                .cloned()
                .unwrap_or_else(|| "##??".into());
            Ok(replace_symbols(&template))
        }
        "slugify" => {
            let text = args
                .get("0")
                .or_else(|| args.values().next())
                .cloned()
                .unwrap_or_else(|| "hello world".into());
            Ok(text.to_lowercase().replace(' ', "-"))
        }
        "rangeToNumber" => {
            let min = arg_i64(args, "min", 1);
            let max = arg_i64(args, "max", 100);
            Ok(random_range(min, max).to_string())
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "helpers".into(),
            method: method.into(),
        }),
    }
}

pub fn gen_image(method: &str, args: &Args) -> Result<String, DynamicError> {
    match method {
        "url" => {
            let w = arg_i64(args, "width", 300);
            let h = arg_i64(args, "height", 300);
            let seed = random_range(1, 9999999999);
            Ok(format!("https://picsum.photos/{}/{}?random={}", w, h, seed))
        }
        "avatar" => {
            let w = arg_i64(args, "width", 200);
            Ok(format!(
                "https://i.pravatar.cc/{}?u={}",
                w,
                random_range(1, 9999)
            ))
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "image".into(),
            method: method.into(),
        }),
    }
}

fn generate_from_regex(pattern: &str) -> Result<String, DynamicError> {
    let mut result = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '[' => {
                let mut class_chars = Vec::new();
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    if i + 2 < chars.len() && chars[i + 1] == '-' {
                        let start = chars[i] as u8;
                        let end = chars[i + 2] as u8;
                        for c in start..=end {
                            class_chars.push(c as char);
                        }
                        i += 3;
                    } else {
                        class_chars.push(chars[i]);
                        i += 1;
                    }
                }
                i += 1; // skip ']'
                if !class_chars.is_empty() {
                    result.push(random_choice(&class_chars));
                }
            }
            '{' => {
                i += 1;
                let mut num_str = String::new();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    num_str.push(chars[i]);
                    i += 1;
                }
                let count: usize = num_str.parse().unwrap_or(1);
                let last_char = result.chars().last().unwrap_or('a');
                for _ in 1..count {
                    result.push(last_char);
                }
                i += 1; // skip '}'
            }
            '\\' => {
                i += 1;
                if i < chars.len() {
                    match chars[i] {
                        'd' => result.push(random_choice(&[
                            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
                        ])),
                        'w' => result.push(random_choice(&[
                            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n',
                            'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
                        ])),
                        's' => result.push(' '),
                        _ => result.push(chars[i]),
                    }
                    i += 1;
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }
    Ok(result)
}

fn replace_symbols(template: &str) -> String {
    template
        .chars()
        .map(|c| match c {
            '#' => random_choice(&['0', '1', '2', '3', '4', '5', '6', '7', '8', '9']),
            '?' => random_choice(&[
                'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
                'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
            ]),
            '*' => random_choice(&[
                'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
                'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5',
                '6', '7', '8', '9',
            ]),
            _ => c,
        })
        .collect()
}
