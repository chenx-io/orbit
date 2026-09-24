//! # orbit-output
//!
//! Output exporters. Unified as the [`Exporter`] trait + the [`ExportFormat`] enum:
//! - Summary level: JSON, CSV, JUnit XML, HTML report
//! - Raw sample level: JTL CSV (aligned with JMeter), JSONL (aligned with k6 JSON export)
//!
//! [`export`] is the single entry point that dispatches by format and returns content plus default file name;
//! to add a format, implement [`Exporter`] and register it in [`builtin`]; callers need no changes.

mod context;
mod csv;
mod error;
mod format;
mod html;
mod json;
mod jtl;
mod junit;
mod raw_jsonl;
mod traits;

pub use context::ExportContext;
pub use error::{OutputError, OutputResult};
pub use format::ExportFormat;
pub use traits::Exporter;

use orbit_metrics::MetricsSummary;

/// An exported report: rendered content + format + default file name
#[derive(Debug)]
pub struct ExportedReport {
    /// Export format
    pub format: ExportFormat,
    /// Rendered text content
    pub content: String,
    /// Default file name (e.g. `orbit_report.html`)
    pub filename: String,
}

/// Built-in exporter table (looked up by format)
pub fn builtin(format: ExportFormat) -> &'static dyn Exporter {
    match format {
        ExportFormat::Json => &json::JsonExporter,
        ExportFormat::Csv => &csv::CsvExporter,
        ExportFormat::Junit => &junit::JunitExporter,
        ExportFormat::Html => &html::HtmlExporter,
        ExportFormat::Jtl => &jtl::JtlExporter,
        ExportFormat::RawJsonl => &raw_jsonl::RawJsonlExporter,
    }
}

/// Export a report by format (single entry point)
///
/// The file name defaults to `orbit_report.<ext>`; for a custom prefix use
/// [`ExportFormat::file_name`] yourself.
pub fn export(
    format: ExportFormat,
    summary: &MetricsSummary,
    ctx: &ExportContext,
) -> OutputResult<ExportedReport> {
    let content = builtin(format).export(summary, ctx)?;
    Ok(ExportedReport {
        format,
        content,
        filename: format.file_name("orbit_report"),
    })
}

/// Shared test fixture builders (test builds only)
#[cfg(test)]
pub(crate) mod test_util {
    use std::time::Duration;

    use orbit_metrics::{MetricSample, MetricsSummary};

    /// Build a raw sample
    pub fn sample() -> MetricSample {
        MetricSample {
            scenario: "sc1".into(),
            step: "step_a".into(),
            vu_id: 2,
            iteration: 7,
            status: 200,
            duration_ms: 12.5,
            body_size: 1024,
            message_count: 0,
            is_error: false,
            error_msg: None,
            error_type: String::new(),
            timestamp: 1_700_000_000_000,
            dns_ms: 1.0,
            tcp_ms: 2.0,
            tls_ms: 3.0,
            send_ms: 0.5,
            ttfb_ms: 8.0,
            download_ms: 4.0,
        }
    }

    /// Build a metrics summary
    pub fn sample_summary() -> MetricsSummary {
        MetricsSummary {
            total_requests: 1000,
            total_errors: 5,
            error_rate: 0.005,
            rps: 100.0,
            duration: Duration::from_secs(10),
            p50_ms: 45.0,
            p90_ms: 98.0,
            p95_ms: 127.0,
            p99_ms: 210.0,
            p999_ms: 350.0,
            min_ms: 12.0,
            max_ms: 450.0,
            mean_ms: 52.3,
            total_bytes: 1024000,
            total_messages: 0,
            avg_dns_ms: 1.2,
            avg_tcp_ms: 3.5,
            avg_tls_ms: 8.1,
            avg_send_ms: 0.3,
            avg_ttfb_ms: 35.0,
            avg_download_ms: 4.2,
            timed_count: 995,
            error_breakdown: vec![],
        }
    }
}
