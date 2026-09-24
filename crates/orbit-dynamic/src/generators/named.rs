//! Word-list generators: lorem / color / food / vehicle / music (all data comes from `data::dataset`)

use crate::data::dataset;
use crate::locale::Locale;

use super::{arg_i64, random_choice, random_range, resolve_locale, Args};
use crate::DynamicError;

// ─── lorem ─────────────────────────────────────────────

pub fn gen_lorem(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    let pick = |count: usize| -> Vec<&'static str> {
        (0..count)
            .map(|_| random_choice(data.lorem_words))
            .collect()
    };
    match method {
        "word" => Ok(random_choice(data.lorem_words).to_string()),
        "words" => {
            let count = arg_i64(args, "count", 5) as usize;
            Ok(pick(count).join(" "))
        }
        "sentence" => {
            let count = arg_i64(args, "count", 8) as usize;
            let mut s = pick(count).join(" ");
            s.push('。');
            Ok(s)
        }
        "paragraph" => {
            let sentences = arg_i64(args, "sentences", 3) as usize;
            let paragraphs: Vec<String> = (0..sentences)
                .map(|_| {
                    let count = random_range(5, 12) as usize;
                    let mut s = pick(count).join(" ");
                    s.push('。');
                    s
                })
                .collect();
            Ok(paragraphs.join(" "))
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "lorem".into(),
            method: method.into(),
        }),
    }
}

// ─── color ─────────────────────────────────────────────

pub fn gen_color(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "name" => Ok(random_choice(data.colors).to_string()),
        "hex" => Ok(format!("#{:06x}", random_range(0, 16777215))),
        "rgb" => Ok(format!(
            "rgb({}, {}, {})",
            random_range(0, 255),
            random_range(0, 255),
            random_range(0, 255)
        )),
        _ => Err(DynamicError::UnknownMethod {
            category: "color".into(),
            method: method.into(),
        }),
    }
}

// ─── food ──────────────────────────────────────────────

pub fn gen_food(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "dish" | "name" => Ok(random_choice(data.dishes).to_string()),
        "vegetable" => Ok(random_choice(data.vegetables).to_string()),
        "fruit" => Ok(random_choice(data.fruits).to_string()),
        "meat" => Ok(random_choice(data.meats).to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "food".into(),
            method: method.into(),
        }),
    }
}

// ─── vehicle ───────────────────────────────────────────

pub fn gen_vehicle(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "manufacturer" | "brand" => Ok(random_choice(data.vehicle_brands).to_string()),
        "model" => {
            let brand = random_choice(data.vehicle_brands);
            let model = random_choice(&["A", "X", "S", "Pro", "Max", "Plus", "Lite"]);
            Ok(format!("{} {}{}", brand, model, random_range(3, 9)))
        }
        "type" => Ok(random_choice(data.vehicle_types).to_string()),
        "vin" => Ok(super::random_string(
            17,
            b"ABCDEFGHJKLMNPRSTUVWXYZ0123456789",
        )),
        _ => Err(DynamicError::UnknownMethod {
            category: "vehicle".into(),
            method: method.into(),
        }),
    }
}

// ─── music ─────────────────────────────────────────────

pub fn gen_music(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "artist" => Ok(random_choice(data.artists).to_string()),
        "album" => {
            let word = random_choice(data.album_words);
            let suffix = random_choice(data.album_suffixes);
            Ok(match locale {
                Locale::En => format!("{} {}", word, suffix),
                Locale::Zh => format!("{}{}", word, suffix),
                Locale::Ja => format!("{}{}", word, suffix),
            })
        }
        "songName" => Ok(random_choice(data.songs).to_string()),
        "genre" => Ok(random_choice(data.genres).to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "music".into(),
            method: method.into(),
        }),
    }
}
