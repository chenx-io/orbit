//! Test plan / environment / scenario / executor models (pure serde definitions, no logic)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::datasource::DataSourceConfig;
use super::step::Step;

/// Top-level test plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPlan {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    /// Datasource configs (optional): the CLI/server registers connection pools before running cases, for db/redis assertions to reference
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub datasources: Vec<DataSourceConfig>,
    pub scenarios: Vec<Scenario>,
    /// Global thresholds - pass/fail conditions for the test
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thresholds: Vec<String>,
}

/// Environment config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    #[serde(default)]
    pub variables: HashMap<String, String>,
}

/// Load-testing scenario / automation scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub name: String,
    pub executor: Executor,
    pub steps: Vec<Step>,
    /// Error handling: stop (halt on error) / continue (ignore errors and keep going)
    #[serde(default)]
    pub on_error: OnError,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            name: String::new(),
            executor: Executor::Sequential { iterations: 1 },
            steps: vec![],
            on_error: Default::default(),
        }
    }
}

/// Error handling strategy
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    #[default]
    Stop,
    Continue,
}

/// Executor type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Executor {
    /// Sequential execution (automation scenarios)
    #[serde(rename = "sequential")]
    Sequential {
        #[serde(default = "default_iterations")]
        iterations: u32,
    },
    /// Constant VU mode
    #[serde(rename = "constant-vus")]
    ConstantVus {
        vus: u32,
        duration: String,
        #[serde(default = "default_ramp_up")]
        ramp_up: String,
    },
    /// Ramping VU mode (aligned with k6 ramping-vus, extended with per-stage ramp modes):
    /// initial VUs + multiple stages, where each stage can "jump instantly", "ramp up over the duration" or "ramp like JMeter then hold",
    /// with a maximum VU cap.
    #[serde(rename = "ramping-vus")]
    RampingVus {
        /// Starting number of VUs
        #[serde(default)]
        start_vus: u32,
        /// Maximum VU cap (0 = unlimited; any stage whose target exceeds it is capped here)
        #[serde(default)]
        max_vus: u32,
        /// Ramping stage config (target/duration/ramp/ramp_up)
        stages: Vec<RampingStage>,
    },
    /// Constant arrival rate mode (open model)
    #[serde(rename = "constant-arrival-rate")]
    ConstantArrivalRate {
        /// Requests per second (RPS)
        rate: u32,
        /// Duration
        duration: String,
        /// Ramp-up time
        #[serde(default = "default_ramp_up")]
        ramp_up: String,
        /// Pre-allocated number of VUs
        #[serde(default)]
        pre_allocated_vus: u32,
    },
}

/// Ramping stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RampingStage {
    /// Stage target number of VUs
    pub target: u32,
    /// Stage duration
    pub duration: String,
    /// Ramp mode: instant = jump to target at stage start; gradual = change gradually over the duration
    #[serde(default)]
    pub ramp: RampMode,
    /// Ramp-up duration when ramp=jmeter (<= duration; default = duration, i.e. pure ramp, equivalent to k6 semantics)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_up: Option<String>,
}

/// Ramp mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RampMode {
    /// Ramp up gradually during the stage, reaching target at the end (k6 semantics, linear change)
    #[default]
    Gradual,
    /// Jump to the target VU count at stage start, then hold until the stage ends
    Instant,
    /// JMeter Thread Group semantics: ramp linearly to target within ramp_up, then hold until the stage ends
    Jmeter,
}

fn default_iterations() -> u32 {
    1
}
fn default_ramp_up() -> String {
    "0s".to_string()
}
