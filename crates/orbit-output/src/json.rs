//! JSON summary export

use orbit_metrics::MetricsSummary;

use crate::context::ExportContext;
use crate::error::{OutputError, OutputResult};
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// JSON exporter: serializes `MetricsSummary` into pretty JSON
pub struct JsonExporter;

impl Exporter for JsonExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Json
    }

    fn export(&self, summary: &MetricsSummary, _ctx: &ExportContext) -> OutputResult<String> {
        serde_json::to_string_pretty(summary).map_err(|e| OutputError::Export(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sample_summary;

    #[test]
    fn test_to_json() {
        let json = JsonExporter
            .export(&sample_summary(), &ExportContext::new())
            .unwrap();
        assert!(json.contains("total_requests"));
        assert!(json.contains("1000"));
    }
}
