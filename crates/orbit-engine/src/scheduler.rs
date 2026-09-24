//! VU scheduler
//!
//! Responsibilities:
//! 1. Allocate VUs according to the executor type
//! 2. Manage the VU lifecycle
//! 3. Synchronized startup
//! 4. Timeout control

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::feeder::{CsvFeeder, FeederMode};
use orbit_codec::traits::Codec;
use orbit_config::{Executor, OnError, RampMode, RampingStage, Scenario, Step, TestPlan};
use orbit_metrics::{LocalMetricsBus, MetricSample, MetricsSink};
use orbit_protocol::traits::ProtocolClient;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::flow_runner::FlowRunner;
use crate::EngineError;

/// Macro for building an error MetricSample - reduces boilerplate
macro_rules! error_sample {
    ($scenario:expr, $step:expr, $vu_id:expr, $iteration:expr, $msg:expr) => {
        MetricSample {
            scenario: $scenario,
            step: $step,
            vu_id: $vu_id,
            iteration: $iteration,
            status: 0,
            duration_ms: 0.0,
            body_size: 0,
            message_count: 0,
            is_error: true,
            error_msg: Some($msg),
            error_type: "internal".to_string(),
            timestamp: 0,
            dns_ms: 0.0,
            tcp_ms: 0.0,
            tls_ms: 0.0,
            send_ms: 0.0,
            ttfb_ms: 0.0,
            download_ms: 0.0,
        }
    };
}

pub struct Scheduler {
    plan: TestPlan,
    protocol: Arc<dyn ProtocolClient>,
    codec: Arc<dyn Codec>,
    metrics: Arc<LocalMetricsBus>,
    /// Optional event channel - pushes step-level execution progress (for SSE / Tauri events)
    event_tx: Option<tokio::sync::broadcast::Sender<String>>,
    /// Whether to capture request/response details (sequential path only; step events carry a detail field)
    capture_requests: bool,
}

impl Scheduler {
    pub fn new(
        plan: TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        metrics: Arc<LocalMetricsBus>,
    ) -> Self {
        Self {
            plan,
            protocol,
            codec,
            metrics,
            event_tx: None,
            capture_requests: false,
        }
    }

    /// Set the event channel (for pushing step progress during scenario execution)
    pub fn with_event_tx(mut self, tx: tokio::sync::broadcast::Sender<String>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Enable request/response detail capture (the automation scenario "record request details" switch)
    pub fn with_capture_requests(mut self, on: bool) -> Self {
        self.capture_requests = on;
        self
    }

    pub async fn run(&self) -> Result<(), EngineError> {
        self.run_with_abort(None).await
    }

    pub async fn run_with_abort(&self, abort: Option<Arc<AtomicBool>>) -> Result<(), EngineError> {
        // Run-level cancellation token: cancelled when abort is set, so in-flight requests are interrupted promptly.
        // The watcher task exits when abort is set or the run ends (token cancelled).
        let cancel = CancellationToken::new();
        if let Some(flag) = &abort {
            let watcher_cancel = cancel.clone();
            let flag = flag.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = watcher_cancel.cancelled() => {}
                    _ = async {
                        while !flag.load(Ordering::Relaxed) {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    } => {}
                }
            });
        }

        for scenario in &self.plan.scenarios {
            self.run_scenario(scenario, &abort, &cancel).await?;
        }

        // Run finished: cancel the token so the watcher task exits (abort may never have been set)
        cancel.cancel();
        Ok(())
    }

    async fn run_scenario(
        &self,
        scenario: &Scenario,
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        match &scenario.executor {
            Executor::ConstantVus {
                vus,
                duration,
                ramp_up,
            } => {
                self.run_constant_vus(scenario, *vus, duration, ramp_up, abort, cancel)
                    .await
            }
            Executor::RampingVus {
                start_vus,
                max_vus,
                stages,
            } => {
                self.run_ramping_vus(scenario, *start_vus, *max_vus, stages, abort, cancel)
                    .await
            }
            Executor::ConstantArrivalRate {
                rate,
                duration,
                ramp_up,
                pre_allocated_vus,
            } => {
                self.run_constant_arrival_rate(
                    scenario,
                    *rate,
                    duration,
                    ramp_up,
                    *pre_allocated_vus,
                    abort,
                    cancel,
                )
                .await
            }
            Executor::Sequential { iterations: _ } => {
                self.run_sequential(scenario, abort, cancel).await
            }
        }
    }

    fn aborted(abort: &Option<Arc<AtomicBool>>) -> bool {
        abort.as_ref().is_some_and(|a| a.load(Ordering::Relaxed))
    }

    /// An abortable sleep: returns true when it sleeps the full duration, false when interrupted.
    /// Used for stage-hold / ramp intervals and other long waits, so a stop exits the engine promptly.
    async fn sleep_abortable(duration: Duration, abort: &Option<Arc<AtomicBool>>) -> bool {
        if Self::aborted(abort) {
            return false;
        }
        if duration.is_zero() {
            return true;
        }
        let mut remaining = duration;
        loop {
            let step = remaining.min(Duration::from_millis(100));
            tokio::time::sleep(step).await;
            remaining -= step;
            if remaining.is_zero() {
                return true;
            }
            if Self::aborted(abort) {
                return false;
            }
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Sequential execution mode (automation scenarios)
    // ═══════════════════════════════════════════════════════════
    async fn run_sequential(
        &self,
        scenario: &Scenario,
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        let mut runner = FlowRunner::new(
            self.protocol.clone_client(),
            self.codec.clone_codec(),
            self.plan.variables.clone(),
        )
        .with_scenario(&scenario.name)
        .with_cancel(cancel.clone())
        .with_capture_requests(self.capture_requests);
        let steps = &scenario.steps;

        if Self::aborted(abort) {
            return Ok(());
        }

        let stop_on_error = matches!(scenario.on_error, OnError::Stop);
        let event_tx = self.event_tx.clone();
        self.execute_steps_with_events(&mut runner, steps, 0, stop_on_error, &event_tx)
            .await
    }

    /// Recursively execute the step tree, pushing a progress event over event_tx after each step
    async fn execute_steps_with_events(
        &self,
        runner: &mut FlowRunner,
        steps: &[Step],
        vu_id: u64,
        stop_on_error: bool,
        event_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    ) -> Result<(), EngineError> {
        let mut iteration = 0u64;
        for step in steps {
            if step.is_disabled() {
                continue;
            }

            let step_name = step.name().to_string();

            match step {
                Step::Request { .. } => {
                    match runner
                        .execute_steps(std::slice::from_ref(step), vu_id, iteration, false)
                        .await
                    {
                        Ok(samples) => {
                            for sample in &samples {
                                let is_err = sample.is_error;
                                self.metrics.push(sample.clone());
                                // "Record request details": one captured detail per sample (None when disabled)
                                let detail = runner
                                    .pop_captured()
                                    .and_then(|d| serde_json::to_value(&d).ok());
                                Self::emit_step(
                                    &step_name,
                                    if is_err { "fail" } else { "pass" },
                                    sample.duration_ms,
                                    detail,
                                    event_tx,
                                );
                                if is_err && stop_on_error {
                                    return Ok(());
                                }
                            }
                        }
                        Err(e) => {
                            Self::emit_step(&step_name, "fail", 0.0, None, event_tx);
                            self.metrics.push(MetricSample {
                                scenario: String::new(),
                                step: step_name.clone(),
                                vu_id,
                                iteration,
                                status: 0,
                                duration_ms: 0.0,
                                body_size: 0,
                                message_count: 0,
                                is_error: true,
                                error_msg: Some(e.clone()),
                                error_type: "internal".to_string(),
                                timestamp: 0,
                                dns_ms: 0.0,
                                tcp_ms: 0.0,
                                tls_ms: 0.0,
                                send_ms: 0.0,
                                ttfb_ms: 0.0,
                                download_ms: 0.0,
                            });
                            if stop_on_error {
                                return Ok(());
                            }
                        }
                    }
                }
                Step::Loop {
                    count,
                    steps: child_steps,
                    ..
                } => {
                    for i in 0..*count {
                        // Expose the loop index as the variable `loop.index` (0-based) for child steps
                        let prev = runner.variables().get("loop.index").cloned();
                        runner.inject_variable("loop.index".to_string(), i.to_string());
                        Self::emit_step(&step_name, "running", 0.0, None, event_tx);
                        Box::pin(self.execute_steps_with_events(
                            runner,
                            child_steps,
                            vu_id,
                            stop_on_error,
                            event_tx,
                        ))
                        .await?;
                        // Restore (supports nested loops)
                        match prev {
                            Some(v) => runner.inject_variable("loop.index".to_string(), v),
                            None => runner.remove_variable("loop.index"),
                        }
                    }
                    Self::emit_step(&step_name, "pass", 0.0, None, event_tx);
                }
                Step::Wait { duration, .. } => {
                    Self::emit_step(&step_name, "running", 0.0, None, event_tx);
                    let secs = orbit_config::parse_duration(duration).unwrap_or(1.0);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
                    Self::emit_step(&step_name, "pass", secs * 1000.0, None, event_tx);
                }
                Step::SetVar { key, value, .. } => {
                    let resolved = orbit_config::interpolate(value, &runner.variables())
                        .unwrap_or_else(|_| value.clone());
                    runner.inject_variable(key.clone(), resolved);
                    Self::emit_step(&step_name, "pass", 0.0, None, event_tx);
                }
                Step::Condition {
                    expression,
                    then: then_steps,
                    else_steps,
                    ..
                } => {
                    let branch = runner.evaluate_condition(expression).unwrap_or(false);
                    Self::emit_step(
                        &step_name,
                        if branch { "pass" } else { "running" },
                        0.0,
                        None,
                        event_tx,
                    );
                    let target = if branch { then_steps } else { else_steps };
                    Box::pin(self.execute_steps_with_events(
                        runner,
                        target,
                        vu_id,
                        stop_on_error,
                        event_tx,
                    ))
                    .await?;
                }
                Step::Group {
                    steps: child_steps, ..
                } => {
                    Self::emit_step(&step_name, "running", 0.0, None, event_tx);
                    Box::pin(self.execute_steps_with_events(
                        runner,
                        child_steps,
                        vu_id,
                        stop_on_error,
                        event_tx,
                    ))
                    .await?;
                }
            }
            iteration += 1;
        }
        Ok(())
    }

    fn emit_step(
        name: &str,
        status: &str,
        duration_ms: f64,
        detail: Option<serde_json::Value>,
        tx: &Option<tokio::sync::broadcast::Sender<String>>,
    ) {
        if let Some(tx) = tx {
            let mut payload = serde_json::json!({
                "type": "step",
                "step": name,
                "status": status,
                "duration_ms": duration_ms,
            });
            if let Some(d) = detail {
                payload["detail"] = d;
            }
            let _ = tx.send(payload.to_string());
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Constant VU mode
    // ═══════════════════════════════════════════════════════════
    //
    // Standard semantics (aligned with k6 constant-vus):
    //   Phase 1 (ramp-up):  start VUs gradually, warm-up only, no metrics recorded
    //   Phase 2 (steady):   all VUs run together for the full duration, metrics recorded
    //   total time ~= ramp_up + duration
    async fn run_constant_vus(
        &self,
        scenario: &Scenario,
        vus: u32,
        duration: &str,
        ramp_up: &str,
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        let load_duration = Duration::from_secs_f64(
            orbit_config::parse_duration(duration)
                .map_err(|e| EngineError::Scheduler(e.to_string()))?,
        );
        let ramp_duration = Duration::from_secs_f64(
            orbit_config::parse_duration(ramp_up)
                .map_err(|e| EngineError::Scheduler(e.to_string()))?,
        );

        tracing::info!(
            "Scenario '{}': {} VUs, {}s load + {}s ramp-up = ~{}s total",
            scenario.name,
            vus,
            load_duration.as_secs_f64(),
            ramp_duration.as_secs_f64(),
            load_duration.as_secs_f64() + ramp_duration.as_secs_f64()
        );

        // watch channel: the initial value is far in the future (VUs do not exit during the ramp)
        let (deadline_tx, deadline_rx) =
            watch::channel(Instant::now() + Duration::from_secs(86400));

        let ramp_interval = if vus > 1 && !ramp_duration.is_zero() {
            ramp_duration / vus
        } else {
            Duration::ZERO
        };

        let mut handles = Vec::new();

        for vu_id in 0..vus {
            if !ramp_interval.is_zero() {
                tokio::time::sleep(ramp_interval).await;
            }
            if Self::aborted(abort) {
                break;
            }
            self.metrics.set_active_vus(vu_id + 1);

            let scenario_name = scenario.name.clone();
            let steps = scenario.steps.clone();
            let protocol = self.protocol.clone_client();
            let codec = self.codec.clone_codec();
            let metrics = self.metrics.clone();
            let plan_vars = self.plan.variables.clone();
            let abort_task = abort.clone();
            let deadline_rx_vu = deadline_rx.clone();
            let cancel_vu = cancel.clone();

            let handle = tokio::spawn(async move {
                let mut runner = FlowRunner::new(protocol, codec, plan_vars)
                    .with_scenario(&scenario_name)
                    .with_cancel(cancel_vu);
                let mut feeder: Option<CsvFeeder> = None;
                if let Ok(path) = std::env::var("ORBIT_FEEDER_CSV") {
                    match CsvFeeder::from_file(std::path::Path::new(&path), FeederMode::Sequential)
                    {
                        Ok(f) => {
                            feeder = Some(f);
                        }
                        Err(e) => {
                            tracing::warn!("Feeder load failed: {}", e);
                        }
                    }
                }

                let mut iteration = 0u64;
                let mut in_steady = false;

                loop {
                    let deadline = *deadline_rx_vu.borrow();
                    if Instant::now() >= deadline {
                        break;
                    }
                    if Self::aborted(&abort_task) {
                        break;
                    }

                    // Detect steady state: the deadline was updated by the main thread to the real cutoff (no longer the 86400s initial value)
                    if !in_steady && deadline < Instant::now() + Duration::from_secs(72000) {
                        in_steady = true;
                    }

                    if let Some(ref mut f) = feeder {
                        let row = f.next();
                        for (key, value) in &row {
                            runner.inject_variable(key.clone(), value.clone());
                        }
                    }

                    match runner
                        .execute_steps(&steps, vu_id as u64, iteration, false)
                        .await
                    {
                        Ok(samples) => {
                            if in_steady {
                                for s in samples {
                                    metrics.push(s);
                                }
                            }
                        }
                        Err(e) => {
                            if in_steady {
                                metrics.push(error_sample!(
                                    scenario_name.clone(),
                                    "unknown".into(),
                                    vu_id as u64,
                                    iteration,
                                    e
                                ));
                            }
                        }
                    }
                    iteration += 1;
                }
            });

            handles.push(handle);
        }

        // Ramp-up done -> set the shared deadline = now + load_duration so all VUs enter steady state at once
        let full_load_deadline = Instant::now() + load_duration;
        tracing::info!(
            "ConstantVus: steady state until {:?} ({}s)",
            full_load_deadline,
            load_duration.as_secs_f64()
        );
        let _ = deadline_tx.send(full_load_deadline);

        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Ramping VU mode (ramping-vus, aligned with k6 ramping-vus + JMeter extensions)
    // ═══════════════════════════════════════════════════════════
    //
    // Semantics:
    //   start_vus VUs start immediately;
    //   each stage has target / duration / ramp:
    //     - gradual: change linearly from the current VU count to target within duration (k6 semantics, ramps both up and down)
    //     - instant: adjust to target immediately at stage start, then hold until the stage ends
    //     - jmeter: climb linearly to target within ramp_up, then hold until the stage ends
    //   max_vus acts as the concurrency cap (0 = unlimited)
    fn spawn_step_vu(
        &self,
        scenario: &Scenario,
        vu_id: u64,
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> (watch::Sender<Instant>, tokio::task::JoinHandle<()>) {
        let (deadline_tx, deadline_rx) =
            watch::channel(Instant::now() + Duration::from_secs(86400));
        let scenario_name = scenario.name.clone();
        let steps = scenario.steps.clone();
        let protocol = self.protocol.clone_client();
        let codec = self.codec.clone_codec();
        let metrics = self.metrics.clone();
        let plan_vars = self.plan.variables.clone();
        let abort_task = abort.clone();
        let cancel_vu = cancel.clone();
        let join = tokio::spawn(async move {
            let mut runner = FlowRunner::new(protocol, codec, plan_vars)
                .with_scenario(&scenario_name)
                .with_cancel(cancel_vu);
            let mut iteration = 0u64;
            loop {
                let deadline = *deadline_rx.borrow();
                if Instant::now() >= deadline {
                    break;
                }
                if Self::aborted(&abort_task) {
                    break;
                }
                match runner.execute_steps(&steps, vu_id, iteration, false).await {
                    Ok(samples) => {
                        for s in samples {
                            metrics.push(s);
                        }
                    }
                    Err(e) => {
                        metrics.push(error_sample!(
                            scenario_name.clone(),
                            "unknown".into(),
                            vu_id,
                            iteration,
                            e
                        ));
                    }
                }
                iteration += 1;
            }
        });
        (deadline_tx, join)
    }

    async fn run_ramping_vus(
        &self,
        scenario: &Scenario,
        start_vus: u32,
        max_vus: u32,
        stages: &[RampingStage],
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        let mut vus: Vec<(watch::Sender<Instant>, tokio::task::JoinHandle<()>)> = Vec::new();
        let start = if max_vus > 0 {
            start_vus.min(max_vus)
        } else {
            start_vus
        };
        let mut current_vus = 0u32;

        for _ in 0..start {
            vus.push(self.spawn_step_vu(scenario, vus.len() as u64, abort, cancel));
            current_vus += 1;
        }
        self.metrics.set_active_vus(current_vus);

        for (i, stage) in stages.iter().enumerate() {
            let stage_duration = Duration::from_secs_f64(
                orbit_config::parse_duration(&stage.duration)
                    .map_err(|e| EngineError::Scheduler(e.to_string()))?,
            );
            // cap at max_vus (0 = unlimited)
            let target = if max_vus > 0 {
                stage.target.min(max_vus)
            } else {
                stage.target
            };
            let diff = target as i64 - current_vus as i64;

            tracing::info!(
                "Ramping stage {}: {} → {} VUs ({}), {}s",
                i + 1,
                current_vus,
                target,
                match stage.ramp {
                    RampMode::Instant => "instant",
                    RampMode::Gradual => "gradual",
                    RampMode::Jmeter => "jmeter",
                },
                stage_duration.as_secs_f64()
            );

            match stage.ramp {
                RampMode::Instant => {
                    // adjust to target in one shot, then hold until the stage ends
                    while current_vus < target {
                        vus.push(self.spawn_step_vu(scenario, vus.len() as u64, abort, cancel));
                        current_vus += 1;
                        self.metrics.set_active_vus(current_vus);
                    }
                    while current_vus > target {
                        if let Some((tx, _)) = vus.pop() {
                            let _ = tx.send(Instant::now());
                        }
                        current_vus -= 1;
                        self.metrics.set_active_vus(current_vus);
                    }
                    Self::sleep_abortable(stage_duration, abort).await;
                }
                RampMode::Gradual => {
                    if diff > 0 {
                        let interval = stage_duration / diff as u32;
                        for _ in 0..diff {
                            // each new VU joins at an integer multiple of interval, the last one landing exactly at the stage end
                            if !Self::sleep_abortable(interval, abort).await {
                                break;
                            }
                            vus.push(self.spawn_step_vu(scenario, vus.len() as u64, abort, cancel));
                            current_vus += 1;
                            self.metrics.set_active_vus(current_vus);
                        }
                    } else if diff < 0 {
                        let interval = stage_duration / (-diff) as u32;
                        for _ in 0..(-diff) {
                            if !Self::sleep_abortable(interval, abort).await {
                                break;
                            }
                            if let Some((tx, _)) = vus.pop() {
                                let _ = tx.send(Instant::now());
                            }
                            current_vus -= 1;
                            self.metrics.set_active_vus(current_vus);
                        }
                    } else {
                        // target unchanged: hold for the whole stage
                        Self::sleep_abortable(stage_duration, abort).await;
                    }
                }
                RampMode::Jmeter => {
                    // JMeter Thread Group semantics: climb linearly to target within ramp_up,
                    // then hold at full load until the stage ends (total stage duration = duration)
                    let ramp_duration = stage
                        .ramp_up
                        .as_deref()
                        .map(|s| -> Result<Duration, EngineError> {
                            Ok(Duration::from_secs_f64(
                                orbit_config::parse_duration(s)
                                    .map_err(|e| EngineError::Scheduler(e.to_string()))?,
                            ))
                        })
                        .transpose()?
                        .unwrap_or(stage_duration)
                        .min(stage_duration);
                    if diff == 0 {
                        Self::sleep_abortable(stage_duration, abort).await;
                    } else if diff > 0 {
                        let interval = ramp_duration / diff as u32;
                        for _ in 0..diff {
                            if !Self::sleep_abortable(interval, abort).await {
                                break;
                            }
                            vus.push(self.spawn_step_vu(scenario, vus.len() as u64, abort, cancel));
                            current_vus += 1;
                            self.metrics.set_active_vus(current_vus);
                        }
                        // hold until the stage ends
                        if !Self::aborted(abort) {
                            Self::sleep_abortable(
                                stage_duration.saturating_sub(ramp_duration),
                                abort,
                            )
                            .await;
                        }
                    } else {
                        let interval = ramp_duration / (-diff) as u32;
                        for _ in 0..(-diff) {
                            if !Self::sleep_abortable(interval, abort).await {
                                break;
                            }
                            if let Some((tx, _)) = vus.pop() {
                                let _ = tx.send(Instant::now());
                            }
                            current_vus -= 1;
                            self.metrics.set_active_vus(current_vus);
                        }
                        if !Self::aborted(abort) {
                            Self::sleep_abortable(
                                stage_duration.saturating_sub(ramp_duration),
                                abort,
                            )
                            .await;
                        }
                    }
                }
            }

            if Self::aborted(abort) {
                break;
            }
        }

        // Wind down: stop all remaining VUs and wait for them to exit
        for (tx, _) in &vus {
            let _ = tx.send(Instant::now());
        }
        for (_, join) in vus {
            let _ = join.await;
        }

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Constant arrival rate mode (open model)
    // ═══════════════════════════════════════════════════════════
    //
    // Standard semantics (aligned with k6 constant-arrival-rate):
    //   rate is the **total** target request rate, shared by pre_allocated_vus VUs.
    //   each VU independently sends requests at rate / pre_allocated_vus.
    #[allow(clippy::too_many_arguments)] // executor config + run control (abort/cancel); could later be folded into a RunContext
    async fn run_constant_arrival_rate(
        &self,
        scenario: &Scenario,
        rate: u32,
        duration: &str,
        ramp_up: &str,
        pre_allocated_vus: u32,
        abort: &Option<Arc<AtomicBool>>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        let load_duration = Duration::from_secs_f64(
            orbit_config::parse_duration(duration)
                .map_err(|e| EngineError::Scheduler(e.to_string()))?,
        );
        let ramp_duration = Duration::from_secs_f64(
            orbit_config::parse_duration(ramp_up)
                .map_err(|e| EngineError::Scheduler(e.to_string()))?,
        );

        // rate is the **total** target request rate, split evenly per VU (may be fractional, e.g. 0.5 req/s/VU -> one every 2s)
        let rate = rate.max(1);
        let vus = pre_allocated_vus.max(1);
        let rate_per_vu = rate as f64 / vus as f64;
        let interval_per_vu = Duration::from_secs_f64(1.0 / rate_per_vu);
        self.metrics.set_active_vus(vus);

        tracing::info!(
            "Constant arrival rate: {} req/s total, {} VUs, {:.1} req/s per VU, {}s load + {}s ramp-up",
            rate, vus, rate_per_vu, load_duration.as_secs_f64(), ramp_duration.as_secs_f64()
        );

        let mut handles = Vec::new();

        for vu_id in 0..vus {
            let scenario_name = scenario.name.clone();
            let steps = scenario.steps.clone();
            let protocol = self.protocol.clone_client();
            let codec = self.codec.clone_codec();
            let metrics = self.metrics.clone();
            let plan_vars = self.plan.variables.clone();
            let abort_task = abort.clone();
            let cancel_vu = cancel.clone();

            let handle = tokio::spawn(async move {
                let mut runner = FlowRunner::new(protocol, codec, plan_vars)
                    .with_scenario(&scenario_name)
                    .with_cancel(cancel_vu);
                let mut iteration = 0u64;

                // Phase 1: Ramp-up - raise the arrival rate gradually
                if !ramp_duration.is_zero() {
                    let ramp_start = Instant::now();
                    let ramp_deadline = ramp_start + ramp_duration;
                    // token-bucket integral scheduling: accumulate budget at the current rate and run once it reaches 1;
                    // small polling steps guarantee the ramp completes within the ramp duration (avoiding one long sleep at low rates)
                    let mut budget = 0.0f64;
                    let mut last_tick = Instant::now();

                    while Instant::now() < ramp_deadline && !Self::aborted(&abort_task) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let now = Instant::now();
                        if now >= ramp_deadline || Self::aborted(&abort_task) {
                            break;
                        }
                        let dt = now.duration_since(last_tick).as_secs_f64();
                        last_tick = now;
                        let elapsed = now.duration_since(ramp_start).as_secs_f64();
                        let ramp_ratio =
                            (elapsed / ramp_duration.as_secs_f64().max(0.001)).min(1.0);
                        budget += rate_per_vu * ramp_ratio * dt;
                        if budget >= 1.0 {
                            budget -= 1.0;
                            match runner
                                .execute_steps(&steps, vu_id as u64, iteration, false)
                                .await
                            {
                                Ok(samples) => {
                                    for s in samples {
                                        metrics.push(s);
                                    }
                                }
                                Err(e) => {
                                    metrics.push(error_sample!(
                                        scenario_name.clone(),
                                        "unknown".into(),
                                        vu_id as u64,
                                        iteration,
                                        e
                                    ));
                                }
                            }
                            iteration += 1;
                        }
                    }
                }

                // Phase 2: Steady state - constant arrival rate for the full duration
                // Catch-up scheduling: run at fixed slots of start + i*interval,
                // execution time is no longer added to the sleep; late iterations run immediately without accumulating delay.
                let load_start = Instant::now();
                let load_deadline = load_start + load_duration;
                let mut slot = load_start + interval_per_vu;

                while Instant::now() < load_deadline && !Self::aborted(&abort_task) {
                    let now = Instant::now();
                    if slot > now {
                        tokio::time::sleep_until(slot.into()).await;
                    }
                    slot += interval_per_vu;

                    match runner
                        .execute_steps(&steps, vu_id as u64, iteration, false)
                        .await
                    {
                        Ok(samples) => {
                            for s in samples {
                                metrics.push(s);
                            }
                        }
                        Err(e) => {
                            metrics.push(error_sample!(
                                scenario_name.clone(),
                                "unknown".into(),
                                vu_id as u64,
                                iteration,
                                e
                            ));
                        }
                    }
                    iteration += 1;
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }
}
