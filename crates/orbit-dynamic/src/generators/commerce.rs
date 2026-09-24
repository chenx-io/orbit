//! Commerce / company / finance generator (multi-language data comes from `data::dataset`)

use crate::data::{dataset, en};
use crate::locale::Locale;

use super::{
    arg_f64, arg_i64, random_choice, random_float_range, random_range, random_string,
    resolve_locale, Args,
};
use crate::DynamicError;

// ─── commerce ──────────────────────────────────────────

pub fn gen_commerce(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "price" => {
            let price = random_float_range(9.9, 9999.0);
            Ok(format!("{:.2}", price))
        }
        "productName" => Ok(random_choice(data.products).to_string()),
        "productNameEn" => Ok(random_choice(en::DATASET.products).to_string()),
        "department" => Ok(random_choice(data.departments).to_string()),
        "productDescription" => Ok(match locale {
            Locale::Zh => format!("高品质{}，性价比之选", random_choice(data.products)),
            Locale::En => format!(
                "High-quality {}, great value for money",
                random_choice(data.products)
            ),
            Locale::Ja => format!("高品質な{}、コスパ抜群", random_choice(data.products)),
        }),
        "sku" => {
            let prefix = random_choice(&["SKU", "SP", "P"]);
            Ok(format!("{}-{}", prefix, random_range(10000, 99999)))
        }
        _ => Err(DynamicError::UnknownMethod {
            category: "commerce".into(),
            method: method.into(),
        }),
    }
}

// ─── company ───────────────────────────────────────────

pub fn gen_company(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "name" => match locale {
            Locale::Zh => Ok(format!(
                "{}{}有限公司",
                random_choice(data.company_prefix),
                random_choice(data.company_suffix)
            )),
            Locale::En => Ok(format!(
                "{} {}",
                random_choice(data.company_prefix),
                random_choice(data.company_suffix)
            )),
            Locale::Ja => Ok(format!(
                "{}{}",
                random_choice(data.company_prefix),
                random_choice(data.company_suffix)
            )),
        },
        "catchPhrase" => Ok(random_choice(data.company_catch).to_string()),
        "bs" => Ok(random_choice(data.company_bs).to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "company".into(),
            method: method.into(),
        }),
    }
}

// ─── finance ───────────────────────────────────────────

pub fn gen_finance(method: &str, args: &Args) -> Result<String, DynamicError> {
    let locale = resolve_locale(args);
    let data = dataset(locale);
    match method {
        "accountNumber" => {
            let len = arg_i64(args, "length", 16) as usize;
            Ok(random_string(len, b"0123456789"))
        }
        "amount" => {
            let min = arg_f64(args, "min", 1.0);
            let max = arg_f64(args, "max", 99999.0);
            Ok(format!("{:.2}", random_float_range(min, max)))
        }
        "currencyCode" => {
            Ok(random_choice(&["CNY", "USD", "EUR", "JPY", "GBP", "HKD"]).to_string())
        }
        "currencyName" => Ok(random_choice(data.currencies).to_string()),
        "creditCardCVV" => Ok(format!("{:03}", random_range(1, 999))),
        "transactionType" => Ok(random_choice(data.tx_types).to_string()),
        _ => Err(DynamicError::UnknownMethod {
            category: "finance".into(),
            method: method.into(),
        }),
    }
}
