//! Exporter trait - a unified abstraction over all output formats

use orbit_metrics::MetricsSummary;

use crate::context::ExportContext;
use crate::error::OutputResult;
use crate::format::ExportFormat;

/// Report exporter: renders a metrics summary (plus optional raw samples) as text in a given format
///
/// Exporters are stateless and safe to share. To add an output format, implement this trait and
/// register it in [`crate::builtin`] to hook into the unified entry point [`crate::export`].
pub trait Exporter: Send + Sync {
    /// Format produced by this exporter
    fn format(&self) -> ExportFormat;

    /// Render into text content
    fn export(&self, summary: &MetricsSummary, ctx: &ExportContext) -> OutputResult<String>;
}
