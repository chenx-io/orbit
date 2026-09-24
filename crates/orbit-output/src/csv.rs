//! Summary CSV export

use orbit_metrics::MetricsSummary;

use crate::context::ExportContext;
use crate::error::OutputResult;
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// CSV exporter: renders the metrics summary as a single CSV row (with header)
pub struct CsvExporter;

impl Exporter for CsvExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Csv
    }

    fn export(&self, summary: &MetricsSummary, _ctx: &ExportContext) -> OutputResult<String> {
        Ok(format!(
            "total_requests,total_errors,error_rate,rps,duration_ms,p50_ms,p90_ms,p95_ms,p99_ms,p999_ms,min_ms,max_ms,mean_ms,total_bytes\n\
             {},{},{:.4},{:.1},{:.0},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{}\n",
            summary.total_requests,
            summary.total_errors,
            summary.error_rate,
            summary.rps,
            summary.duration.as_millis(),
            summary.p50_ms,
            summary.p90_ms,
            summary.p95_ms,
            summary.p99_ms,
            summary.p999_ms,
            summary.min_ms,
            summary.max_ms,
            summary.mean_ms,
            summary.total_bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sample_summary;

    #[test]
    fn test_to_csv() {
        let csv = CsvExporter
            .export(&sample_summary(), &ExportContext::new())
            .unwrap();
        assert!(csv.contains("1000"));
        assert!(csv.contains("45.00"));
    }
}
