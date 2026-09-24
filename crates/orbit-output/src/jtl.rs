//! JMeter-style JTL raw sample export

use std::fmt::Write;

use orbit_metrics::MetricsSummary;

use crate::context::ExportContext;
use crate::error::OutputResult;
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// JTL exporter: renders raw samples as JMeter-style CSV
///
/// The first 17 columns align with standard JMeter JTL fields, followed by Orbit-specific columns
/// (vu/iteration/error_type/per-phase timing), for Grafana-like tools or scripts
/// to recompute metrics by time range/phase.
pub struct JtlExporter;

impl Exporter for JtlExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Jtl
    }

    fn export(&self, _summary: &MetricsSummary, ctx: &ExportContext) -> OutputResult<String> {
        let samples = ctx.samples().unwrap_or_default();
        let mut out = String::from(
            "timeStamp,elapsed,label,responseCode,responseMessage,threadName,dataType,success,failureMessage,bytes,sentBytes,grpThreads,allThreads,URL,Latency,IdleTime,Connect,vuId,iteration,errorType,dnsMs,tlsMs,sendMs,downloadMs\n",
        );
        for s in samples {
            let label = if s.step.is_empty() {
                s.scenario.clone()
            } else {
                format!("{}/{}", s.scenario, s.step)
            };
            let msg = s.error_msg.as_deref().unwrap_or("");
            #[allow(clippy::format_in_format_args)]
            let _ = writeln!(
                out,
                "{},{:.3},{},{},{},{},,{},{},{},{},{},{},,{:.3},{},{:.3},{},{},{},{:.3},{:.3},{:.3},{:.3}",
                s.timestamp,
                s.duration_ms,
                csv_escape(&label),
                s.status,
                csv_escape(msg),
                format!("vu-{}", s.vu_id),
                if s.is_error { "false" } else { "true" },
                csv_escape(msg),
                s.body_size,
                0,
                0,
                0,
                s.ttfb_ms,
                0,
                s.tcp_ms,
                s.vu_id,
                s.iteration,
                csv_escape(&s.error_type),
                s.dns_ms,
                s.tls_ms,
                s.send_ms,
                s.download_ms,
            );
        }
        Ok(out)
    }
}

/// CSV field escaping (comma/quote/newline)
fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sample, sample_summary};

    #[test]
    fn test_to_jtl_csv() {
        let csv = JtlExporter
            .export(
                &sample_summary(),
                &ExportContext::new().with_samples(&[sample()]),
            )
            .unwrap();
        assert!(csv.starts_with("timeStamp,elapsed,label"));
        assert!(csv.contains("sc1/step_a"));
        assert!(csv.contains("1700000000000,12.500"));
        assert!(csv.contains(",vu-2,"));
    }
}
