//! HTML report export (with APDEX / latency distribution / timeline charts)

use std::collections::HashSet;

use orbit_metrics::{MetricSample, MetricsSummary};

use crate::context::ExportContext;
use crate::error::OutputResult;
use crate::format::ExportFormat;
use crate::traits::Exporter;

/// HTML exporter: renders the metrics summary as a self-contained HTML report
///
/// Without raw samples the APDEX and timeline charts are omitted; with samples it adds
/// the APDEX score, a latency distribution histogram, and throughput (RPS) / active VU timelines
/// (aligned with the information structure of the JMeter HTML Dashboard).
pub struct HtmlExporter;

impl Exporter for HtmlExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Html
    }

    fn export(&self, summary: &MetricsSummary, ctx: &ExportContext) -> OutputResult<String> {
        let samples = ctx.samples().filter(|s| !s.is_empty());
        let apdex = samples.map(apdex_html).unwrap_or_default();
        let histogram = samples.map(latency_histogram_svg).unwrap_or_default();
        let timeline = samples.map(timeline_svgs).unwrap_or_default();

        Ok(to_html_report(
            summary,
            ctx.test_name(),
            &apdex,
            &histogram,
            &timeline,
        ))
    }
}

/// Render the HTML body (template + sections)
fn to_html_report(
    summary: &MetricsSummary,
    test_name: &str,
    apdex: &str,
    histogram: &str,
    timeline: &str,
) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <title>Orbit Load Test Report: {name}</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; max-width: 800px; margin: 40px auto; padding: 20px; background: #f8f9fa; }}
        .card {{ background: white; border-radius: 8px; padding: 24px; margin-bottom: 16px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        h1 {{ color: #1a1a2e; margin-top: 0; }}
        .metric-grid {{ display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }}
        .metric {{ text-align: center; }}
        .metric-value {{ font-size: 28px; font-weight: bold; color: #2563eb; }}
        .metric-label {{ font-size: 12px; color: #6b7280; margin-top: 4px; }}
        .error {{ color: #ef4444; }}
        .apdex {{ display: flex; align-items: center; gap: 20px; flex-wrap: wrap; }}
        .apdex-score {{ font-size: 42px; font-weight: bold; color: #2563eb; }}
        .apdex-grade {{ font-size: 22px; font-weight: bold; padding: 4px 12px; border-radius: 6px; color: #fff; }}
        .bar-row {{ display: flex; align-items: center; gap: 12px; margin: 6px 0; }}
        .bar-label {{ width: 48px; color: #6b7280; font-size: 12px; }}
        .bar-track {{ flex: 1; height: 12px; background: #e5e7eb; border-radius: 6px; overflow: hidden; }}
        .bar-fill {{ height: 100%; background: linear-gradient(90deg, #60a5fa, #2563eb); border-radius: 6px; }}
        .bar-value {{ width: 90px; text-align: right; font-family: ui-monospace, monospace; font-size: 12px; color: #1a1a2e; }}
        .chart-grid {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }}
        .chart-title {{ font-size: 13px; color: #6b7280; margin-bottom: 6px; }}
        .chart-axis {{ display: flex; justify-content: space-between; font-size: 13px; font-weight: 600; color: #374151; margin-top: 4px; }}
        svg {{ max-width: 100%; }}
        table {{ width: 100%; border-collapse: collapse; }}
        th, td {{ padding: 8px 12px; text-align: left; border-bottom: 1px solid #e5e7eb; }}
        th {{ color: #6b7280; font-weight: 500; font-size: 12px; text-transform: uppercase; }}
        .pass {{ color: #22c55e; font-weight: bold; }}
        .fail {{ color: #ef4444; font-weight: bold; }}
    </style>
</head>
<body>
    <h1>🚀 Orbit Load Test Report</h1>
    <p>Test Plan: <strong>{name}</strong></p>
    <div class="card">
        <div class="metric-grid">
            <div class="metric">
                <div class="metric-value">{total}</div>
                <div class="metric-label">Total Requests</div>
            </div>
            <div class="metric">
                <div class="metric-value">{rps:.1}</div>
                <div class="metric-label">RPS</div>
            </div>
            <div class="metric">
                <div class="metric-value {error_class}">{error_rate:.2}%</div>
                <div class="metric-label">Error Rate</div>
            </div>
            <div class="metric">
                <div class="metric-value">{duration:.1}s</div>
                <div class="metric-label">Duration</div>
            </div>
        </div>
    </div>
    {apdex}
    <div class="card">
        <h2>Latency Distribution</h2>
        <div class="bars">{percentile_bars}</div>
        <table>
            <tr><th>Percentile</th><th>Latency (ms)</th></tr>
            <tr><td>Min</td><td>{min:.2}</td></tr>
            <tr><td>Mean</td><td>{mean:.2}</td></tr>
            <tr><td>p50</td><td>{p50:.2}</td></tr>
            <tr><td>p90</td><td>{p90:.2}</td></tr>
            <tr><td>p95</td><td>{p95:.2}</td></tr>
            <tr><td>p99</td><td>{p99:.2}</td></tr>
            <tr><td>p99.9</td><td>{p999:.2}</td></tr>
            <tr><td>Max</td><td>{max:.2}</td></tr>
        </table>
    </div>
    {histogram}
    {timeline}
    <p style="color: #9ca3af; font-size: 12px; text-align: center;">Generated by Orbit v0.1.0</p>
</body>
</html>"#,
        name = esc_html(test_name),
        apdex = apdex,
        percentile_bars = percentile_bars(summary),
        histogram = histogram,
        timeline = timeline,
        total = summary.total_requests,
        rps = summary.rps,
        error_class = if summary.error_rate > 0.0 {
            "error"
        } else {
            ""
        },
        error_rate = summary.error_rate * 100.0,
        duration = summary.duration.as_secs_f64(),
        min = summary.min_ms,
        mean = summary.mean_ms,
        p50 = summary.p50_ms,
        p90 = summary.p90_ms,
        p95 = summary.p95_ms,
        p99 = summary.p99_ms,
        p999 = summary.p999_ms,
        max = summary.max_ms,
    )
}

/// HTML escaping (for user input such as the report name)
fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Percentile bar chart (aligned with the JMeter dashboard response time distribution display)
fn percentile_bars(summary: &MetricsSummary) -> String {
    let rows = [
        ("Min", summary.min_ms),
        ("Mean", summary.mean_ms),
        ("P50", summary.p50_ms),
        ("P90", summary.p90_ms),
        ("P95", summary.p95_ms),
        ("P99", summary.p99_ms),
        ("P99.9", summary.p999_ms),
        ("Max", summary.max_ms),
    ];
    let max = rows.iter().map(|(_, v)| *v).fold(1.0, f64::max);
    rows.iter()
        .map(|(label, v)| {
            let w = ((v / max) * 100.0).max(1.0);
            format!(
                r#"<div class="bar-row"><span class="bar-label">{label}</span><div class="bar-track"><div class="bar-fill" style="width:{w:.1}%"></div></div><span class="bar-value">{v:.2}ms</span></div>"#
            )
        })
        .collect()
}

/// APDEX score card (T=500ms, aligned with the JMeter dashboard default satisfaction threshold)
fn apdex_html(samples: &[MetricSample]) -> String {
    const T_MS: f64 = 500.0;
    let mut satisfied = 0u64;
    let mut tolerated = 0u64;
    for s in samples {
        if s.duration_ms <= T_MS {
            satisfied += 1;
        } else if s.duration_ms <= T_MS * 4.0 {
            tolerated += 1;
        }
    }
    let score = (satisfied as f64 + tolerated as f64 * 0.5) / samples.len() as f64;
    let (grade, color) = match score {
        s if s >= 0.94 => ("A", "#22c55e"),
        s if s >= 0.85 => ("B", "#84cc16"),
        s if s >= 0.70 => ("C", "#f59e0b"),
        s if s >= 0.50 => ("D", "#f97316"),
        _ => ("F", "#ef4444"),
    };
    format!(
        r#"<div class="card"><h2>APDEX (T = {T:.0}ms)</h2><div class="apdex"><span class="apdex-score">{score:.3}</span><span class="apdex-grade" style="background:{color}">{grade}</span><span style="color:#6b7280;font-size:12px">Satisfied (≤{T:.0}ms): {satisfied} · Tolerated (≤{T4:.0}ms): {tolerated} · Samples: {total}</span></div></div>"#,
        T = T_MS,
        T4 = T_MS * 4.0,
        score = score,
        satisfied = satisfied,
        tolerated = tolerated,
        total = samples.len(),
    )
}

/// Latency distribution histogram (SVG, linear bins)
fn latency_histogram_svg(samples: &[MetricSample]) -> String {
    let max_ms = samples
        .iter()
        .map(|s| s.duration_ms)
        .fold(0.0, f64::max)
        .max(1.0);
    let bin = nice_bin(max_ms / 16.0);
    let bins = ((max_ms / bin).ceil() as usize).max(1);
    let mut counts = vec![0u64; bins];
    for s in samples {
        let idx = ((s.duration_ms / bin) as usize).min(bins - 1);
        counts[idx] += 1;
    }
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1);
    let w = 720.0;
    let h = 150.0;
    let plot_h = h;
    let bw = w / bins as f64;
    let bars: String = counts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let bh = plot_h * (*c as f64 / max_count as f64);
            format!(
                r##"<rect x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" fill="#60a5fa"><title>{lo}–{hi}ms: {c}</title></rect>"##,
                x = i as f64 * bw + 1.0,
                y = plot_h - bh,
                bw = (bw - 2.0).max(0.5),
                bh = bh.max(0.5),
                lo = (i as f64 * bin) as u64,
                hi = ((i + 1) as f64 * bin) as u64,
                c = c,
            )
        })
        .collect();
    format!(
        r##"<div class="card"><h2>Response Time Distribution</h2><svg viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg">{bars}</svg><div class="chart-axis"><span>0ms</span><span>{max:.0}ms</span></div></div>"##,
        w = w,
        h = h,
        max = max_ms,
    )
}

/// Throughput (RPS) and active VU timelines (SVG polylines, bucketed by time)
fn timeline_svgs(samples: &[MetricSample]) -> String {
    let t0 = samples.iter().map(|s| s.timestamp).min().unwrap_or(0) as f64;
    let t1 = samples.iter().map(|s| s.timestamp).max().unwrap_or(0) as f64;
    let span = (t1 - t0).max(1.0);
    // Aim for about 1s per bucket, clamped to 20..120 buckets
    let n = ((span / 1000.0).round() as usize).clamp(20, 120);
    let bucket_ms = span / n as f64;
    let mut counts = vec![0u64; n];
    let mut vus: Vec<HashSet<u64>> = (0..n).map(|_| HashSet::new()).collect();
    for s in samples {
        let idx = (((s.timestamp as f64 - t0) / bucket_ms) as usize).min(n - 1);
        counts[idx] += 1;
        vus[idx].insert(s.vu_id);
    }
    let rps: Vec<f64> = counts
        .iter()
        .map(|c| *c as f64 / (bucket_ms / 1000.0))
        .collect();
    let vu_series: Vec<f64> = vus.iter().map(|v| v.len() as f64).collect();
    let rps_svg = line_chart_svg(&rps, "Throughput (RPS)", "#34d399");
    let vus_svg = line_chart_svg(&vu_series, "Active VUs", "#38bdf8");
    format!(
        r#"<div class="card"><h2>Over Time</h2><div class="chart-grid">{rps_svg}{vus_svg}</div></div>"#
    )
}

/// A single line chart SVG
fn line_chart_svg(series: &[f64], title: &str, color: &str) -> String {
    let w = 720.0;
    let h = 150.0;
    let plot_h = h;
    let ymax = series.iter().cloned().fold(0.0, f64::max).max(0.001);
    let n = series.len().max(2) as f64;
    let mut path = String::new();
    for (i, v) in series.iter().enumerate() {
        let x = if series.len() == 1 {
            0.0
        } else {
            i as f64 / (n - 1.0) * w
        };
        let y = plot_h - (v / ymax) * plot_h;
        if i == 0 {
            path.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            path.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }
    format!(
        r##"<div><div class="chart-title">{title}</div><svg viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg"><polyline points="{path}" fill="none" stroke="{color}" stroke-width="2"/></svg><div class="chart-axis"><span>0</span><span>{ymax:.1}</span></div></div>"##,
    )
}

/// Pick a "nice" bin width (1/2/5 x 10^n)
fn nice_bin(v: f64) -> f64 {
    let v = v.max(1.0);
    let mag = 10f64.powf(v.log10().floor());
    let n = v / mag;
    let step = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * mag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sample, sample_summary};

    #[test]
    fn test_to_html() {
        let html = HtmlExporter
            .export(
                &sample_summary(),
                &ExportContext::new().with_test_name("test_plan"),
            )
            .unwrap();
        assert!(html.contains("test_plan"));
        assert!(html.contains("1000"));
        assert!(html.contains("45.00"));
    }

    #[test]
    fn test_to_html_escapes_name() {
        let html = HtmlExporter
            .export(
                &sample_summary(),
                &ExportContext::new().with_test_name("a<b&c\"d"),
            )
            .unwrap();
        assert!(html.contains("a&lt;b&amp;c&quot;d"));
    }

    #[test]
    fn test_to_html_report_with_samples() {
        let html = HtmlExporter
            .export(
                &sample_summary(),
                &ExportContext::new()
                    .with_test_name("test_plan")
                    .with_samples(&[sample()]),
            )
            .unwrap();
        assert!(html.contains("APDEX"));
        assert!(html.contains("Response Time Distribution"));
        assert!(html.contains("Over Time"));
        assert!(html.contains("<svg"));
        assert!(html.contains("chart-axis"));
    }
}
