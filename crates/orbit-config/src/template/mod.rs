//! Template layer - variable interpolation / arithmetic expression evaluation / dynamic values

use std::collections::HashMap;

use crate::error::ConfigError;

mod arithmetic;
mod pattern;

use self::arithmetic::eval_arithmetic;
use self::pattern::{ARITH_PATTERN, DOUBLE_VAR_PATTERN, VAR_PATTERN};

/// Variable interpolation - replaces `${var}`, `${env:var}`, `{{var}}` and dynamic values `{{$category.method}}`
///
/// Processing order:
/// Variable interpolation supports four forms:
/// 1. `${var}` / `${env:var}` - plain variable / environment variable substitution (errors when missing)
/// 2. `${=expr}`             - arithmetic expression evaluation (e.g. `${=loop.index * 100}`)
/// 3. `{{var}}`              - application-layer double-brace syntax (frontend API management / importers);
///    missing variables are left as-is
/// 4. `{{$category.method}}` - dynamic values
///
/// Arithmetic expressions (`${=...}`) support `+ - * / %`, parentheses, unary minus and variable references.
/// Variable values are parsed as floating point numbers for the computation, e.g. `offset=${=loop.index * pageSize}`.
/// The evaluator only performs numeric operations, calls no functions and runs no external code, so there is no injection risk.
pub fn interpolate(template: &str, vars: &HashMap<String, String>) -> Result<String, ConfigError> {
    let mut result = template.to_string();

    // 1. Arithmetic expressions: ${=expr}
    for cap in ARITH_PATTERN.captures_iter(template) {
        let full = &cap[0];
        let expr = &cap[1];
        match eval_arithmetic(expr, vars) {
            Ok(v) => {
                let val = if v.is_finite() && v.fract() == 0.0 {
                    format!("{}", v as i64)
                } else {
                    format!("{}", v)
                };
                result = result.replace(full, &val);
            }
            Err(e) => {
                return Err(ConfigError::Validation(format!(
                    "Expression '{}': {}",
                    expr, e
                )));
            }
        }
    }

    // 2. Plain variable substitution: ${var} / ${env:var}
    // Collect the captures first (owning them) to avoid mutably borrowing `result` while iterating it
    let caps: Vec<(String, String)> = VAR_PATTERN
        .captures_iter(&result)
        .map(|cap| (cap[0].to_string(), cap[1].to_string()))
        .collect();
    for (full, name) in caps {
        let value = vars
            .get(&name)
            .cloned()
            .or_else(|| std::env::var(&name).ok())
            .ok_or_else(|| ConfigError::VariableNotFound(name.clone()))?;

        result = result.replace(&full, &value);
    }

    // 2b. Application-layer double-brace plain variables `{{var}}` (variable syntax of frontend API management / importers).
    //     `{{$...}}` dynamic values are not handled here (step 3, orbit_dynamic, generates them per request).
    //     Missing variables are left as-is without failing the whole call: pipeline::interp falls back via unwrap_or_else,
    //     since returning Err here would roll back already-substituted variables in the same string.
    {
        let caps: Vec<(String, String)> = DOUBLE_VAR_PATTERN
            .captures_iter(&result)
            .filter_map(|cap| {
                let name = cap[1].trim();
                if name.is_empty() || name.starts_with('$') {
                    return None;
                }
                Some((cap[0].to_string(), name.to_string()))
            })
            .collect();
        for (full, name) in caps {
            if let Some(value) = vars
                .get(&name)
                .cloned()
                .or_else(|| std::env::var(&name).ok())
            {
                result = result.replace(&full, &value);
            }
        }
    }

    // 3. Dynamic values
    result = orbit_dynamic::resolve(&result)
        .map_err(|e| ConfigError::Validation(format!("Dynamic value error: {}", e)))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate() {
        let mut vars = HashMap::new();
        vars.insert("base_url".into(), "https://api.example.com".into());
        vars.insert("token".into(), "abc123".into());

        let result = interpolate("${base_url}/users", &vars).unwrap();
        assert_eq!(result, "https://api.example.com/users");

        let result = interpolate("Bearer ${token}", &vars).unwrap();
        assert_eq!(result, "Bearer abc123");
    }

    #[test]
    fn test_interpolate_missing() {
        let vars = HashMap::new();
        let result = interpolate("${missing}", &vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_interpolate_double_brace() {
        let mut vars = HashMap::new();
        vars.insert("username".into(), "alice".into());

        // Application-layer {{var}} syntax (frontend API management / dataset row variables)
        assert_eq!(
            interpolate("http://x?username={{username}}", &vars).unwrap(),
            "http://x?username=alice"
        );
        // Mixed with ${var}
        vars.insert("token".into(), "t1".into());
        assert_eq!(
            interpolate("?u={{username}}&t=${token}", &vars).unwrap(),
            "?u=alice&t=t1"
        );
        // Spaces are allowed inside placeholders
        assert_eq!(interpolate("{{ username }}", &vars).unwrap(), "alice");
        // Missing variables are left as-is (no whole-call failure, avoiding rollback of already-substituted variables)
        assert_eq!(
            interpolate("?u={{missing}}&t=${token}", &vars).unwrap(),
            "?u={{missing}}&t=t1"
        );
    }

    #[test]
    fn test_interpolate_arithmetic() {
        let mut vars = HashMap::new();
        vars.insert("page_size".into(), "10".into());
        vars.insert("base".into(), "0".into());

        // Pure arithmetic
        assert_eq!(interpolate("${=2 * 3}", &vars).unwrap(), "6");
        // Variables in the computation
        assert_eq!(
            interpolate("${=loop.index * page_size}", &make_vars(2)).unwrap(),
            "20"
        );
        // Parentheses and precedence
        assert_eq!(interpolate("${=(1 + 2) * 3}", &vars).unwrap(), "9");
        // Unary minus
        assert_eq!(interpolate("${=-loop.index}", &make_vars(5)).unwrap(), "-5");
        // Mixed interpolation: URL path + arithmetic
        assert_eq!(
            interpolate("/items?offset=${=loop.index * page_size}", &make_vars(1)).unwrap(),
            "/items?offset=10"
        );
        // Floating point results keep their decimals
        assert_eq!(interpolate("${=7 / 2}", &vars).unwrap(), "3.5");
    }

    #[test]
    fn test_interpolate_arithmetic_errors() {
        let vars = HashMap::new();
        // Unknown variable
        assert!(interpolate("${=loop.index * 10}", &vars).is_err());
        // Division by zero
        assert!(interpolate("${=1 / 0}", &vars).is_err());
        // Illegal character
        assert!(interpolate("${=2 # 3}", &vars).is_err());
    }

    fn make_vars(index: i64) -> HashMap<String, String> {
        let mut v = HashMap::new();
        v.insert("loop.index".into(), index.to_string());
        v.insert("page_size".into(), "10".into());
        v
    }
}
