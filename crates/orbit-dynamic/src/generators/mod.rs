//! Generator dispatch and shared helper functions

use std::collections::HashMap;

use rand::Rng;

use crate::locale::Locale;

use super::DynamicError;

pub type Args = HashMap<String, String>;

pub fn generate(category: &str, method: &str, args: &Args) -> Result<String, DynamicError> {
    match category {
        "string" => basic::gen_string(method, args),
        "number" => basic::gen_number(method, args),
        "date" => date::gen_date(method, args),
        "person" => person::gen_person(method, args),
        "internet" => internet::gen_internet(method, args),
        "phone" => internet::gen_phone(method, args),
        "location" => location::gen_location(method, args),
        "commerce" => commerce::gen_commerce(method, args),
        "company" => commerce::gen_company(method, args),
        "finance" => commerce::gen_finance(method, args),
        "helpers" => helpers::gen_helpers(method, args),
        "datatype" => basic::gen_datatype(method, args),
        "image" => helpers::gen_image(method, args),
        "lorem" => named::gen_lorem(method, args),
        "color" => named::gen_color(method, args),
        "food" => named::gen_food(method, args),
        "vehicle" => named::gen_vehicle(method, args),
        "music" => named::gen_music(method, args),
        _ => Err(DynamicError::UnknownCategory(category.to_string())),
    }
}

pub fn apply_pipe(value: &str, pipe: &str, _args: &Args) -> Result<String, DynamicError> {
    match pipe {
        "toUpperCase" => Ok(value.to_uppercase()),
        "toLowerCase" => Ok(value.to_lowercase()),
        "trim" => Ok(value.trim().to_string()),
        _ => Err(DynamicError::InvalidArgs(format!("Unknown pipe: {}", pipe))),
    }
}

/// Resolve the locale from generator arguments: the `locale=` argument takes priority, falling back to the global default
pub(crate) fn resolve_locale(args: &Args) -> Locale {
    args.get("locale")
        .and_then(|s| Locale::parse(s))
        .unwrap_or_else(crate::default_locale)
}

// ─── Shared random/argument helpers ──────────────────

pub(crate) fn arg_i64(args: &Args, key: &str, default: i64) -> i64 {
    args.get(key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn arg_f64(args: &Args, key: &str, default: f64) -> f64 {
    args.get(key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn random_range(min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    rand::thread_rng().gen_range(min..=max)
}

pub(crate) fn random_float_range(min: f64, max: f64) -> f64 {
    if min >= max {
        return min;
    }
    rand::thread_rng().gen_range(min..=max)
}

pub(crate) fn random_choice<T: Clone>(items: &[T]) -> T {
    let idx = rand::thread_rng().gen_range(0..items.len());
    items[idx].clone()
}

pub(crate) fn random_string(len: usize, charset: &[u8]) -> String {
    (0..len).map(|_| random_choice(charset) as char).collect()
}

mod basic;
mod commerce;
mod date;
mod helpers;
mod internet;
mod location;
mod named;
mod person;
