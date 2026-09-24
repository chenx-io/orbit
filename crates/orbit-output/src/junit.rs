//! JUnit XML report export

use orbit_metrics::MetricsSummary;

use crate::context::ExportContext;
use crate::error::OutputResult;
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// JUnit XML exporter: renders the metrics summary as a JUnit testsuite
pub struct JunitExporter;

impl Exporter for JunitExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Junit
    }

    fn export(&self, summary: &MetricsSummary, ctx: &ExportContext) -> OutputResult<String> {
        let test_name = ctx.test_name();
        let failures = if summary.error_rate > 0.0 {
            format!(
                r#"    <testcase name="error_rate" classname="orbit.load">
      <failure message="Error rate {:.2}% exceeds threshold" />
    </testcase>"#,
                summary.error_rate * 100.0
            )
        } else {
            String::new()
        };

        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="{name}" tests="1" failures="{failures_count}" errors="0" time="{time:.3}">
  <testcase name="load_test" classname="orbit.load" time="{time:.3}">
    <properties>
      <property name="total_requests" value="{total}" />
      <property name="rps" value="{rps:.1}" />
      <property name="p50_ms" value="{p50:.2}" />
      <property name="p95_ms" value="{p95:.2}" />
      <property name="p99_ms" value="{p99:.2}" />
      <property name="error_rate" value="{error_rate:.4}" />
    </properties>
  </testcase>
  {failures}
</testsuite>"#,
            name = test_name,
            failures_count = if summary.error_rate > 0.0 { 1 } else { 0 },
            time = summary.duration.as_secs_f64(),
            total = summary.total_requests,
            rps = summary.rps,
            p50 = summary.p50_ms,
            p95 = summary.p95_ms,
            p99 = summary.p99_ms,
            error_rate = summary.error_rate,
            failures = failures,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sample_summary;

    #[test]
    fn test_to_junit() {
        let xml = JunitExporter
            .export(
                &sample_summary(),
                &ExportContext::new().with_test_name("test_plan"),
            )
            .unwrap();
        assert!(xml.contains("test_plan"));
        assert!(xml.contains("1000"));
        assert!(xml.contains("testsuite"));
    }
}
