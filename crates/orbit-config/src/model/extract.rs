//! Variable extraction model

use serde::{Deserialize, Serialize};

/// Variable extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extraction {
    pub name: String,
    #[serde(flatten)]
    pub source: ExtractionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ExtractionSource {
    #[serde(rename = "jsonpath")]
    JsonPath { path: String },
    #[serde(rename = "jmespath")]
    JmesPath { expression: String },
    #[serde(rename = "header")]
    Header { name: String },
    #[serde(rename = "regex")]
    Regex {
        pattern: String,
        #[serde(default = "default_regex_group")]
        group: usize,
    },
    #[serde(rename = "cookie")]
    Cookie {
        name: String,
        #[serde(default)]
        attr: Option<String>,
    },
}

fn default_regex_group() -> usize {
    1
}
