//! Internal type ↔ gRPC proto conversions

use crate::proto;
use crate::types::{
    AgentMetricsSnapshot, AgentState, ExecuteRequestData, ExecuteResult, ExecuteTimings,
    MetricsSummary, ResourceSnapshot,
};

pub fn state_to_proto(s: AgentState) -> proto::AgentState {
    proto::AgentState::try_from(i32::from(s)).unwrap_or(proto::AgentState::Idle)
}

pub fn snapshot_to_proto(s: &AgentMetricsSnapshot) -> proto::AgentMetricsSnapshot {
    proto::AgentMetricsSnapshot {
        agent_id: s.agent_id.clone(),
        timestamp_ms: s.timestamp_ms,
        active_vus: s.active_vus,
        total_requests: s.total_requests,
        total_errors: s.total_errors,
        hdr_histogram_b64: s.hdr_histogram_b64.clone(),
        summary: s.summary.as_ref().map(|m| proto::MetricsSummary {
            p50_ms: m.p50_ms,
            p90_ms: m.p90_ms,
            p95_ms: m.p95_ms,
            p99_ms: m.p99_ms,
            mean_ms: m.mean_ms,
            rps: m.rps,
            error_rate: m.error_rate,
        }),
    }
}

pub fn proto_to_snapshot(p: &proto::AgentMetricsSnapshot) -> AgentMetricsSnapshot {
    AgentMetricsSnapshot {
        agent_id: p.agent_id.clone(),
        timestamp_ms: p.timestamp_ms,
        active_vus: p.active_vus,
        total_requests: p.total_requests,
        total_errors: p.total_errors,
        hdr_histogram_b64: p.hdr_histogram_b64.clone(),
        summary: p.summary.as_ref().map(|m| MetricsSummary {
            p50_ms: m.p50_ms,
            p90_ms: m.p90_ms,
            p95_ms: m.p95_ms,
            p99_ms: m.p99_ms,
            mean_ms: m.mean_ms,
            rps: m.rps,
            error_rate: m.error_rate,
        }),
    }
}

pub fn resource_to_proto(r: &ResourceSnapshot) -> proto::ResourceSnapshot {
    proto::ResourceSnapshot {
        cpu_percent: r.cpu_percent,
        mem_used_mb: r.mem_used_mb,
        mem_total_mb: r.mem_total_mb,
        mem_percent: r.mem_percent,
        load_avg_1m: r.load_avg_1m,
        timestamp_ms: r.timestamp_ms,
    }
}

pub fn proto_to_resource(p: &proto::ResourceSnapshot) -> ResourceSnapshot {
    ResourceSnapshot {
        cpu_percent: p.cpu_percent,
        mem_used_mb: p.mem_used_mb,
        mem_total_mb: p.mem_total_mb,
        mem_percent: p.mem_percent,
        load_avg_1m: p.load_avg_1m,
        timestamp_ms: p.timestamp_ms,
    }
}

pub fn labels_to_proto(labels: &[(String, String)]) -> Vec<proto::Label> {
    labels
        .iter()
        .map(|(k, v)| proto::Label {
            key: k.clone(),
            value: v.clone(),
        })
        .collect()
}

pub fn labels_from_proto(labels: &[proto::Label]) -> Vec<(String, String)> {
    labels
        .iter()
        .map(|l| (l.key.clone(), l.value.clone()))
        .collect()
}

pub fn execute_data_to_proto(d: &ExecuteRequestData) -> proto::ExecuteRequestPayload {
    proto::ExecuteRequestPayload {
        method: d.method.clone(),
        url: d.url.clone(),
        headers: d.headers.clone(),
        body: d.body.clone(),
        protocol: d.protocol.clone(),
        request_format: d.request_format.clone().unwrap_or_default(),
        response_format: d.response_format.clone().unwrap_or_default(),
        prereq_script: d.prereq_script.clone().unwrap_or_default(),
        postreq_script: d.postreq_script.clone().unwrap_or_default(),
        env_vars: d.env_vars.clone(),
    }
}

pub fn proto_to_execute_result(r: &proto::ExecuteResponse) -> ExecuteResult {
    ExecuteResult {
        status: r.status,
        headers: r.headers.clone(),
        body: r.body.clone(),
        duration_ms: r.duration_ms,
        error: if r.error.is_empty() {
            None
        } else {
            Some(r.error.clone())
        },
        timing: r.timing.as_ref().map(|t| ExecuteTimings {
            dns_ms: t.dns_ms,
            tcp_ms: t.tcp_ms,
            tls_ms: t.tls_ms,
            ttfb_ms: t.ttfb_ms,
            download_ms: t.download_ms,
            total_ms: t.total_ms,
        }),
        pre_logs: r
            .pre_logs
            .iter()
            .map(|l| crate::types::ScriptLogEntry {
                level: l.level.clone(),
                message: l.message.clone(),
            })
            .collect(),
        post_logs: r
            .post_logs
            .iter()
            .map(|l| crate::types::ScriptLogEntry {
                level: l.level.clone(),
                message: l.message.clone(),
            })
            .collect(),
        decoded: if r.decoded.is_empty() {
            None
        } else {
            Some(r.decoded.clone())
        },
        post_tests: r
            .post_tests
            .iter()
            .map(|t| crate::types::TestResultEntry {
                name: t.name.clone(),
                passed: t.passed,
                message: t.message.clone(),
            })
            .collect(),
    }
}
