//! HDR histogram merger - exact percentile computation across agents

use crate::types::{AgentMetricsSnapshot, DistributedError, DistributedResult};
use hdrhistogram::Histogram;

pub struct MetricsMerger {
    merged: Histogram<u64>,
    total_requests: u64,
    total_errors: u64,
    agent_snapshots: Vec<AgentMetricsSnapshot>,
    /// Start time of this run (recorded on reset, used to compute RPS from the actual elapsed time)
    started_at: Option<std::time::Instant>,
}

impl MetricsMerger {
    pub fn new() -> Self {
        Self {
            merged: Histogram::new(3).expect("Failed to create HDR histogram"),
            total_requests: 0,
            total_errors: 0,
            agent_snapshots: Vec::new(),
            started_at: None,
        }
    }

    /// Reset for a new load-test round (clear merged results and record the start time)
    pub fn reset(&mut self) {
        self.merged.reset();
        self.total_requests = 0;
        self.total_errors = 0;
        self.agent_snapshots.clear();
        self.started_at = Some(std::time::Instant::now());
    }

    /// Merge a snapshot from one agent
    ///
    /// Prefers `hdr_histogram_b64` for an **exact HDR merge** (lossless percentiles);
    /// when the field is empty (the agent reported no histogram) or decoding fails, it degrades to a mean-based approximation so no data is lost.
    pub fn merge_snapshot(
        &mut self,
        snapshot: AgentMetricsSnapshot,
    ) -> Result<(), DistributedError> {
        self.total_requests += snapshot.total_requests;
        self.total_errors += snapshot.total_errors;

        if snapshot.hdr_histogram_b64.is_empty() {
            Self::merge_approx(
                &mut self.merged,
                snapshot.total_requests,
                snapshot.summary.as_ref().map(|s| s.mean_ms),
            );
        } else {
            match orbit_metrics::deserialize_histogram_b64(&snapshot.hdr_histogram_b64) {
                Ok(other) => {
                    self.merged
                        .add(&other)
                        .map_err(|e| DistributedError::Merge(format!("{:?}", e)))?;
                }
                Err(e) => {
                    tracing::warn!(
                        "agent {} HDR histogram decode failed; falling back to mean approximation: {}",
                        snapshot.agent_id,
                        e
                    );
                    Self::merge_approx(
                        &mut self.merged,
                        snapshot.total_requests,
                        snapshot.summary.as_ref().map(|s| s.mean_ms),
                    );
                }
            }
        }

        self.agent_snapshots.push(snapshot);
        Ok(())
    }

    /// Fallback approximation: record the mean `total_requests` times (only as a floor when no HDR data exists)
    fn merge_approx(merged: &mut Histogram<u64>, total_requests: u64, mean_ms: Option<f64>) {
        if let Some(mean_ms) = mean_ms {
            let count = total_requests.max(1);
            let mean_us = (mean_ms * 1000.0) as u64;
            for _ in 0..count.min(10000) {
                merged.record(mean_us).ok();
            }
        }
    }

    pub fn finalize(&self, duration_secs: f64) -> DistributedResult {
        let rps = if duration_secs > 0.0 {
            self.total_requests as f64 / duration_secs
        } else {
            0.0
        };
        let error_rate = if self.total_requests > 0 {
            self.total_errors as f64 / self.total_requests as f64
        } else {
            0.0
        };

        DistributedResult {
            total_requests: self.total_requests,
            total_errors: self.total_errors,
            error_rate,
            rps,
            p50_ms: self.merged.value_at_quantile(0.50) as f64 / 1000.0,
            p90_ms: self.merged.value_at_quantile(0.90) as f64 / 1000.0,
            p95_ms: self.merged.value_at_quantile(0.95) as f64 / 1000.0,
            p99_ms: self.merged.value_at_quantile(0.99) as f64 / 1000.0,
            p999_ms: self.merged.value_at_quantile(0.999) as f64 / 1000.0,
            mean_ms: self.merged.mean() / 1000.0,
            agent_count: self.agent_snapshots.len(),
            agent_stats: self.agent_snapshots.clone(),
        }
    }

    /// Finalize using the actual elapsed time (from reset to now)
    pub fn finalize_elapsed(&self) -> DistributedResult {
        let secs = self
            .started_at
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        self.finalize(secs)
    }
}

impl Default for MetricsMerger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_metrics::MetricsAggregator;

    fn make_snapshot(agent_id: &str, requests: u64, mean_ms: f64) -> AgentMetricsSnapshot {
        AgentMetricsSnapshot {
            agent_id: agent_id.into(),
            timestamp_ms: 0,
            active_vus: 1,
            total_requests: requests,
            total_errors: 0,
            hdr_histogram_b64: String::new(),
            summary: Some(crate::types::MetricsSummary {
                p50_ms: mean_ms,
                p90_ms: mean_ms * 1.5,
                p95_ms: mean_ms * 2.0,
                p99_ms: mean_ms * 3.0,
                mean_ms,
                rps: requests as f64,
                error_rate: 0.0,
            }),
        }
    }

    /// Build a snapshot with a real HDR histogram (latency: start_ms..=end_ms)
    fn snapshot_with_histogram(agent_id: &str, start_ms: u64, end_ms: u64) -> AgentMetricsSnapshot {
        let mut agg = MetricsAggregator::new();
        for i in start_ms..=end_ms {
            agg.record(&orbit_metrics::MetricSample {
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
        let count = end_ms - start_ms + 1;
        AgentMetricsSnapshot {
            agent_id: agent_id.into(),
            timestamp_ms: 0,
            active_vus: 1,
            total_requests: count,
            total_errors: 0,
            hdr_histogram_b64: agg.encode_histogram_b64().unwrap(),
            summary: None,
        }
    }

    #[test]
    fn test_merge_two_agents() {
        let mut merger = MetricsMerger::new();
        merger
            .merge_snapshot(make_snapshot("agent-1", 100, 5.0))
            .unwrap();
        merger
            .merge_snapshot(make_snapshot("agent-2", 200, 10.0))
            .unwrap();

        let result = merger.finalize(1.0);
        assert_eq!(result.total_requests, 300);
        assert_eq!(result.agent_count, 2);
        assert!(result.rps > 0.0);
    }

    #[test]
    fn test_merge_empty() {
        let merger = MetricsMerger::new();
        let result = merger.finalize(1.0);
        assert_eq!(result.total_requests, 0);
    }

    /// Exact HDR merge: two agents each report a full histogram; the merged percentiles must equal the exact values of the union
    #[test]
    fn test_merge_precise_hdr() {
        // Agent 1: 1..=100 ms, Agent 2: 101..=200 ms → union 1..=200 ms
        let mut merger = MetricsMerger::new();
        merger
            .merge_snapshot(snapshot_with_histogram("a1", 1, 100))
            .unwrap();
        merger
            .merge_snapshot(snapshot_with_histogram("a2", 101, 200))
            .unwrap();

        let result = merger.finalize(1.0);
        assert_eq!(result.total_requests, 200);
        // exact median of the 1..=200 union ≈ 100.5ms
        assert!(
            (result.p50_ms - 100.5).abs() < 2.0,
            "p50 = {}",
            result.p50_ms
        );
        // p99 of 1..=200 ≈ 198ms (HDR 3-significant-digit approximation)
        assert!(
            result.p99_ms > 190.0 && result.p99_ms < 200.0,
            "p99 = {}",
            result.p99_ms
        );
        // p999 of 1..=200 = 200ms
        assert!(result.p999_ms > 195.0, "p999 = {}", result.p999_ms);
    }
}
