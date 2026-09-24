//! Export format definitions

/// Supported export formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    /// Summary JSON (serialized MetricsSummary)
    Json,
    /// Summary CSV (single row)
    Csv,
    /// JUnit XML report
    Junit,
    /// HTML report (adds APDEX/charts when raw samples are provided)
    Html,
    /// JMeter-style JTL raw sample CSV
    Jtl,
    /// k6-style raw sample JSONL
    RawJsonl,
}

impl ExportFormat {
    /// Parse from a format name (matches the format argument passed by CLI/Tauri)
    pub fn parse(s: &str) -> Option<ExportFormat> {
        match s {
            "json" => Some(ExportFormat::Json),
            "csv" => Some(ExportFormat::Csv),
            "junit" => Some(ExportFormat::Junit),
            "html" => Some(ExportFormat::Html),
            "jtl" => Some(ExportFormat::Jtl),
            "raw-json" | "jsonl" => Some(ExportFormat::RawJsonl),
            _ => None,
        }
    }

    /// Canonical format name
    pub fn as_str(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Junit => "junit",
            ExportFormat::Html => "html",
            ExportFormat::Jtl => "jtl",
            ExportFormat::RawJsonl => "raw-json",
        }
    }

    /// File extension
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Junit => "xml",
            ExportFormat::Html => "html",
            ExportFormat::Jtl => "jtl",
            ExportFormat::RawJsonl => "raw.jsonl",
        }
    }

    /// Default export file name (e.g. `orbit_report.html`)
    pub fn file_name(self, stem: &str) -> String {
        format!("{}.{}", stem, self.extension())
    }

    /// Whether raw sample collection must be enabled (sample-level formats)
    pub fn needs_samples(self) -> bool {
        matches!(
            self,
            ExportFormat::Html | ExportFormat::Jtl | ExportFormat::RawJsonl
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(ExportFormat::parse("json"), Some(ExportFormat::Json));
        assert_eq!(
            ExportFormat::parse("raw-json"),
            Some(ExportFormat::RawJsonl)
        );
        assert_eq!(ExportFormat::parse("jsonl"), Some(ExportFormat::RawJsonl));
        assert_eq!(ExportFormat::parse("nope"), None);
    }

    #[test]
    fn test_file_name() {
        assert_eq!(
            ExportFormat::Html.file_name("orbit_report"),
            "orbit_report.html"
        );
        assert_eq!(ExportFormat::Junit.file_name("r"), "r.xml");
        assert_eq!(
            ExportFormat::RawJsonl.file_name("orbit_report"),
            "orbit_report.raw.jsonl"
        );
    }

    #[test]
    fn test_needs_samples() {
        assert!(ExportFormat::Jtl.needs_samples());
        assert!(ExportFormat::RawJsonl.needs_samples());
        assert!(ExportFormat::Html.needs_samples());
        assert!(!ExportFormat::Json.needs_samples());
    }
}
