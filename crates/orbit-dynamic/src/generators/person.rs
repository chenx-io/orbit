//! Person generator (names, etc.; multi-language data comes from `data::dataset`)

use jiff::{Span, Zoned};

use crate::data::dataset;
use crate::locale::Locale;

use super::{arg_i64, random_choice, random_range, resolve_locale, Args};
use crate::DynamicError;

pub fn gen_person(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        // Name order varies by language: zh/ja surname+given, en given+surname
        "fullName" => match locale {
            Locale::Zh => Ok(format!(
                "{}{}",
                random_choice(data.person_last),
                random_choice(data.person_first)
            )),
            Locale::En => Ok(format!(
                "{} {}",
                random_choice(data.person_first),
                random_choice(data.person_last)
            )),
            Locale::Ja => Ok(format!(
                "{}{}",
                random_choice(data.person_last),
                random_choice(data.person_first)
            )),
        },
        "firstName" => Ok(random_choice(data.person_first).to_string()),
        "lastName" => Ok(random_choice(data.person_last).to_string()),
        "idCard" => {
            let min_age = arg_i64(args, "minAge", 18);
            let max_age = arg_i64(args, "maxAge", 60);
            let age = random_range(min_age, max_age);
            let now = Zoned::now();
            let birth = now
                .checked_sub(Span::new().days(age * 365 + random_range(0, 364)))
                .unwrap_or(now);
            let area_code = random_range(110000, 820000);
            let area = format!("{:06}", area_code);
            let date = birth.strftime("%Y%m%d").to_string();
            let seq = format!("{:03}", random_range(1, 999));
            let body = format!("{}{}{}", area, date, seq);
            Ok(format!("{}0", &body[..17]))
        }
        "gender" => Ok(random_choice(data.genders).to_string()),
        "age" => {
            let min = arg_i64(args, "min", 18);
            let max = arg_i64(args, "max", 80);
            Ok(random_range(min, max).to_string())
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "person".into(),
            method: method.into(),
        }),
    }
}
