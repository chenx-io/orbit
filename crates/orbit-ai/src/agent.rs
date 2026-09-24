//! Agent loop: one user turn -> multiple model calls + tool executions -> an event stream.
//!
//! ## Mode and approval (all enforced here, never left to the prompt's goodwill)
//!
//! 1. **Tool visibility**: the tool list sent to the model is filtered by [`AiMode::allows`] first - Ask / Plan modes
//!    never even see write or execute tools;
//! 2. **Execution admission**: if the model invents a filtered-out tool name, it is denied on the spot and the reason is fed back (anti-hallucination),
//!    and never reaches the host;
//! 3. **Write operations**: visible only in Agent mode, applied straight to the store (changes are replayed as diffs via [`AIEvent::ProposalReady`]);
//! 4. **Execution tools**: confirmed one by one through [`Approver`] in every mode - writes only touch a local, revertible snapshot,
//!    while execution has irreversible effects on **external systems**; the two risks are not symmetric.
//!
//! `run` deliberately returns [`AgentOutcome`] (not a `Result`): even when it fails midway, the host still needs
//! the "messages produced so far" and the "partial text" for persistence and display, so the error goes into `outcome.error`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

use crate::error::AiError;
use crate::event::{AiEvent, EventSink, ToolStatus};
use crate::message::{ChatMessage, ProviderDelta, StopReason, ToolCall, Usage};
use crate::mode::AiMode;
use crate::provider::{Provider, TurnRequest};
use crate::session::trim_history;
use crate::tools::catalog::kind_of;
use crate::tools::validate::clip_text;
use crate::tools::{ToolHost, ToolKind, ToolSpec};

/// The user's approval decision (currently only used for [`ToolKind::Execute`]).
#[derive(Debug, Clone)]
pub struct Approval {
    /// Whether execution is allowed.
    pub allow: bool,
    /// Denial reason / extra note (fed back to the model so it tries a different approach).
    pub note: Option<String>,
}

impl Approval {
    /// Allow.
    pub fn allow() -> Self {
        Self {
            allow: true,
            note: None,
        }
    }

    /// Deny with a reason.
    pub fn deny(note: impl Into<String>) -> Self {
        Self {
            allow: false,
            note: Some(note.into()),
        }
    }
}

/// Approval callback (host-implemented: the Tauri side waits for a frontend click).
#[async_trait]
pub trait Approver: Send + Sync {
    /// Ask the user to approve one tool call. When `cancel` fires it should return a denial as soon as possible.
    async fn request(
        &self,
        call: &ToolCall,
        kind: ToolKind,
        cancel: &CancellationToken,
    ) -> Approval;
}

/// Auto-approve (only for tests and "no UI" scenarios; never for production paths that execute tools).
pub struct AutoApprover;

#[async_trait]
impl Approver for AutoApprover {
    async fn request(&self, _: &ToolCall, _: ToolKind, _: &CancellationToken) -> Approval {
        Approval::allow()
    }
}

/// Loop limits (so the model cannot spin forever between tools).
#[derive(Debug, Clone, Copy)]
pub struct AgentLimits {
    /// Maximum number of model round-trips within a single user turn.
    pub max_rounds: usize,
    /// Character cap for a single tool result fed back to the model.
    pub max_tool_output_chars: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            max_tool_output_chars: 8_000,
        }
    }
}

/// Inputs for one `run`.
pub struct AgentRunRequest {
    /// Turn id (the frontend uses it to merge streaming deltas into one message).
    pub turn_id: String,
    /// Model name.
    pub model: String,
    /// System prompt.
    pub system: String,
    /// History messages (including the new user message of this turn).
    pub messages: Vec<ChatMessage>,
    /// Full candidate tool set (filtered again inside the loop by [`AgentRunRequest::mode`]).
    pub tools: Vec<ToolSpec>,
    /// Work mode (Ask / Agent / Plan): decides tool visibility and the write policy.
    pub mode: AiMode,
    /// Temperature.
    pub temperature: f32,
    /// Maximum output tokens per round.
    pub max_tokens: u32,
    /// History trimming budget (characters).
    pub context_budget: usize,
}

/// Result of one `run`.
pub struct AgentOutcome {
    /// Turn id.
    pub turn_id: String,
    /// Messages added this turn (in order; they can be appended to the session as-is).
    pub messages: Vec<ChatMessage>,
    /// Accumulated usage.
    pub usage: Usage,
    /// Stop reason.
    pub stop_reason: StopReason,
    /// Whether the run ended early because the maximum number of rounds was reached.
    pub truncated: bool,
    /// Error info (`None` = completed normally).
    pub error: Option<AiError>,
}

/// The agent loop executor.
pub struct Agent {
    provider: Box<dyn Provider>,
    host: Arc<dyn ToolHost>,
    approver: Arc<dyn Approver>,
    sink: EventSink,
    limits: AgentLimits,
}

impl Agent {
    /// Assemble.
    pub fn new(
        provider: Box<dyn Provider>,
        host: Arc<dyn ToolHost>,
        approver: Arc<dyn Approver>,
        sink: EventSink,
        limits: AgentLimits,
    ) -> Self {
        Self {
            provider,
            host,
            approver,
            sink,
            limits,
        }
    }

    /// Run one full user turn.
    pub async fn run(&self, req: AgentRunRequest, cancel: CancellationToken) -> AgentOutcome {
        let mut outcome = AgentOutcome {
            turn_id: req.turn_id.clone(),
            messages: Vec::new(),
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
            truncated: false,
            error: None,
        };
        (self.sink)(AiEvent::TurnStarted {
            turn_id: req.turn_id.clone(),
        });

        // Mode filtering: tools outside the current mode never even have their schema sent to the model
        let tools: Vec<ToolSpec> = req
            .tools
            .iter()
            .filter(|t| req.mode.allows(t.kind))
            .cloned()
            .collect();
        let kinds: HashMap<String, ToolKind> =
            tools.iter().map(|t| (t.name.clone(), t.kind)).collect();
        let mut history = trim_history(&req.messages, req.context_budget);

        for round in 0..self.limits.max_rounds.max(1) {
            if cancel.is_cancelled() {
                outcome.error = Some(AiError::Cancelled);
                break;
            }

            let (tx, mut rx) = unbounded_channel::<ProviderDelta>();
            let acc = Arc::new(Mutex::new((String::new(), String::new())));
            let sink = self.sink.clone();
            let turn_id = req.turn_id.clone();
            let consumer = tokio::spawn({
                let acc = acc.clone();
                async move {
                    while let Some(delta) = rx.recv().await {
                        match delta {
                            ProviderDelta::Text(t) => {
                                acc.lock().unwrap_or_else(|e| e.into_inner()).0.push_str(&t);
                                sink(AiEvent::TextDelta {
                                    turn_id: turn_id.clone(),
                                    delta: t,
                                });
                            }
                            ProviderDelta::Reasoning(t) => {
                                acc.lock().unwrap_or_else(|e| e.into_inner()).1.push_str(&t);
                                sink(AiEvent::ReasoningDelta {
                                    turn_id: turn_id.clone(),
                                    delta: t,
                                });
                            }
                            // Tool-call argument deltas are aggregated inside the Provider; the event layer does not emit per-fragment updates
                            ProviderDelta::ToolCallDelta { .. } | ProviderDelta::Usage(_) => {}
                        }
                    }
                }
            });

            let turn_req = TurnRequest {
                model: &req.model,
                system: &req.system,
                messages: &history,
                tools: &tools,
                max_tokens: req.max_tokens,
                temperature: req.temperature,
            };
            let streamed = self
                .provider
                .stream_turn(turn_req, tx, cancel.clone())
                .await;
            let _ = consumer.await;
            let (text, reasoning) = {
                let g = acc.lock().unwrap_or_else(|e| e.into_inner());
                (g.0.clone(), g.1.clone())
            };

            let turn_end = match streamed {
                Ok(end) => end,
                Err(err) => {
                    if !text.is_empty() {
                        let mut partial = ChatMessage::assistant(text);
                        if !reasoning.is_empty() {
                            partial.reasoning = Some(reasoning);
                        }
                        outcome.messages.push(partial);
                    }
                    (self.sink)(AiEvent::Error {
                        turn_id: Some(req.turn_id.clone()),
                        message: err.user_message(),
                        retryable: err.retryable(),
                    });
                    outcome.error = Some(err);
                    break;
                }
            };
            outcome.usage.add(turn_end.usage);

            let mut assistant = ChatMessage::assistant(text);
            assistant.tool_calls = turn_end.tool_calls.clone();
            if !reasoning.is_empty() {
                assistant.reasoning = Some(reasoning);
            }
            // An empty round (no text, no tool calls) **must not enter the request history**: strict implementations reject it outright
            // (DeepSeek 400 `Invalid assistant message: content or tool_calls must be set`),
            // and it is persisted with the session, making every later round of that session fail.
            // But the chain of thought must still be shown (the DeepSeek reasoner's reasoning is valuable), so an empty
            // round carrying reasoning stays in the outcome (UI / persistence) and is simply not sent back to the model.
            let empty_turn = assistant.text.is_empty() && assistant.tool_calls.is_empty();
            if !empty_turn {
                history.push(assistant.clone());
            }
            if !empty_turn || assistant.reasoning.is_some() {
                outcome.messages.push(assistant);
            }

            if turn_end.tool_calls.is_empty() {
                outcome.stop_reason = turn_end.stop_reason.normalized();
                break;
            }

            for call in turn_end.tool_calls.clone() {
                if cancel.is_cancelled() {
                    outcome.error = Some(AiError::Cancelled);
                    break;
                }
                let catalog_kind = kind_of(&call.name);
                let kind = kinds
                    .get(&call.name)
                    .copied()
                    .or(catalog_kind)
                    .unwrap_or(ToolKind::Read);

                // Mode admission: a filtered-out tool is never allowed to run, even if the model "remembers" it.
                // This is the real line of defense for read-only modes (Ask / Plan) - the prompt only informs, this enforces.
                if let Some(real) = catalog_kind.filter(|k| !req.mode.allows(*k)) {
                    self.emit_tool(
                        &req.turn_id,
                        &call,
                        real,
                        ToolStatus::Denied,
                        Some(format!("unavailable in {} mode", req.mode.as_tag())),
                    );
                    let msg = format!(
                        "Tool {} is unavailable in the current mode ({}). {}",
                        call.name,
                        req.mode.as_tag(),
                        mode_refusal_hint(req.mode)
                    );
                    (self.sink)(AiEvent::ToolResult {
                        turn_id: req.turn_id.clone(),
                        call_id: call.id.clone(),
                        ok: false,
                        summary: msg.clone(),
                        payload: serde_json::Value::Null,
                        elapsed_ms: 0,
                    });
                    let m =
                        ChatMessage::tool_result(call.id.clone(), call.name.clone(), msg, false);
                    history.push(m.clone());
                    outcome.messages.push(m);
                    continue;
                }

                // Unknown tool: feed the error back so the model can correct itself without aborting the turn
                if catalog_kind.is_none() {
                    self.emit_tool(
                        &req.turn_id,
                        &call,
                        kind,
                        ToolStatus::Failed,
                        Some("unknown tool".into()),
                    );
                    let msg =
                        "No such tool: choose one from the list of available tools.".to_string();
                    (self.sink)(AiEvent::ToolResult {
                        turn_id: req.turn_id.clone(),
                        call_id: call.id.clone(),
                        ok: false,
                        summary: msg.clone(),
                        payload: serde_json::Value::Null,
                        elapsed_ms: 0,
                    });
                    let m =
                        ChatMessage::tool_result(call.id.clone(), call.name.clone(), msg, false);
                    history.push(m.clone());
                    outcome.messages.push(m);
                    continue;
                }

                // Only execution tools need confirmation: in Agent mode writes go straight to the store (revertible),
                // whereas execution has irreversible effects on external systems.
                if kind.always_confirms() {
                    self.emit_tool(&req.turn_id, &call, kind, ToolStatus::PendingApproval, None);
                    let decision = self.approver.request(&call, kind, &cancel).await;
                    if !decision.allow {
                        let note = decision
                            .note
                            .clone()
                            .unwrap_or_else(|| "the user denied this operation".to_string());
                        self.emit_tool(
                            &req.turn_id,
                            &call,
                            kind,
                            ToolStatus::Denied,
                            Some(note.clone()),
                        );
                        (self.sink)(AiEvent::ToolResult {
                            turn_id: req.turn_id.clone(),
                            call_id: call.id.clone(),
                            ok: false,
                            summary: note.clone(),
                            payload: serde_json::Value::Null,
                            elapsed_ms: 0,
                        });
                        let m = ChatMessage::tool_result(
                            call.id.clone(),
                            call.name.clone(),
                            format!("User denied: {note}. Do not repeat the same call; explain why or propose an alternative instead."),
                            false,
                        );
                        history.push(m.clone());
                        outcome.messages.push(m);
                        continue;
                    }
                }

                self.emit_tool(&req.turn_id, &call, kind, ToolStatus::Running, None);
                let started = Instant::now();
                let result = self.host.call(&call.name, &call.arguments).await;
                let elapsed_ms = started.elapsed().as_millis() as u64;

                match result {
                    Ok(o) => {
                        (self.sink)(AiEvent::ToolResult {
                            turn_id: req.turn_id.clone(),
                            call_id: call.id.clone(),
                            ok: o.ok,
                            summary: o.summary.clone(),
                            payload: clip_value(&o.payload, self.limits.max_tool_output_chars),
                            elapsed_ms,
                        });
                        if let Some(proposal) = &o.proposal {
                            (self.sink)(AiEvent::ProposalReady {
                                turn_id: req.turn_id.clone(),
                                call_id: call.id.clone(),
                                proposal: proposal.clone(),
                            });
                        }
                        if let Some(plan) = &o.plan {
                            (self.sink)(AiEvent::PlanReady {
                                turn_id: req.turn_id.clone(),
                                call_id: call.id.clone(),
                                plan: plan.clone(),
                            });
                        }
                        self.emit_tool(
                            &req.turn_id,
                            &call,
                            kind,
                            if o.ok {
                                ToolStatus::Completed
                            } else {
                                ToolStatus::Failed
                            },
                            None,
                        );
                        let content = clip_text(&o.summary, self.limits.max_tool_output_chars);
                        let m = ChatMessage::tool_result(
                            call.id.clone(),
                            call.name.clone(),
                            content,
                            o.ok,
                        );
                        history.push(m.clone());
                        outcome.messages.push(m);
                    }
                    Err(err) => {
                        let msg = err.user_message();
                        (self.sink)(AiEvent::ToolResult {
                            turn_id: req.turn_id.clone(),
                            call_id: call.id.clone(),
                            ok: false,
                            summary: msg.clone(),
                            payload: serde_json::Value::Null,
                            elapsed_ms,
                        });
                        self.emit_tool(
                            &req.turn_id,
                            &call,
                            kind,
                            ToolStatus::Failed,
                            Some(msg.clone()),
                        );
                        let m = ChatMessage::tool_result(
                            call.id.clone(),
                            call.name.clone(),
                            format!("Execution failed: {msg}"),
                            false,
                        );
                        history.push(m.clone());
                        outcome.messages.push(m);
                    }
                }
            }

            // Last round: stop asking the model and mark the run as truncated
            if round + 1 == self.limits.max_rounds.max(1) {
                outcome.truncated = true;
            }
        }

        (self.sink)(AiEvent::TurnFinished {
            turn_id: req.turn_id.clone(),
            stop_reason: outcome.stop_reason,
            usage: outcome.usage,
            truncated: outcome.truncated,
        });
        outcome
    }

    fn emit_tool(
        &self,
        turn_id: &str,
        call: &ToolCall,
        kind: ToolKind,
        status: ToolStatus,
        note: Option<String>,
    ) {
        (self.sink)(AiEvent::ToolCall {
            turn_id: turn_id.to_string(),
            call_id: call.id.clone(),
            name: call.name.clone(),
            kind,
            arguments: call.arguments.clone(),
            status,
            note,
        });
    }
}

/// Extra guidance when a mode denies a call (fed back to the model to steer it onto a legal path).
fn mode_refusal_hint(mode: AiMode) -> &'static str {
    match mode {
        AiMode::Ask => {
            "This mode can only read and explain. Instead, describe what should be done and suggest that the user switch to Agent mode to execute it (for multi-step work, switch to Plan mode first to lay out a plan)."
        }
        AiMode::Plan => {
            "This mode can only read and plan. Keep investigating and submit your plan through present_plan; do not modify data or start execution."
        }
        AiMode::Agent => "Choose from the list of available tools.",
    }
}

/// Clip JSON to a character budget (string fields are shortened first).
fn clip_value(value: &serde_json::Value, max_chars: usize) -> serde_json::Value {
    let text = value.to_string();
    if text.chars().count() <= max_chars {
        return value.clone();
    }
    serde_json::json!({ "_truncated": clip_text(&text, max_chars) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventBus;
    use crate::message::{ProviderTurnEnd, StopReasonRaw};
    use crate::proposal::{Proposal, ProposalAction};
    use crate::tools::catalog;
    use crate::tools::ToolOutcome;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Scripted fake Provider: returns preset text and tool calls per round (text is emitted as deltas).
    struct FakeProvider {
        /// Remaining rounds (taken from the front); each item = (text, tool calls).
        rounds: Mutex<std::collections::VecDeque<(String, Vec<ToolCall>)>>,
        /// Number of history messages carried by each round's request (asserts whether context accumulates); an external handle is kept so it can be asserted on.
        message_counts: Arc<Mutex<Vec<usize>>>,
        /// Tool names carried by each round's request (asserts whether mode filtering takes effect).
        tools_seen: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl FakeProvider {
        fn new(rounds: Vec<(String, Vec<ToolCall>)>) -> Self {
            Self {
                rounds: Mutex::new(rounds.into_iter().collect()),
                message_counts: Arc::new(Mutex::new(vec![])),
                tools_seen: Arc::new(Mutex::new(vec![])),
            }
        }

        /// External handle: read the history message count of each round.
        fn counter(&self) -> Arc<Mutex<Vec<usize>>> {
            self.message_counts.clone()
        }

        /// External handle: read the tool names visible in each round.
        fn tools_handle(&self) -> Arc<Mutex<Vec<Vec<String>>>> {
            self.tools_seen.clone()
        }
    }

    #[async_trait]
    impl Provider for FakeProvider {
        fn id(&self) -> &str {
            "fake"
        }
        fn kind(&self) -> crate::provider::ProviderKind {
            crate::provider::ProviderKind::OpenAi
        }
        async fn stream_turn(
            &self,
            req: TurnRequest<'_>,
            sink: crate::provider::DeltaSink,
            _cancel: CancellationToken,
        ) -> crate::error::AiResult<ProviderTurnEnd> {
            self.message_counts.lock().unwrap().push(req.messages.len());
            self.tools_seen
                .lock()
                .unwrap()
                .push(req.tools.iter().map(|t| t.name.clone()).collect());
            let (text, calls) = self
                .rounds
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| (String::new(), vec![]));
            if !text.is_empty() {
                let _ = sink.send(ProviderDelta::Text(text.clone()));
            }
            for (index, c) in calls.iter().enumerate() {
                let _ = sink.send(ProviderDelta::ToolCallDelta {
                    index,
                    id: Some(c.id.clone()),
                    name: Some(c.name.clone()),
                    arguments_delta: c.arguments.to_string(),
                });
            }
            Ok(ProviderTurnEnd {
                tool_calls: calls,
                stop_reason: StopReasonRaw(Some(if text.is_empty() {
                    "tool_use".into()
                } else {
                    "end_turn".into()
                })),
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
            })
        }
    }

    #[derive(Default)]
    struct RecordingHost {
        calls: Mutex<Vec<(String, serde_json::Value)>>,
        fail_next: AtomicBool,
    }

    #[async_trait]
    impl ToolHost for RecordingHost {
        async fn call(
            &self,
            name: &str,
            args: &serde_json::Value,
        ) -> crate::error::AiResult<ToolOutcome> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_string(), args.clone()));
            if self.fail_next.swap(false, Ordering::SeqCst) {
                return Err(AiError::Tool {
                    name: name.to_string(),
                    message: "the server returned 500".into(),
                });
            }
            if name == "create_request" {
                return Ok(ToolOutcome::proposed(
                    "request written",
                    Proposal::new(
                        name,
                        "Create request",
                        "collection User Center",
                        ProposalAction::CreateRequest {
                            collection_id: "c1".into(),
                            parent_id: None,
                        },
                        None,
                        json!({"name":"Login","method":"POST","url":"https://x/login"}),
                    ),
                ));
            }
            if name == "present_plan" {
                let plan = crate::plan::PlanArtifact::from_args(args)?;
                return Ok(ToolOutcome::planned(
                    "plan submitted",
                    plan.stamped(1, 0, 1_700_000_000_000),
                ));
            }
            Ok(ToolOutcome::read("ok", json!({"items":[]})))
        }
    }

    struct CountingApprover {
        calls: AtomicUsize,
        allow: bool,
    }

    #[async_trait]
    impl Approver for CountingApprover {
        async fn request(&self, _: &ToolCall, _: ToolKind, _: &CancellationToken) -> Approval {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.allow {
                Approval::allow()
            } else {
                Approval::deny("the user clicked deny")
            }
        }
    }

    fn tools() -> Vec<ToolSpec> {
        catalog::all_tools()
    }

    fn base_req(msgs: Vec<ChatMessage>, mode: AiMode) -> AgentRunRequest {
        AgentRunRequest {
            turn_id: "turn-1".into(),
            model: "test-model".into(),
            system: "sys".into(),
            messages: msgs,
            tools: tools(),
            mode,
            temperature: 0.2,
            max_tokens: 1024,
            context_budget: 10_000,
        }
    }

    fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: args,
        }
    }

    fn agent(
        provider: FakeProvider,
        host: Arc<RecordingHost>,
        approver: Arc<dyn Approver>,
        bus: EventBus,
    ) -> Agent {
        Agent::new(
            Box::new(provider),
            host,
            approver,
            bus.sink(None),
            AgentLimits::default(),
        )
    }

    #[tokio::test]
    async fn read_tool_runs_without_approval_and_loop_finishes() {
        let provider = FakeProvider::new(vec![
            (String::new(), vec![call("c1", "list_requests", json!({}))]),
            ("3 requests exist".to_string(), vec![]),
        ]);
        let host = Arc::new(RecordingHost::default());
        let approver = Arc::new(CountingApprover {
            calls: AtomicUsize::new(0),
            allow: true,
        });
        let bus = EventBus::new();
        let a = agent(provider, host.clone(), approver.clone(), bus.clone());

        let outcome = a
            .run(
                base_req(
                    vec![ChatMessage::user("which requests are there")],
                    AiMode::Agent,
                ),
                CancellationToken::new(),
            )
            .await;

        assert!(outcome.error.is_none());
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(
            approver.calls.load(Ordering::SeqCst),
            0,
            "read-only tools must not ask for approval"
        );
        assert_eq!(host.calls.lock().unwrap().len(), 1);
        // Messages: assistant (with tool calls) + tool result + assistant (text)
        assert_eq!(outcome.messages.len(), 3);
        assert_eq!(outcome.messages[2].text, "3 requests exist");
        // Each of the two rounds counts 1 input and 1 output
        assert_eq!(outcome.usage.total(), 4);

        let events = bus.drain();
        assert!(events
            .iter()
            .any(|e| matches!(e, AiEvent::ToolResult { ok: true, .. })));
        assert!(events.iter().any(|e| matches!(
            e,
            AiEvent::TurnFinished {
                truncated: false,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn agent_mode_applies_writes_directly_without_approval() {
        let provider = FakeProvider::new(vec![
            (
                String::new(),
                vec![call(
                    "c1",
                    "create_request",
                    json!({"collectionId":"c1","request":{"method":"POST","url":"https://x"}}),
                )],
            ),
            ("login request created".to_string(), vec![]),
        ]);
        let host = Arc::new(RecordingHost::default());
        let approver = Arc::new(CountingApprover {
            calls: AtomicUsize::new(0),
            allow: true,
        });
        let bus = EventBus::new();
        let a = agent(provider, host.clone(), approver.clone(), bus.clone());

        let outcome = a
            .run(
                base_req(
                    vec![ChatMessage::user("create a login request")],
                    AiMode::Agent,
                ),
                CancellationToken::new(),
            )
            .await;

        assert_eq!(
            approver.calls.load(Ordering::SeqCst),
            0,
            "Agent mode permissions: writes need no per-call confirmation"
        );
        assert_eq!(
            host.calls.lock().unwrap().len(),
            1,
            "a write must actually reach the store"
        );
        let events = bus.drain();
        let proposals = events
            .iter()
            .filter(|e| matches!(e, AiEvent::ProposalReady { .. }))
            .count();
        assert_eq!(
            proposals, 1,
            "one persisted change should be replayed so the frontend can show the diff"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            AiEvent::ToolCall {
                status: ToolStatus::Completed,
                ..
            }
        )));
        assert!(outcome.error.is_none());
    }

    #[tokio::test]
    async fn write_and_execute_tools_are_refused_outside_agent_mode() {
        // The prompt only "informs"; this verifies enforcement: even a filtered-out tool name invented by the model must not reach the host
        let cases = [
            (
                AiMode::Ask,
                "create_request",
                json!({"collectionId":"c1","request":{"method":"POST","url":"https://x"}}),
                "Agent",
            ),
            (
                AiMode::Ask,
                "run_request",
                json!({"requestId":"r1"}),
                "Agent",
            ),
            (
                AiMode::Plan,
                "create_scenario",
                json!({"scenario":{"name":"case"}}),
                "present_plan",
            ),
        ];
        for (mode, name, args, expected_hint) in cases {
            let provider = FakeProvider::new(vec![
                (String::new(), vec![call("c1", name, args)]),
                (
                    "Understood, I'll explain the approach instead".to_string(),
                    vec![],
                ),
            ]);
            let host = Arc::new(RecordingHost::default());
            let bus = EventBus::new();
            let a = agent(provider, host.clone(), Arc::new(AutoApprover), bus.clone());

            let outcome = a
                .run(
                    base_req(vec![ChatMessage::user("change something")], mode),
                    CancellationToken::new(),
                )
                .await;

            assert!(
                host.calls.lock().unwrap().is_empty(),
                "{mode:?} mode must not execute {name}"
            );
            let tool_msg = outcome
                .messages
                .iter()
                .find(|m| m.role == crate::message::Role::Tool)
                .expect("a tool failure result is expected");
            assert_eq!(tool_msg.tool_ok, Some(false));
            assert!(tool_msg.text.contains("unavailable"), "{}", tool_msg.text);
            assert!(
                tool_msg.text.contains(expected_hint),
                "the denial message must point to the correct way out for this mode (expected to contain {expected_hint}): {}",
                tool_msg.text
            );
            assert!(
                outcome.error.is_none(),
                "a denied tool must not abort the turn"
            );
            assert!(bus.drain().iter().any(|e| matches!(
                e,
                AiEvent::ToolCall {
                    status: ToolStatus::Denied,
                    ..
                }
            )));
        }
    }

    #[tokio::test]
    async fn plan_mode_records_plan_and_emits_plan_ready() {
        let provider = FakeProvider::new(vec![
            (
                String::new(),
                vec![call(
                    "c1",
                    "present_plan",
                    json!({
                        "title": "Add automated cases for the login request",
                        "steps": [
                            { "title": "Read the login request definition", "detail": "get_request r1" },
                            { "title": "Create case: login failure path" }
                        ],
                        "notes": ["Check that the test environment is available first"]
                    }),
                )],
            ),
            (
                "Plan submitted; click Start implementing and I will get to work".to_string(),
                vec![],
            ),
        ]);
        let host = Arc::new(RecordingHost::default());
        let bus = EventBus::new();
        let a = agent(provider, host.clone(), Arc::new(AutoApprover), bus.clone());

        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("help me plan this")], AiMode::Plan),
                CancellationToken::new(),
            )
            .await;

        assert!(outcome.error.is_none());
        assert_eq!(
            host.calls.lock().unwrap().len(),
            1,
            "present_plan should be recorded by the host"
        );
        let plan = bus
            .drain()
            .into_iter()
            .find_map(|e| match e {
                AiEvent::PlanReady { plan, .. } => Some(plan),
                _ => None,
            })
            .expect("Plan mode must emit a PlanReady event");
        assert_eq!(plan.title, "Add automated cases for the login request");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.revision, 1);
    }

    #[tokio::test]
    async fn tool_list_is_filtered_by_mode() {
        let ask = seen_tools(AiMode::Ask).await;
        assert!(
            ask.iter().any(|n| n == "list_requests"),
            "read-only tools should be visible"
        );
        assert!(
            !ask.iter().any(|n| n == "create_request"),
            "Ask must not see write tools"
        );
        assert!(
            !ask.iter().any(|n| n == "run_request"),
            "Ask must not see execute tools"
        );
        assert!(
            !ask.iter().any(|n| n == "present_plan"),
            "Ask must not see planning tools"
        );

        let plan = seen_tools(AiMode::Plan).await;
        assert!(plan.iter().any(|n| n == "list_collections"));
        assert!(
            plan.iter().any(|n| n == "present_plan"),
            "Plan must be able to submit a plan"
        );
        assert!(!plan.iter().any(|n| n == "update_request"));
        assert!(!plan.iter().any(|n| n == "run_scenario"));

        let agent_tools = seen_tools(AiMode::Agent).await;
        assert!(agent_tools.iter().any(|n| n == "create_request"));
        assert!(agent_tools.iter().any(|n| n == "run_load_test"));
        assert!(
            !agent_tools.iter().any(|n| n == "present_plan"),
            "plans are produced only in Plan mode"
        );
    }

    /// Run one empty turn and collect the tool names sent to the model in that mode.
    async fn seen_tools(mode: AiMode) -> Vec<String> {
        let provider = FakeProvider::new(vec![("done".to_string(), vec![])]);
        let seen = provider.tools_handle();
        let a = agent(
            provider,
            Arc::new(RecordingHost::default()),
            Arc::new(AutoApprover),
            EventBus::new(),
        );
        a.run(
            base_req(vec![ChatMessage::user("hi")], mode),
            CancellationToken::new(),
        )
        .await;
        let names = seen.lock().unwrap().first().cloned().unwrap_or_default();
        names
    }

    #[tokio::test]
    async fn execute_tools_still_ask_even_in_agent_mode() {
        // Writes are revertible (they only change a local snapshot), while execution has irreversible effects on **external systems** -> the two risks are not symmetric
        let provider = FakeProvider::new(vec![
            (
                String::new(),
                vec![call("c1", "run_request", json!({"requestId":"r1"}))],
            ),
            ("done running".to_string(), vec![]),
        ]);
        let approver = Arc::new(CountingApprover {
            calls: AtomicUsize::new(0),
            allow: true,
        });
        let host = Arc::new(RecordingHost::default());
        let a = agent(provider, host.clone(), approver.clone(), EventBus::new());
        a.run(
            base_req(vec![ChatMessage::user("run it")], AiMode::Agent),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(
            approver.calls.load(Ordering::SeqCst),
            1,
            "execution tools always confirm"
        );
        assert_eq!(host.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn denied_tool_is_reported_back_to_model_without_execution() {
        let provider = FakeProvider::new(vec![
            (
                String::new(),
                vec![call(
                    "c1",
                    "run_load_test",
                    json!({"requestId":"r1","vus":10,"durationSec":5}),
                )],
            ),
            ("OK, the load test is cancelled".to_string(), vec![]),
        ]);
        let host = Arc::new(RecordingHost::default());
        let approver = Arc::new(CountingApprover {
            calls: AtomicUsize::new(0),
            allow: false,
        });
        let bus = EventBus::new();
        let a = agent(provider, host.clone(), approver, bus.clone());

        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("put it under load")], AiMode::Agent),
                CancellationToken::new(),
            )
            .await;

        assert!(
            host.calls.lock().unwrap().is_empty(),
            "a denied call must not be executed"
        );
        let tool_msg = outcome
            .messages
            .iter()
            .find(|m| m.role == crate::message::Role::Tool)
            .expect("a tool result message is expected");
        assert_eq!(tool_msg.tool_ok, Some(false));
        assert!(tool_msg.text.contains("User denied"));
        assert!(bus.drain().iter().any(|e| matches!(
            e,
            AiEvent::ToolCall {
                status: ToolStatus::Denied,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn unknown_tool_is_fed_back_instead_of_aborting() {
        let provider = FakeProvider::new(vec![
            (String::new(), vec![call("c1", "teleport", json!({}))]),
            ("I will try another tool".to_string(), vec![]),
        ]);
        let host = Arc::new(RecordingHost::default());
        let a = agent(
            provider,
            host.clone(),
            Arc::new(AutoApprover),
            EventBus::new(),
        );
        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("x")], AiMode::Agent),
                CancellationToken::new(),
            )
            .await;
        assert!(host.calls.lock().unwrap().is_empty());
        assert!(outcome.error.is_none());
        assert!(outcome
            .messages
            .iter()
            .any(|m| m.text.contains("No such tool")));
    }

    #[tokio::test]
    async fn host_failure_is_reported_and_loop_continues() {
        let provider = FakeProvider::new(vec![
            (
                String::new(),
                vec![call("c1", "run_request", json!({"requestId":"r1"}))],
            ),
            (
                "the endpoint returned 500; check the service".to_string(),
                vec![],
            ),
        ]);
        let host = Arc::new(RecordingHost::default());
        host.fail_next.store(true, Ordering::SeqCst);
        let bus = EventBus::new();
        let a = agent(provider, host, Arc::new(AutoApprover), bus.clone());
        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("run the request")], AiMode::Agent),
                CancellationToken::new(),
            )
            .await;

        assert!(
            outcome.error.is_none(),
            "a tool failure must not abort the turn"
        );
        assert!(outcome
            .messages
            .iter()
            .any(|m| m.text.contains("Execution failed")));
        assert!(bus
            .drain()
            .iter()
            .any(|e| matches!(e, AiEvent::ToolResult { ok: false, .. })));
    }

    #[tokio::test]
    async fn max_rounds_truncates_runaway_loops() {
        // Every round asks for a tool call -> the loop should stop after max_rounds
        let provider = FakeProvider::new(vec![
            (String::new(), vec![call("c1", "list_requests", json!({}))]),
            (String::new(), vec![call("c2", "list_requests", json!({}))]),
            (String::new(), vec![call("c3", "list_requests", json!({}))]),
            (String::new(), vec![call("c4", "list_requests", json!({}))]),
        ]);
        let host = Arc::new(RecordingHost::default());
        let bus = EventBus::new();
        let a = Agent::new(
            Box::new(provider),
            host.clone(),
            Arc::new(AutoApprover),
            bus.sink(None),
            AgentLimits {
                max_rounds: 2,
                max_tool_output_chars: 1000,
            },
        );
        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("loop")], AiMode::Agent),
                CancellationToken::new(),
            )
            .await;
        assert!(outcome.truncated);
        assert_eq!(host.calls.lock().unwrap().len(), 2);
        assert!(bus.drain().iter().any(|e| matches!(
            e,
            AiEvent::TurnFinished {
                truncated: true,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn cancelled_before_start_returns_immediately() {
        let provider = FakeProvider::new(vec![("x".into(), vec![])]);
        let a = agent(
            provider,
            Arc::new(RecordingHost::default()),
            Arc::new(AutoApprover),
            EventBus::new(),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("hi")], AiMode::Agent),
                cancel,
            )
            .await;
        assert!(matches!(outcome.error, Some(AiError::Cancelled)));
        assert!(outcome.messages.is_empty());
    }

    /// Regression (a real incident): an empty round (no text, no tool calls) **must not enter the session history**.
    ///
    /// It would be persisted with the messages and carried verbatim into the next round, making DeepSeek return 400
    /// （`Invalid assistant message: content or tool_calls must be set`），
    /// so every later round of that session fails - what the user sees is "it worked before and suddenly broke".
    #[tokio::test]
    async fn empty_turn_leaves_nothing_in_history() {
        let provider = FakeProvider::new(vec![(String::new(), vec![])]);
        let counts = provider.counter();
        let host = Arc::new(RecordingHost::default());
        let bus = EventBus::new();
        let a = agent(provider, host, Arc::new(AutoApprover), bus);

        let outcome = a
            .run(
                base_req(vec![ChatMessage::user("you there?")], AiMode::Agent),
                CancellationToken::new(),
            )
            .await;

        assert!(outcome.error.is_none());
        assert_eq!(
            counts.lock().unwrap().len(),
            1,
            "an empty round has no tool calls and should simply end"
        );
        assert!(
            outcome.messages.is_empty(),
            "an empty round with neither text nor reasoning must leave no message in the session: {:?}",
            outcome.messages
        );
    }

    #[tokio::test]
    async fn history_grows_with_tool_results_between_rounds() {
        let provider = FakeProvider::new(vec![
            (String::new(), vec![call("c1", "list_requests", json!({}))]),
            ("ok".to_string(), vec![]),
        ]);
        let counts = provider.counter();
        let host = Arc::new(RecordingHost::default());
        let bus = EventBus::new();
        let a = agent(provider, host, Arc::new(AutoApprover), bus);
        let _ = a
            .run(
                base_req(
                    vec![ChatMessage::user("which requests are there")],
                    AiMode::Agent,
                ),
                CancellationToken::new(),
            )
            .await;
        // The second request must carry two more messages than the first: the assistant tool call + the tool result
        let seen = counts.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "two model calls should happen");
        assert_eq!(
            seen[1],
            seen[0] + 2,
            "tool results must be fed back to the model"
        );
    }
}
