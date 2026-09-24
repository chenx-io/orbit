//! # orbit-metrics
//!
//! HDR Histogram metrics engine. Provides:
//! - MetricSample: metrics sample for a single request
//! - MetricsAggregator: HDR histogram aggregator
//! - MetricsBus: metrics channel (single-node mpsc)
//! - MetricsSink trait: reserved interface for distributed use

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use hdrhistogram::serialization::{Deserializer, Serializer, V2Serializer};
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Raw sample collection cap: stop collecting and flag truncation once exceeded (avoids memory blow-up on very long load tests).
/// 2_000_000 entries ~= a few hundred MB ceiling, enough for the vast majority of analysis scenarios.
pub const MAX_RAW_SAMPLES: usize = 2_000_000;

/// Metrics sample for a single request
#[derive(Debug, Clone)]
pub struct MetricSample {
    /// Scenario name
    pub scenario: String,
    /// Step name
    pub step: String,
    /// VU ID
    pub vu_id: u64,
    /// Iteration index
    pub iteration: u64,
    /// Status code
    pub status: i32,
    /// Response duration (ms)
    pub duration_ms: f64,
    /// Response body size (bytes)
    pub body_size: usize,
    /// Number of messages received for streaming protocols (0 for non-streaming requests)
    pub message_count: u64,
    /// Whether the request failed
    pub is_error: bool,
    /// Error message
    pub error_msg: Option<String>,
    /// Error classification key (used to group stats by type: connect/dns/tls/timeout/send/receive/
    /// protocol/codec/server_error/assertion/post_script/internal/other）
    pub error_type: String,
    /// Timestamp
    pub timestamp: u64,
    /// DNS resolution time (ms)
    pub dns_ms: f64,
    /// TCP connect time (ms)
    pub tcp_ms: f64,
    /// TLS handshake time (ms)
    pub tls_ms: f64,
    /// Request send time (ms)
    pub send_ms: f64,
    /// Time to first byte (TTFB, ms)
    pub ttfb_ms: f64,
    /// Response download time (ms)
    pub download_ms: f64,
}

/// Grouped statistics for a single error type
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorGroup {
    /// Error classification key (connect/dns/tls/timeout/send/receive/protocol/codec/
    /// server_error/assertion/post_script/internal/other)
    pub error_type: String,
    /// Number of occurrences of this error type
    pub count: u64,
    /// One representative error message (shown on hover in load test reports)
    pub sample: String,
}

/// Metrics aggregator
pub struct MetricsAggregator {
    /// Latency histogram (HDR)
    histogram: Histogram<u64>,
    /// Total request count
    total_requests: u64,
    /// Total error count
    total_errors: u64,
    /// Total response body size
    total_bytes: u64,
    /// Total streaming message count
    total_messages: u64,
    /// Minimum latency
    min_duration_ms: f64,
    /// Maximum latency
    max_duration_ms: f64,
    /// Total duration
    total_duration_ms: f64,
    /// Errors grouped by type: classification key -> (count, representative message)
    error_breakdown: HashMap<String, (u64, String)>,
    /// Start time
    start_time: Option<std::time::Instant>,
    /// End time
    end_time: Option<std::time::Instant>,
    // ── Per-phase timing accumulators ──
    total_dns_ms: f64,
    total_tcp_ms: f64,
    total_tls_ms: f64,
    total_send_ms: f64,
    total_ttfb_ms: f64,
    total_download_ms: f64,
    /// Number of requests with per-phase data (some protocols / failed requests may have no timing)
    timed_count: u64,
    /// Raw sample buffer (non-empty only when raw export is enabled)
    raw_samples: Option<Vec<MetricSample>>,
    /// Whether raw samples were truncated at the cap
    raw_truncated: bool,
    /// Raw sample collection cap (guards against memory blow-up; fixed once enabled)
    raw_cap: usize,
}

impl MetricsAggregator {
    /// Create a new aggregator (latency range 1us ~ 1h, 3 significant digits)
    pub fn new() -> Self {
        Self {
            histogram: Histogram::new(3).expect("Failed to create HDR histogram"),
            total_requests: 0,
            total_errors: 0,
            total_bytes: 0,
            total_messages: 0,
            min_duration_ms: f64::MAX,
            max_duration_ms: 0.0,
            total_duration_ms: 0.0,
            error_breakdown: HashMap::new(),
            start_time: None,
            end_time: None,
            total_dns_ms: 0.0,
            total_tcp_ms: 0.0,
            total_tls_ms: 0.0,
            total_send_ms: 0.0,
            total_ttfb_ms: 0.0,
            total_download_ms: 0.0,
            timed_count: 0,
            raw_samples: None,
            raw_truncated: false,
            raw_cap: MAX_RAW_SAMPLES,
        }
    }

    /// Enable raw sample collection (for JTL / raw JSON export)
    pub fn enable_raw_samples(&mut self) {
        self.enable_raw_samples_with_cap(MAX_RAW_SAMPLES);
    }

    /// Enable raw sample collection with an explicit cap (use a smaller cap to bound memory when the desktop app keeps it resident)
    pub fn enable_raw_samples_with_cap(&mut self, cap: usize) {
        self.raw_samples = Some(Vec::with_capacity(cap.min(65_536)));
        self.raw_truncated = false;
        self.raw_cap = cap.max(1);
    }

    /// Whether raw sample collection is enabled
    pub fn raw_enabled(&self) -> bool {
        self.raw_samples.is_some()
    }

    /// Collected raw samples (cloned, discarded after export)
    pub fn raw_samples(&self) -> Vec<MetricSample> {
        self.raw_samples.clone().unwrap_or_default()
    }

    /// Whether raw samples were truncated at the cap
    pub fn raw_truncated(&self) -> bool {
        self.raw_truncated
    }

    /// Record one sample
    pub fn record(&mut self, sample: &MetricSample) {
        if self.start_time.is_none() {
            self.start_time = Some(std::time::Instant::now());
        }

        let duration_us = (sample.duration_ms * 1000.0) as u64;
        if duration_us > 0 {
            self.histogram.record(duration_us).ok();
        }

        self.total_requests += 1;
        if sample.is_error {
            self.total_errors += 1;
            // Group by error type: keep one representative error message
            let key = if sample.error_type.is_empty() {
                "other"
            } else {
                sample.error_type.as_str()
            };
            let entry = self
                .error_breakdown
                .entry(key.to_string())
                .or_insert((0u64, String::new()));
            entry.0 += 1;
            if entry.1.is_empty() {
                entry.1 = sample
                    .error_msg
                    .clone()
                    .unwrap_or_else(|| "(no error message)".to_string());
            }
        }
        self.total_bytes += sample.body_size as u64;
        self.total_messages += sample.message_count;
        self.total_duration_ms += sample.duration_ms;

        if sample.duration_ms < self.min_duration_ms {
            self.min_duration_ms = sample.duration_ms;
        }
        if sample.duration_ms > self.max_duration_ms {
            self.max_duration_ms = sample.duration_ms;
        }

        // Accumulate per-phase timings (only when data exists, so the 0 values of failed requests don't drag the average down)
        let has_timing = sample.dns_ms > 0.0
            || sample.tcp_ms > 0.0
            || sample.tls_ms > 0.0
            || sample.ttfb_ms > 0.0;
        if has_timing {
            self.total_dns_ms += sample.dns_ms;
            self.total_tcp_ms += sample.tcp_ms;
            self.total_tls_ms += sample.tls_ms;
            self.total_send_ms += sample.send_ms;
            self.total_ttfb_ms += sample.ttfb_ms;
            self.total_download_ms += sample.download_ms;
            self.timed_count += 1;
        }

        self.end_time = Some(std::time::Instant::now());

        if let Some(buf) = &mut self.raw_samples {
            if buf.len() < self.raw_cap {
                buf.push(sample.clone());
            } else {
                self.raw_truncated = true;
            }
        }
    }

    /// Get the summary metrics
    pub fn summary(&self) -> MetricsSummary {
        let elapsed = self
            .start_time
            .and_then(|s| self.end_time.map(|e| e.duration_since(s)))
            .unwrap_or(Duration::from_secs(1));

        let rps = self.total_requests as f64 / elapsed.as_secs_f64().max(0.001);
        let error_rate = if self.total_requests > 0 {
            self.total_errors as f64 / self.total_requests as f64
        } else {
            0.0
        };

        MetricsSummary {
            total_requests: self.total_requests,
            total_errors: self.total_errors,
            error_rate,
            rps,
            duration: elapsed,
            p50_ms: self.percentile(50.0),
            p90_ms: self.percentile(90.0),
            p95_ms: self.percentile(95.0),
            p99_ms: self.percentile(99.0),
            p999_ms: self.percentile(99.9),
            min_ms: if self.min_duration_ms == f64::MAX {
                0.0
            } else {
                self.min_duration_ms
            },
            max_ms: self.max_duration_ms,
            mean_ms: if self.total_requests > 0 {
                self.total_duration_ms / self.total_requests as f64
            } else {
                0.0
            },
            total_bytes: self.total_bytes,
            total_messages: self.total_messages,
            avg_dns_ms: if self.timed_count > 0 {
                self.total_dns_ms / self.timed_count as f64
            } else {
                0.0
            },
            avg_tcp_ms: if self.timed_count > 0 {
                self.total_tcp_ms / self.timed_count as f64
            } else {
                0.0
            },
            avg_tls_ms: if self.timed_count > 0 {
                self.total_tls_ms / self.timed_count as f64
            } else {
                0.0
            },
            avg_send_ms: if self.timed_count > 0 {
                self.total_send_ms / self.timed_count as f64
            } else {
                0.0
            },
            avg_ttfb_ms: if self.timed_count > 0 {
                self.total_ttfb_ms / self.timed_count as f64
            } else {
                0.0
            },
            avg_download_ms: if self.timed_count > 0 {
                self.total_download_ms / self.timed_count as f64
            } else {
                0.0
            },
            timed_count: self.timed_count,
            error_breakdown: {
                let mut groups: Vec<ErrorGroup> = self
                    .error_breakdown
                    .iter()
                    .map(|(k, (c, s))| ErrorGroup {
                        error_type: k.clone(),
                        count: *c,
                        sample: s.clone(),
                    })
                    .collect();
                groups.sort_by_key(|g| std::cmp::Reverse(g.count));
                groups
            },
        }
    }

    /// Compute a percentile (microseconds to milliseconds)
    fn percentile(&self, p: f64) -> f64 {
        self.histogram.value_at_quantile(p / 100.0) as f64 / 1000.0
    }

    /// Export the HDR histogram as a binary encoding (V2 format, for exact distributed merging)
    pub fn encode_histogram(&self) -> Result<Vec<u8>, MetricsError> {
        serialize_histogram(&self.histogram)
    }

    /// Export the HDR histogram as base64 (for `AgentMetricsSnapshot.hdr_histogram_b64`)
    pub fn encode_histogram_b64(&self) -> Result<String, MetricsError> {
        Ok(BASE64.encode(self.encode_histogram()?))
    }

    /// Merge another aggregator's HDR histogram (exact merge, not a mean approximation)
    ///
    /// Note: only the histogram itself is merged; request/error totals across agents must be accumulated separately by the caller.
    pub fn merge_histogram(&mut self, encoded: &[u8]) -> Result<(), MetricsError> {
        let other = deserialize_histogram(encoded)?;
        self.histogram
            .add(&other)
            .map_err(|e| MetricsError::Merge(format!("{:?}", e)))
    }
}

impl Default for MetricsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Serialize an HDR histogram to the V2 binary encoding (for distributed reporting)
pub fn serialize_histogram(hist: &Histogram<u64>) -> Result<Vec<u8>, MetricsError> {
    let mut serializer = V2Serializer::new();
    let mut buf = Vec::new();
    serializer
        .serialize(hist, &mut buf)
        .map_err(|e| MetricsError::Encode(format!("{:?}", e)))?;
    Ok(buf)
}

/// Deserialize an HDR histogram from the V2 binary encoding (for distributed merging)
pub fn deserialize_histogram(bytes: &[u8]) -> Result<Histogram<u64>, MetricsError> {
    let mut deserializer = Deserializer::new();
    let mut cursor = Cursor::new(bytes.to_vec());
    deserializer
        .deserialize::<u64, _>(&mut cursor)
        .map_err(|e| MetricsError::Merge(format!("{:?}", e)))
}

/// Deserialize an HDR histogram from a base64 encoding (V2 format)
pub fn deserialize_histogram_b64(s: &str) -> Result<Histogram<u64>, MetricsError> {
    let bytes = BASE64
        .decode(s)
        .map_err(|e| MetricsError::Merge(format!("base64: {:?}", e)))?;
    deserialize_histogram(&bytes)
}

/// Metrics summary
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSummary {
    pub total_requests: u64,
    pub total_errors: u64,
    pub error_rate: f64,
    pub rps: f64,
    pub duration: Duration,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub total_bytes: u64,
    /// Total streaming message count
    pub total_messages: u64,
    /// Average per-phase timings (only requests with timing data are counted)
    pub avg_dns_ms: f64,
    pub avg_tcp_ms: f64,
    pub avg_tls_ms: f64,
    pub avg_send_ms: f64,
    pub avg_ttfb_ms: f64,
    pub avg_download_ms: f64,
    pub timed_count: u64,
    /// Errors grouped by type (descending by count), shown in load test reports
    pub error_breakdown: Vec<ErrorGroup>,
}

impl std::fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "═══════════════════════════════════════════")?;
        writeln!(f, "  Load Test Summary")?;
        writeln!(f, "═══════════════════════════════════════════")?;
        writeln!(f, "  Duration:      {:?}", self.duration)?;
        writeln!(f, "  Total Requests: {}", self.total_requests)?;
        writeln!(
            f,
            "  Total Errors:   {} ({:.2}%)",
            self.total_errors,
            self.error_rate * 100.0
        )?;
        writeln!(f, "  RPS:            {:.1}", self.rps)?;
        writeln!(f, "  Data Transferred: {} bytes", self.total_bytes)?;
        writeln!(f, "───────────────────────────────────────────")?;
        writeln!(f, "  Latency Distribution:")?;
        writeln!(f, "    Min:    {:>8.2}ms", self.min_ms)?;
        writeln!(f, "    Mean:   {:>8.2}ms", self.mean_ms)?;
        writeln!(f, "    p50:    {:>8.2}ms", self.p50_ms)?;
        writeln!(f, "    p90:    {:>8.2}ms", self.p90_ms)?;
        writeln!(f, "    p95:    {:>8.2}ms", self.p95_ms)?;
        writeln!(f, "    p99:    {:>8.2}ms", self.p99_ms)?;
        writeln!(f, "    p99.9:  {:>8.2}ms", self.p999_ms)?;
        writeln!(f, "    Max:    {:>8.2}ms", self.max_ms)?;
        writeln!(f, "═══════════════════════════════════════════")
    }
}

/// Metrics Sink trait — local mpsc, extensible to a gRPC stream
///
/// This is the core reserved interface for distributed load testing.
pub trait MetricsSink: Send + Sync {
    fn push(&self, sample: MetricSample);
    fn snapshot(&self) -> MetricsSummary;
}

/// Local MetricsBus — the local implementation
pub struct LocalMetricsBus {
    // std Mutex: the critical section is only the O(1) record()/summary();
    // unlike the old RwLock + try_write, this never silently drops samples under high concurrency.
    aggregator: Arc<Mutex<MetricsAggregator>>,
    /// Current active VU count (maintained by the scheduler, shown in live charts)
    active_vus: Arc<AtomicU32>,
}

impl LocalMetricsBus {
    pub fn new() -> Self {
        Self {
            aggregator: Arc::new(Mutex::new(MetricsAggregator::new())),
            active_vus: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn aggregator(&self) -> &Arc<Mutex<MetricsAggregator>> {
        &self.aggregator
    }

    fn lock(&self) -> MutexGuard<'_, MetricsAggregator> {
        // Recover from a poisoned lock: a single panic must not lose every later sample
        self.aggregator.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Export the current aggregated histogram as base64 for distributed agents to report
    pub fn histogram_b64(&self) -> Option<String> {
        self.lock().encode_histogram_b64().ok()
    }

    /// Set the current active VU count (updated by the scheduler as VUs start/stop)
    pub fn set_active_vus(&self, vus: u32) {
        self.active_vus.store(vus, Ordering::Relaxed);
    }

    /// Current active VU count
    pub fn active_vus(&self) -> u32 {
        self.active_vus.load(Ordering::Relaxed)
    }

    /// Enable raw sample collection (for JTL / raw JSON export; zero overhead when disabled)
    pub fn enable_raw_samples(&self) {
        self.lock().enable_raw_samples();
    }

    /// Enable raw sample collection with an explicit cap (use a smaller cap to bound memory when the desktop app keeps it resident)
    pub fn enable_raw_samples_with_cap(&self, cap: usize) {
        self.lock().enable_raw_samples_with_cap(cap);
    }

    /// Collected raw samples (cloned)
    pub fn raw_samples(&self) -> Vec<MetricSample> {
        self.lock().raw_samples()
    }

    /// Whether raw samples were truncated at the cap
    pub fn raw_truncated(&self) -> bool {
        self.lock().raw_truncated()
    }
}

impl MetricsSink for LocalMetricsBus {
    fn push(&self, sample: MetricSample) {
        self.lock().record(&sample);
    }

    fn snapshot(&self) -> MetricsSummary {
        self.lock().summary()
    }
}

impl Default for LocalMetricsBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics error
#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    #[error("encoding failed: {0}")]
    Encode(String),
    #[error("merge failed: {0}")]
    Merge(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_aggregator() {
        let mut agg = MetricsAggregator::new();

        for i in 0..1000 {
            agg.record(&MetricSample {
                scenario: "test".into(),
                step: "step1".into(),
                vu_id: 0,
                iteration: i,
                status: 200,
                duration_ms: 10.0 + (i % 100) as f64,
                body_size: 1024,
                message_count: 0,
                is_error: i % 100 == 0,
                error_msg: None,
                error_type: String::new(),
                timestamp: 0,
                dns_ms: 1.0,
                tcp_ms: 2.0,
                tls_ms: 3.0,
                send_ms: 0.5,
                ttfb_ms: 15.0,
                download_ms: 5.0,
            });
        }

        let summary = agg.summary();
        assert_eq!(summary.total_requests, 1000);
        assert!(summary.error_rate > 0.0);
        assert!(summary.p50_ms > 0.0);
        assert!(summary.p95_ms > 0.0);
        assert!(summary.p99_ms > 0.0);
    }

    #[test]
    fn test_histogram_encode_decode() {
        let mut agg = MetricsAggregator::new();
        for i in 0..100 {
            agg.record(&MetricSample {
                scenario: "test".into(),
                step: "step".into(),
                vu_id: 0,
                iteration: i,
                status: 200,
                duration_ms: (i + 1) as f64,
                body_size: 100,
                message_count: 0,
                is_error: false,
                error_msg: None,
                error_type: String::new(),
                timestamp: 0,
                dns_ms: 0.0,
                tcp_ms: 0.0,
                tls_ms: 0.0,
                send_ms: 0.0,
                ttfb_ms: 0.0,
                download_ms: 0.0,
            });
        }

        // encode_histogram now serializes the real HDR histogram (V2 format)
        let encoded = agg.encode_histogram().unwrap();
        assert!(!encoded.is_empty());

        // merge_histogram performs exact HDR merge — percentiles must match the source
        let p50_src = agg.percentile(50.0);
        let p99_src = agg.percentile(99.0);
        let mut agg2 = MetricsAggregator::new();
        agg2.merge_histogram(&encoded).unwrap();
        let p50_merged = agg2.percentile(50.0);
        let p99_merged = agg2.percentile(99.0);
        assert!(
            (p50_src - p50_merged).abs() < 1e-6,
            "p50 mismatch: {} vs {}",
            p50_src,
            p50_merged
        );
        assert!(
            (p99_src - p99_merged).abs() < 1e-6,
            "p99 mismatch: {} vs {}",
            p99_src,
            p99_merged
        );
    }

    #[test]
    fn test_histogram_b64_roundtrip() {
        let mut agg = MetricsAggregator::new();
        for i in 1..=200u64 {
            agg.record(&MetricSample {
                scenario: "t".into(),
                step: "s".into(),
                vu_id: 0,
                iteration: i,
                status: 200,
                duration_ms: i as f64,
                body_size: 10,
                is_error: false,
                error_msg: None,
                error_type: String::new(),
                timestamp: 0,
                message_count: 0,
                dns_ms: 0.0,
                tcp_ms: 0.0,
                tls_ms: 0.0,
                send_ms: 0.0,
                ttfb_ms: 0.0,
                download_ms: 0.0,
            });
        }
        let b64 = agg.encode_histogram_b64().unwrap();
        assert!(!b64.is_empty());

        let decoded = deserialize_histogram_b64(&b64).unwrap();
        // Exact merge: the decoded histogram must match the source histogram's percentiles
        let src = agg.percentile(95.0);
        let merged_p95 = decoded.value_at_quantile(0.95) as f64 / 1000.0;
        assert!(
            (src - merged_p95).abs() < 1e-6,
            "b64 roundtrip p95 mismatch: {} vs {}",
            src,
            merged_p95
        );
    }
}

#[test]
fn test_metrics_bus_snapshot() {
    let bus = LocalMetricsBus::new();
    for i in 0..100 {
        bus.push(MetricSample {
            scenario: "test".into(),
            step: "s1".into(),
            vu_id: 0,
            iteration: i,
            status: 200,
            duration_ms: 20.0,
            body_size: 512,
            is_error: false,
            error_msg: None,
            error_type: String::new(),
            timestamp: 0,
            message_count: 0,
            dns_ms: 0.0,
            tcp_ms: 0.0,
            tls_ms: 0.0,
            send_ms: 0.0,
            ttfb_ms: 0.0,
            download_ms: 0.0,
        });
    }
    let snap = bus.snapshot();
    assert_eq!(snap.total_requests, 100);
    assert!(snap.p50_ms > 0.0);
}

#[test]
fn test_metrics_summary_display() {
    let summary = MetricsSummary {
        total_requests: 1000,
        total_errors: 10,
        error_rate: 0.01,
        rps: 100.0,
        duration: std::time::Duration::from_secs(10),
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
    };
    let display = format!("{}", summary);
    assert!(display.contains("1000"));
    assert!(display.contains("45.00ms"));
}
#[cfg(test)]
fn sample(
    duration_ms: f64,
    is_error: bool,
    error_type: &str,
    error_msg: Option<&str>,
) -> MetricSample {
    MetricSample {
        scenario: "s".into(),
        step: "st".into(),
        vu_id: 0,
        iteration: 0,
        status: if is_error { 500 } else { 200 },
        duration_ms,
        body_size: 128,
        message_count: 0,
        is_error,
        error_msg: error_msg.map(String::from),
        error_type: error_type.into(),
        timestamp: 0,
        dns_ms: 1.0,
        tcp_ms: 2.0,
        tls_ms: 3.0,
        send_ms: 0.5,
        ttfb_ms: 15.0,
        download_ms: 5.0,
    }
}

#[test]
fn test_error_breakdown_groups_by_type() {
    let mut agg = MetricsAggregator::new();
    agg.record(&sample(10.0, true, "assertion", Some("check failed")));
    agg.record(&sample(10.0, true, "assertion", None));
    agg.record(&sample(10.0, true, "timeout", Some("conn timeout")));
    agg.record(&sample(10.0, true, "", None)); // empty type -> other
    agg.record(&sample(10.0, false, "", None));

    let summary = agg.summary();
    assert_eq!(summary.total_errors, 4);
    let map: std::collections::HashMap<_, _> = summary
        .error_breakdown
        .iter()
        .map(|g| (g.error_type.clone(), g.count))
        .collect();
    assert_eq!(map["assertion"], 2);
    assert_eq!(map["timeout"], 1);
    assert_eq!(map["other"], 1);
    // Descending order: assertion must come first
    assert_eq!(summary.error_breakdown[0].error_type, "assertion");
    // Same type keeps the first representative error message
    let assertion = summary
        .error_breakdown
        .iter()
        .find(|g| g.error_type == "assertion")
        .unwrap();
    assert_eq!(assertion.sample, "check failed");
}

#[test]
fn test_raw_samples_truncated_at_cap() {
    let mut agg = MetricsAggregator::new();
    agg.enable_raw_samples_with_cap(2);
    for i in 0..5 {
        agg.record(&sample(10.0 + i as f64, false, "", None));
    }
    assert!(
        agg.raw_truncated(),
        "exceeding the cap must flag truncation"
    );
    assert_eq!(agg.raw_samples().len(), 2);
    assert_eq!(agg.raw_samples()[0].duration_ms, 10.0);
    assert_eq!(agg.raw_samples()[1].duration_ms, 11.0);
}

#[test]
fn test_summary_min_max_mean() {
    let mut agg = MetricsAggregator::new();
    for ms in [100.0, 50.0, 200.0, 25.0] {
        agg.record(&sample(ms, false, "", None));
    }
    let summary = agg.summary();
    assert_eq!(summary.total_requests, 4);
    assert_eq!(summary.min_ms, 25.0);
    assert_eq!(summary.max_ms, 200.0);
    assert!((summary.mean_ms - 93.75).abs() < 1.0);
    assert_eq!(summary.total_bytes, 4 * 128);
}

#[test]
fn test_timing_stats_only_for_timed_requests() {
    let mut agg = MetricsAggregator::new();
    let mut untimed = sample(10.0, false, "", None);
    untimed.dns_ms = 0.0;
    untimed.tcp_ms = 0.0;
    untimed.tls_ms = 0.0;
    untimed.ttfb_ms = 0.0;
    agg.record(&untimed); // no timing -> not counted in avg
    agg.record(&sample(10.0, false, "", None)); // has timing
    agg.record(&sample(10.0, false, "", None));

    let summary = agg.summary();
    assert_eq!(
        summary.timed_count, 2,
        "only requests carrying timing are counted"
    );
    assert_eq!(summary.avg_dns_ms, 1.0);
    assert_eq!(summary.avg_ttfb_ms, 15.0);
}
