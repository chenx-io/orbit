//! k6-style raw sample JSONL export

use orbit_metrics::{MetricSample, MetricsSummary};

use crate::context::ExportContext;
use crate::error::{OutputError, OutputResult};
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// JSONL exporter: one JSON line per request with all fields and timing info,
/// making it easy to recompute percentiles, aggregate by phase, or diff against a baseline.
pub struct RawJsonlExporter;

impl Exporter for RawJsonlExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::RawJsonl
    }

    fn export(&self, _summary: &MetricsSummary, ctx: &ExportContext) -> OutputResult<String> {
        let samples = ctx.samples().unwrap_or_default();
        let mut out = String::new();
        for s in samples {
            out.push_str(
                &serde_json::to_string(&sample_to_json(s))
                    .map_err(|e| OutputError::Export(e.to_string()))?,
            );
            out.push('\n');
        }
        Ok(out)
    }
}

/// Sample → JSON object (millisecond fields keep 3 decimals)
fn sample_to_json(s: &MetricSample) -> serde_json::Value {
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;
    serde_json::json!({
        "timestamp_ms": s.timestamp,
        "scenario": s.scenario,
        "step": s.step,
        "vu_id": s.vu_id,
        "iteration": s.iteration,
        "status": s.status,
        "duration_ms": round3(s.duration_ms),
        "body_size": s.body_size,
        "message_count": s.message_count,
        "is_error": s.is_error,
        "error_msg": s.error_msg,
        "error_type": s.error_type,
        "timing": {
            "dns_ms": round3(s.dns_ms),
            "tcp_ms": round3(s.tcp_ms),
            "tls_ms": round3(s.tls_ms),
            "send_ms": round3(s.send_ms),
            "ttfb_ms": round3(s.ttfb_ms),
            "download_ms": round3(s.download_ms),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sample, sample_summary};

    #[test]
    fn test_to_raw_jsonl() {
        let jsonl = RawJsonlExporter
            .export(
                &sample_summary(),
                &ExportContext::new().with_samples(&[sample()]),
            )
            .unwrap();
        assert!(jsonl.trim_end().ends_with('}'));
        assert!(jsonl.contains("\"timestamp_ms\":1700000000000"));
        assert!(jsonl.contains("\"scenario\":\"sc1\""));
        assert!(jsonl.contains("\"ttfb_ms\":8.0"));
    }
}
