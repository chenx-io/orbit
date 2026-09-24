//! Geographic location generator (city / state / street / address, multi-language; concatenation format exhaustively matched by locale)

use crate::data::dataset;
use crate::locale::Locale;

use super::{random_choice, random_float_range, random_range, resolve_locale, Args};
use crate::DynamicError;

pub fn gen_location(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "city" => Ok(random_choice(data.cities).to_string()),
        "state" => Ok(random_choice(data.states).to_string()),
        "street" | "streetAddress" => {
            let num = random_range(1, 999);
            let street = random_choice(data.streets);
            Ok(match locale {
                Locale::Zh => format!("{}{}号", street, num),
                Locale::En => format!("{} {}", num, street),
                Locale::Ja => format!("{}{}番", street, num),
            })
        }
        "county" => {
            let word = random_choice(data.counties);
            Ok(match locale {
                Locale::En => format!("{} County", word),
                Locale::Zh => format!("{}区", word),
                Locale::Ja => format!("{}区", word),
            })
        }
        "secondaryAddress" => Ok(match locale {
            Locale::Zh => format!("{}单元{}室", random_range(1, 10), random_range(101, 999)),
            Locale::En => format!("Apt {}", random_range(100, 999)),
            Locale::Ja => format!("{}号室", random_range(101, 999)),
        }),
        "address" => {
            let num = random_range(1, 999);
            match locale {
                Locale::Zh => Ok(format!(
                    "{}{}{}{}号",
                    random_choice(data.states),
                    random_choice(data.cities),
                    random_choice(data.streets),
                    num
                )),
                Locale::En => Ok(format!(
                    "{} {}, {}, {}",
                    num,
                    random_choice(data.streets),
                    random_choice(data.cities),
                    random_choice(data.states)
                )),
                Locale::Ja => Ok(format!(
                    "{}{}{}{}番",
                    random_choice(data.states),
                    random_choice(data.cities),
                    random_choice(data.streets),
                    num
                )),
            }
        }
        "zipCode" => Ok(format!("{:06}", random_range(100000, 999999))),
        "country" => Ok(random_choice(data.countries).to_string()),
        "latitude" => Ok(format!("{:.6}", random_float_range(18.0, 54.0))),
        "longitude" => Ok(format!("{:.6}", random_float_range(73.0, 135.0))),
        _ => Err(DynamicError::UnknownMethod {
            category: "location".into(),
            method: method.into(),
        }),
    }
}
