//! # orbit-config
//!
//! YAML config parser. Supports:
//! - Structured TestPlan definitions
//! - Variable interpolation `{{var}}` (recommended) / `${var}`, `${env:var}` (compatible) and dynamic values `{{$cat.method}}`
//! - Multi-environment merging
//! - Config validation with friendly error messages
//! - Postman/curl/OpenAPI/HAR/JMeter/k6 import
//!
//! ## Module layout
//! - [`model`]: pure serde model definitions (TestPlan / Scenario / Step / RequestSpec / Check / Extraction)
//! - [`parse`]: YAML parsing entry point, duration parsing, environment merging
//! - [`template`]: variable interpolation + arithmetic expression evaluation + dynamic values
//! - [`importer`]: external format importers
//! - [`importer_openapi`]: OpenAPI endpoint parsing (compatibility entry point)
//!
//! The public API stays stable through the top-level re-exports in lib.rs.

pub mod exchange;
pub mod importer_openapi;
pub mod server;

mod error;
mod model;
mod parse;
mod template;

pub use error::ConfigError;
pub use model::action::{
    merge_pre_actions, normalize_actions, normalize_pre_actions, ColumnVar, RequestAction,
};
pub use model::check::{Check, CheckKind, DbTarget, RetryPolicy};
pub use model::datasource::{DataSourceConfig, DataSourceKind};
pub use model::extract::{Extraction, ExtractionSource};
pub use model::plan::{Environment, Executor, OnError, RampMode, RampingStage, Scenario, TestPlan};
pub use model::request::{
    GraphqlConfig, GrpcConfig, HttpRequestConfig, MessageSpec, PayloadType, RequestSpec, SseConfig,
    TcpConfig, UdpConfig, WebSocketConfig,
};
pub use model::step::Step;
pub use model::template::{resolve_action_refs, ActionTemplate};
pub use parse::{apply_environment, from_str, parse_duration};
pub use server::ServerConfig;
pub use template::interpolate;
