//! Multi-language datasets: one file per language (`zh.rs` / `en.rs` / `ja.rs`),
//! retrieved by [`Locale`] via [`dataset`]. Generators only read data and don't branch; the composition format
//! (address concatenation, name order, etc.) is handled in the generators with an exhaustive `match locale`.
//!
//! **Three steps to add a language**: (1) add a variant to [`crate::locale::Locale`]; (2) create `data/<lang>.rs`
//! filling in `Dataset`; (3) add a match branch in `dataset()`. Generators need no changes,
//! and the compiler will point out every spot that needs composition logic via exhaustive matching.

pub mod en;
pub mod ja;
pub mod zh;

use crate::locale::Locale;

/// Full word-list data for a language (adding a field requires updates in all three language files; the compiler enforces it)
#[derive(Debug, Clone, Copy)]
pub struct Dataset {
    // ── person ──
    /// Given name
    pub person_first: &'static [&'static str],
    /// Surname
    pub person_last: &'static [&'static str],
    /// Gender
    pub genders: &'static [&'static str],
    // ── location ──
    /// City
    pub cities: &'static [&'static str],
    /// Province / state / prefecture
    pub states: &'static [&'static str],
    /// Street
    pub streets: &'static [&'static str],
    /// District / county (composition word)
    pub counties: &'static [&'static str],
    /// Country
    pub countries: &'static [&'static str],
    // ── commerce / company / finance ──
    /// Product name
    pub products: &'static [&'static str],
    /// Department
    pub departments: &'static [&'static str],
    /// Company name prefix (a city word under zh)
    pub company_prefix: &'static [&'static str],
    /// Company name suffix
    pub company_suffix: &'static [&'static str],
    /// Company slogan
    pub company_catch: &'static [&'static str],
    /// Company business phrase
    pub company_bs: &'static [&'static str],
    /// Currency name
    pub currencies: &'static [&'static str],
    /// Transaction type
    pub tx_types: &'static [&'static str],
    // ── Word lists ──
    /// lorem word
    pub lorem_words: &'static [&'static str],
    /// Color name
    pub colors: &'static [&'static str],
    /// Dish
    pub dishes: &'static [&'static str],
    /// Vegetable
    pub vegetables: &'static [&'static str],
    /// Fruit
    pub fruits: &'static [&'static str],
    /// Meat
    pub meats: &'static [&'static str],
    /// Car brand
    pub vehicle_brands: &'static [&'static str],
    /// Car type
    pub vehicle_types: &'static [&'static str],
    /// Artist
    pub artists: &'static [&'static str],
    /// Album name word
    pub album_words: &'static [&'static str],
    /// Album name suffix
    pub album_suffixes: &'static [&'static str],
    /// Song name
    pub songs: &'static [&'static str],
    /// Music genre
    pub genres: &'static [&'static str],
    // ── date names (indices 0..7 / 0..12) ──
    /// Full weekday name (starting with Monday)
    pub weekdays: &'static [&'static str],
    /// Short weekday name
    pub weekdays_short: &'static [&'static str],
    /// Full month name
    pub months: &'static [&'static str],
    /// Short month name
    pub months_short: &'static [&'static str],
}

/// Get the dataset by locale
pub fn dataset(locale: Locale) -> &'static Dataset {
    match locale {
        Locale::Zh => &zh::DATASET,
        Locale::En => &en::DATASET,
        Locale::Ja => &ja::DATASET,
    }
}
