//! Language-agnostic basic generators: string / number / datatype

use super::{
    arg_f64, arg_i64, random_choice, random_float_range, random_range, random_string, Args,
};
use crate::DynamicError;
use rand::Rng;

// ─── string ────────────────────────────────────────────

pub fn gen_string(method: &str, args: &Args) -> Result<String, DynamicError> {
    match method {
        "uuid" => Ok(uuid::Uuid::new_v4().to_string()),
        "alpha" => {
            let len = arg_i64(args, "length", 10) as usize;
            Ok(random_string(len, b"abcdefghijklmnopqrstuvwxyz"))
        }
        "alphanumeric" => {
            let len = arg_i64(args, "length", 10) as usize;
            Ok(random_string(len, b"abcdefghijklmnopqrstuvwxyz0123456789"))
        }
        "numeric" => {
            let len = arg_i64(args, "length", 10) as usize;
            Ok(random_string(len, b"0123456789"))
        }
        "hexadecimal" => {
            let len = arg_i64(args, "length", 8) as usize;
            Ok(random_string(len, b"0123456789abcdef"))
        }
        "symbol" => {
            let len = arg_i64(args, "length", 1) as usize;
            Ok(random_string(len, b"!@#$%^&*()_+-=[]{}|;:,.<>?"))
        }
        "sample" => {
            let count = arg_i64(args, "count", 1) as usize;
            let mut rng = rand::thread_rng();
            Ok((0..count)
                .map(|_| rng.gen_range(b'a'..=b'z') as char)
                .collect())
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "string".into(),
            method: method.into(),
        }),
    }
}

// ─── number ────────────────────────────────────────────

pub fn gen_number(method: &str, args: &Args) -> Result<String, DynamicError> {
    match method {
        "int" => {
            let min = arg_i64(args, "min", 0);
            let max = arg_i64(args, "max", 100);
            Ok(random_range(min, max).to_string())
        }
        "float" => {
            let min = arg_f64(args, "min", 0.0);
            let max = arg_f64(args, "max", 100.0);
            let decimals = arg_i64(args, "decimals", 2) as usize;
            let val = random_float_range(min, max);
            Ok(format!("{:.precision$}", val, precision = decimals))
        }
        "hex" => {
            let min = arg_i64(args, "min", 0);
            let max = arg_i64(args, "max", 255);
            Ok(format!("{:x}", random_range(min, max)))
        }
        "binary" => {
            let min = arg_i64(args, "min", 0);
            let max = arg_i64(args, "max", 255);
            Ok(format!("{:b}", random_range(min, max)))
        }
        "octal" => {
            let min = arg_i64(args, "min", 0);
            let max = arg_i64(args, "max", 255);
            Ok(format!("{:o}", random_range(min, max)))
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "number".into(),
            method: method.into(),
        }),
    }
}

// ─── datatype ──────────────────────────────────────────

pub fn gen_datatype(method: &str, _args: &Args) -> Result<String, DynamicError> {
    match method {
        "boolean" => Ok(random_choice(&["true", "false"]).to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "datatype".into(),
            method: method.into(),
        }),
    }
}
