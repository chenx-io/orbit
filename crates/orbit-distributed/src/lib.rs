//! # orbit-distributed
//!
//! Distributed load-testing system - Controller + Agent architecture (dual mode).
//!
//! ## Architecture
//! ```text
//! Mode A (server): Controller --gRPC--> Agent (the agent listens, the controller connects)
//! Mode B (client): Agent --gRPC(bidi)--> Controller (the agent connects out)
//! ```
//!
//! ## Core features
//! - Controller: dual-mode attachment, AgentRegistry (state machine), dynamic partitioning, metrics merging
//! - Agent: dual-mode registration, local execution, HDR metrics reporting, heartbeat, system resource sampling
//! - MetricsMerger: exact HDR histogram merging (not averaged percentiles)

pub mod agent;
pub mod controller;
pub mod conv;
pub mod merger;
pub mod registry;
pub mod resource;
pub mod types;

/// gRPC reporting protocol code generated from `proto/controller.proto` by tonic-build.
#[allow(unsafe_code, missing_docs, clippy::all)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/orbit.distributed.rs"));
}

pub use agent::{run_client, AgentConfig, AgentCore, AgentServer};
pub use controller::{AddAgentOutcome, Controller, ControllerState};
pub use merger::MetricsMerger;
pub use registry::{AgentRegistry, RegistryEvent};
pub use resource::ResourceCollector;
pub use types::*;
