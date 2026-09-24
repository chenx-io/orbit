//! CSS selector assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// CSS selector assertion: checks that the given selector exists in / matches the HTML body
pub struct CssSelectorAssertion {
    pub selector: String,
    pub comparator: Comparator,
    pub expected: String,
}

#[async_trait]
impl Assertion for CssSelectorAssertion {
    fn name(&self) -> &str {
        "css_selector"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        // MVP CSS selector: use scraper crate for HTML parsing
        let document = scraper::Html::parse_document(&body_str);
        let selector = match scraper::Selector::parse(&self.selector) {
            Ok(s) => s,
            Err(e) => {
                return AssertionResult {
                    name: "css_selector".into(),
                    passed: false,
                    message: format!("Invalid CSS selector: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        let mut elements = document.select(&selector);
        let found = elements.next().is_some();

        let passed = match &self.comparator {
            Comparator::Exists => found,
            Comparator::Equal => {
                elements = document.select(&selector);
                elements
                    .next()
                    .map(|el| el.text().collect::<String>().trim() == self.expected)
                    .unwrap_or(false)
            }
            Comparator::Contains => {
                elements = document.select(&selector);
                elements
                    .next()
                    .map(|el| el.text().collect::<String>().contains(&self.expected))
                    .unwrap_or(false)
            }
            _ => found,
        };

        AssertionResult {
            name: "css_selector".into(),
            passed,
            message: format!(
                "CSS '{}': {}",
                self.selector,
                if passed { "matched" } else { "no match" }
            ),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}
