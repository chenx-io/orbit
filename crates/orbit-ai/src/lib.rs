//! # orbit-ai
//!
//! Orbit's AI capability layer: **Provider adapters + agent loop + tool contracts + proposals + credential/session storage**.
//!
//! Its positioning mirrors [`orbit_git`]: **zero Tauri / zero UI dependencies**, reusable by any UI shape (today the Tauri desktop app,
//! tomorrow native shells or a CLI). All I/O (reading/writing snapshots, executing requests, running load tests) is injected by the host through
//! [`tools::ToolHost`]; this crate only decides "what to say, whether to confirm, and whether the arguments are valid".
//!
//! ## Layers
//!
//! ```text
//! Frontend drawer --> Tauri commands (host) --> agent loop --> Provider (OpenAI-compatible / Anthropic)
//!                     │                    │                    │
//!                     │                    └─► tool contracts/validation      └─► orbit-protocol HTTP stream
//!                     └─► DataService / execution engine / session and credential files
//! ```
//!
//! ## Work modes (Ask / Agent / Plan)
//!
//! Modes are an **engine-layer** concept, not just UI copy: the tool list is filtered by mode, and a filtered-out tool is
//! rejected on the spot even if the model makes it up (see [`mode`] and [`agent`]). Writes are available only in Agent mode and go straight to the store;
//! execution tools (run request / scenario / load test) always need per-call confirmation in every mode.
//!
//! ## Secure defaults
//!
//! - Written changes are replayed to the frontend as [`proposal::Proposal`] (before/after diffs); execution operations are force-confirmed by the host;
//! - For environment secrets only the **variable names** go to the model; the values never enter the prompt;
//! - Logs never record API keys or Authorization headers.

pub mod agent;
pub mod auth;
pub mod error;
pub mod event;
pub mod fsx;
pub mod message;
pub mod mode;
pub mod plan;
pub mod prompt;
pub mod proposal;
pub mod provider;
pub mod session;
pub mod syntax;
pub mod tools;
pub mod transport;

pub use agent::{Agent, AgentLimits, AgentOutcome, AgentRunRequest, Approval, Approver};
/// Re-export of the `async_trait` macro so hosts implementing [`tools::ToolHost`] / [`agent::Approver`] do not need
/// to declare the dependency themselves (which also avoids version drift).
pub use async_trait::async_trait;
pub use auth::{
    default_credentials_path, AiCredential, CredentialInput, CredentialStore, CredentialView,
};
pub use error::{AiError, AiResult};
pub use event::{AiEvent, EventBus, EventSink, ToolStatus};
pub use message::{ChatMessage, ProviderDelta, ProviderTurnEnd, Role, StopReason, ToolCall, Usage};
pub use mode::AiMode;
pub use plan::{PlanArtifact, PlanStep};
pub use prompt::{ContextSummary, Language, Reference, Selection};
pub use proposal::{DiffKind, FieldDiff, Proposal, ProposalAction};
pub use provider::{
    build_provider, Provider, ProviderConfig, ProviderKind, TurnRequest, DEFAULT_TEMPERATURE,
};
pub use session::{AiSession, SessionStore, SessionSummary};
pub use tools::{ToolHost, ToolKind, ToolOutcome, ToolSpec};
