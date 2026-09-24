//! Persistent connection session management: interactive debugging for non-HTTP protocols.
//!
//! Unified model: open a session → send/receive messages in real time → close.
//! - websocket / tcp / udp: interactive send/receive (connection kept open);
//! - grpc: unary closes automatically after one round trip; server/client/bidirectional streaming keep the connection open;
//! - sse: pushes events one by one after subscribing; ends once max_events is reached or the user closes it;
//! - graphql: closes automatically after one request-response.
//!
//! Each session has a dedicated background task owning the connection: commands are sent via mpsc, received messages are broadcast via broadcast,
//! and also written to an in-memory log (for history queries). axum subscribes to events via SSE, Tauri receives event pushes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use orbit_js::JsSandbox;
use orbit_protocol::graphql::GraphqlClient;
use orbit_protocol::grpc::{encode_grpc_frame, GrpcClient};
use orbit_protocol::http::HttpClient;
use orbit_protocol::tcp::read_tcp_frame;
use orbit_protocol::traits::ProtocolClient;
use orbit_protocol::types::{FramingMode, FramingOptions, ProtocolRequest, StreamingMode};

// ────────────────────────────────────────────────────────────
// gRPC Server Reflection (service discovery) helpers — shared by the HTTP API and Tauri commands
// ────────────────────────────────────────────────────────────

/// A single method listed by reflection
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcReflectMethod {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

/// A single service listed by reflection (including all its methods)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcReflectService {
    pub name: String,
    pub methods: Vec<GrpcReflectMethod>,
}

/// Discover services and methods via gRPC Server Reflection (returns an empty list on connection failure / no reflection,
/// the caller decides whether to prompt for manual fallback input)
pub async fn grpc_reflect(url: &str) -> Result<Vec<GrpcReflectService>, String> {
    let mut client = GrpcClient::new();
    client
        .connect(url)
        .await
        .map_err(|e| format!("connection failed: {}", e))?;
    let services = client
        .list_services()
        .await
        .map_err(|e| format!("Reflection unavailable: {}", e))?;

    let mut out = Vec::new();
    let cache = client.service_cache().cloned().unwrap_or_default();
    for svc in services {
        let mut methods: Vec<GrpcReflectMethod> = cache
            .methods
            .iter()
            .filter_map(|(key, info)| {
                key.strip_prefix(&format!("{}/", svc))
                    .map(|name| GrpcReflectMethod {
                        name: name.to_string(),
                        input_type: info.input_type.clone(),
                        output_type: info.output_type.clone(),
                        client_streaming: info.client_streaming,
                        server_streaming: info.server_streaming,
                    })
            })
            .collect();
        methods.sort_by(|a, b| a.name.cmp(&b.name));
        out.push(GrpcReflectService { name: svc, methods });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Session event (pushed to the frontend in real time)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Opened {
        session_id: String,
        protocol: String,
        can_send: bool,
        time: u64,
    },
    Sent {
        session_id: String,
        seq: u64,
        data: String,
        text: Option<String>,
        /// Pre-request script console logs
        pre_logs: Vec<orbit_js::ScriptLog>,
        time: u64,
    },
    Received {
        session_id: String,
        seq: u64,
        data: String,
        text: Option<String>,
        decoded: Option<String>,
        /// Post-response script console logs
        post_logs: Vec<orbit_js::ScriptLog>,
        /// Structured SSE message fields (sse protocol only)
        sse: Option<SseFields>,
        time: u64,
    },
    Error {
        session_id: String,
        message: String,
        time: u64,
    },
    Closed {
        session_id: String,
        reason: String,
        time: u64,
    },
}

/// Session message log entry
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMessage {
    pub seq: u64,
    pub direction: &'static str,
    pub data: String,
    pub text: Option<String>,
    pub decoded: Option<String>,
    /// Pre-request script console logs (sent message)
    pub pre_logs: Vec<orbit_js::ScriptLog>,
    /// Post-response script console logs (received message)
    pub post_logs: Vec<orbit_js::ScriptLog>,
    /// Structured SSE message fields (sse protocol only)
    pub sse: Option<SseFields>,
    pub error: Option<String>,
    pub time: u64,
}

/// Standard SSE message fields: id / event / data / retry
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SseFields {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
    pub retry: Option<u64>,
}

/// Request to open a session
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenSessionRequest {
    pub protocol: String,
    pub url: String,
    /// gRPC service method (/pkg.Service/Method)
    pub service: Option<String>,
    /// gRPC message format (json/protobuf)
    pub message_format: Option<String>,
    /// gRPC streaming mode (unary/server_streaming/client_streaming/bidirectional)
    pub streaming: Option<String>,
    /// TCP framing config (mode/delimiter/fixed_len/big_endian)
    pub framing: Option<serde_json::Value>,
    /// WebSocket message type (text/binary)
    pub message_type: Option<String>,
    /// Auto-close WebSocket after receiving N messages (0/default = no auto-close)
    pub close_after: Option<u32>,
    /// Initial payload (text/base64/hex, decoded according to payload_type)
    pub payload: Option<String>,
    pub payload_type: Option<String>,
    /// Default pre-request script for sends (runs for every sent message, overridable per message)
    pub pre_script: Option<String>,
    /// Default post-response script for receives (runs for every received message, writes to pm.response.decoded)
    pub post_script: Option<String>,
    /// Max number of SSE events to collect (default 50)
    pub max_events: Option<u64>,
    /// GraphQL query / variables / operation_name
    pub query: Option<String>,
    pub variables: Option<String>,
    pub operation_name: Option<String>,
    /// Additional request headers (graphql/sse/grpc metadata)
    pub headers: Option<HashMap<String, String>>,
    /// Script environment variables
    pub env_vars: Option<HashMap<String, String>>,
    /// Plugin protocol connection config (JSON; from the collection connection, passed through to the plugin by run_plugin)
    pub connection: Option<serde_json::Value>,
}

/// Response to opening a session
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenSessionResponse {
    pub session_id: String,
    pub protocol: String,
    pub can_send: bool,
}

enum SessionCommand {
    Send {
        data: Vec<u8>,
        pre_script: Option<String>,
        ack: oneshot::Sender<Result<u64, String>>,
    },
    Close {
        ack: oneshot::Sender<Result<(), String>>,
    },
}

pub struct SessionHandle {
    pub id: String,
    pub protocol: String,
    pub can_send: bool,
    pub created_at: u64,
    pub closed: Arc<AtomicBool>,
    cmd_tx: mpsc::Sender<SessionCommand>,
    events_tx: broadcast::Sender<SessionEvent>,
    log: Arc<Mutex<Vec<SessionMessage>>>,
}

#[derive(Clone)]
struct EventCtx {
    events: broadcast::Sender<SessionEvent>,
    log: Arc<Mutex<Vec<SessionMessage>>>,
    seq: Arc<AtomicU64>,
}

#[derive(Clone)]
struct SessionCtx {
    id: String,
    protocol: String,
    url: String,
    env: HashMap<String, String>,
    events: EventCtx,
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    sink: Arc<dyn Fn(&SessionEvent) + Send + Sync>,
}

impl SessionCtx {
    fn emit(&self, ev: SessionEvent) {
        let _ = self.events.events.send(ev.clone());
        (self.sink)(&ev);
    }

    fn push_sent(&self, data: &[u8], pre_logs: &[orbit_js::ScriptLog]) -> u64 {
        let seq = self.events.seq.fetch_add(1, Ordering::SeqCst);
        let time = now_ms();
        self.events.log.lock().unwrap().push(SessionMessage {
            seq,
            direction: "send",
            data: bytes_to_b64(data),
            text: utf8_text(data),
            decoded: None,
            pre_logs: pre_logs.to_vec(),
            post_logs: Vec::new(),
            sse: None,
            error: None,
            time,
        });
        self.emit(SessionEvent::Sent {
            session_id: self.id.clone(),
            seq,
            data: bytes_to_b64(data),
            text: utf8_text(data),
            pre_logs: pre_logs.to_vec(),
            time,
        });
        seq
    }

    fn push_received(&self, data: &[u8], post_script: Option<&str>, sse: Option<SseFields>) {
        let seq = self.events.seq.fetch_add(1, Ordering::SeqCst);
        let time = now_ms();
        let mut post_logs: Vec<orbit_js::ScriptLog> = Vec::new();
        let decoded = match post_script {
            Some(s) => match run_recv_script(s, data, &self.env) {
                Ok((d, logs)) => {
                    post_logs = logs;
                    d
                }
                Err(e) => {
                    self.emit_error(&e);
                    None
                }
            },
            None => None,
        };
        self.events.log.lock().unwrap().push(SessionMessage {
            seq,
            direction: "recv",
            data: bytes_to_b64(data),
            text: utf8_text(data),
            decoded: decoded.clone(),
            pre_logs: Vec::new(),
            post_logs: post_logs.clone(),
            sse: sse.clone(),
            error: None,
            time,
        });
        self.emit(SessionEvent::Received {
            session_id: self.id.clone(),
            seq,
            data: bytes_to_b64(data),
            text: utf8_text(data),
            decoded,
            post_logs,
            sse,
            time,
        });
    }

    fn emit_error(&self, message: &str) {
        self.emit(SessionEvent::Error {
            session_id: self.id.clone(),
            message: message.to_string(),
            time: now_ms(),
        });
    }

    fn emit_closed(&self, reason: &str) {
        self.emit(SessionEvent::Closed {
            session_id: self.id.clone(),
            reason: reason.to_string(),
            time: now_ms(),
        });
        self.done();
    }

    /// Session ended: remove from the manager
    fn done(&self) {
        if let Ok(m) = self.sessions.lock() {
            if let Some(h) = m.get(&self.id) {
                h.closed.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// Session manager: shared by axum and Tauri
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    sink: Arc<dyn Fn(&SessionEvent) + Send + Sync>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            sink: Arc::new(|_| {}),
        }
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a global event callback (used by Tauri for app.emit; left empty on the axum side to use broadcast)
    pub fn set_global_sink(&mut self, f: impl Fn(&SessionEvent) + Send + Sync + 'static) {
        self.sink = Arc::new(f);
    }

    pub async fn open(&self, req: OpenSessionRequest) -> Result<OpenSessionResponse, String> {
        let id = format!("s-{}", Uuid::new_v4().simple());
        let (events_tx, _) = broadcast::channel(1024);
        let log = Arc::new(Mutex::new(Vec::new()));
        let seq = Arc::new(AtomicU64::new(0));
        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCommand>(64);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

        let protocol = req.protocol.clone();
        let connection = req.connection.clone();
        let env = req.env_vars.clone().unwrap_or_default();
        let events = EventCtx {
            events: events_tx.clone(),
            log: log.clone(),
            seq: seq.clone(),
        };
        let ctx = SessionCtx {
            id: id.clone(),
            protocol: protocol.clone(),
            url: req.url.clone(),
            env,
            events,
            sessions: self.sessions.clone(),
            sink: self.sink.clone(),
        };

        let pre_script = req.pre_script.clone();
        let post_script = req.post_script.clone();
        let streaming = parse_streaming(req.streaming.as_deref());
        let can_send = match protocol.as_str() {
            "websocket" | "tcp" | "udp" => true,
            "grpc" => !matches!(streaming, Some(StreamingMode::ServerStreaming)),
            other => {
                // Plugin protocols (native/wasm registered into the dynamic protocol registry): sending is supported (one request per execute)
                orbit_protocol::registry::build_client_by_id(other).is_some()
            }
        };

        let task = async move {
            match ctx.protocol.as_str() {
                "websocket" => {
                    run_websocket(
                        ctx,
                        cmd_rx,
                        ready_tx,
                        req.message_type.as_deref().unwrap_or("text") == "binary",
                        req.close_after.unwrap_or(0),
                        pre_script,
                        post_script,
                    )
                    .await;
                }
                "tcp" => {
                    run_tcp(
                        ctx,
                        cmd_rx,
                        ready_tx,
                        parse_framing(req.framing.clone()),
                        pre_script,
                        post_script,
                    )
                    .await;
                }
                "udp" => {
                    run_udp(ctx, cmd_rx, ready_tx, pre_script, post_script).await;
                }
                "grpc" => {
                    run_grpc(
                        ctx,
                        cmd_rx,
                        ready_tx,
                        req.service.clone().unwrap_or_default(),
                        req.message_format.clone().unwrap_or_else(|| "json".into()),
                        streaming,
                        decode_payload(
                            req.payload.as_deref().unwrap_or(""),
                            req.payload_type.as_deref().unwrap_or("text"),
                        )
                        .unwrap_or_default(),
                        req.headers.clone().unwrap_or_default(),
                        pre_script,
                        post_script,
                    )
                    .await;
                }
                "sse" => {
                    run_sse(
                        ctx,
                        cmd_rx,
                        ready_tx,
                        req.max_events.unwrap_or(50) as usize,
                        req.headers.clone().unwrap_or_default(),
                        post_script,
                    )
                    .await;
                }
                "graphql" => {
                    run_graphql(
                        ctx,
                        cmd_rx,
                        ready_tx,
                        req.query.clone().unwrap_or_default(),
                        req.variables.clone(),
                        req.operation_name.clone(),
                        req.headers.clone().unwrap_or_default(),
                        post_script,
                    )
                    .await;
                }
                other => {
                    // Plugin protocols (native/wasm dynamic protocols): each send builds one request to execute (request-response)
                    if orbit_protocol::registry::build_client_by_id(other).is_some() {
                        run_plugin(
                            ctx,
                            cmd_rx,
                            ready_tx,
                            connection.clone(),
                            pre_script,
                            post_script,
                        )
                        .await;
                    } else {
                        let _ = ready_tx.send(Err(format!("unsupported protocol: {other}")));
                        ctx.done();
                    }
                }
            }
        };
        tokio::spawn(task);

        match tokio::time::timeout(Duration::from_secs(15), ready_rx).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => {
                self.sessions.lock().unwrap().remove(&id);
                return Err(e);
            }
            Ok(Err(_)) => {
                self.sessions.lock().unwrap().remove(&id);
                return Err("session task exited unexpectedly".into());
            }
            Err(_) => {
                self.sessions.lock().unwrap().remove(&id);
                return Err("connection timed out".into());
            }
        }

        let handle = SessionHandle {
            id: id.clone(),
            protocol: protocol.clone(),
            can_send,
            created_at: now_ms(),
            closed: Arc::new(AtomicBool::new(false)),
            cmd_tx,
            events_tx,
            log,
        };
        {
            let mut m = self.sessions.lock().unwrap();
            m.insert(id.clone(), handle);
            // Capacity cap: evict the oldest closed sessions to avoid memory growth over long runs
            if m.len() > 128 {
                let mut closed_ids: Vec<(u64, String)> = m
                    .iter()
                    .filter(|(_, h)| h.closed.load(Ordering::SeqCst))
                    .map(|(k, h)| (h.created_at, k.clone()))
                    .collect();
                closed_ids.sort_unstable();
                while m.len() > 64 {
                    if let Some((_, k)) = closed_ids.first() {
                        m.remove(k);
                        closed_ids.remove(0);
                    } else {
                        break;
                    }
                }
            }
        }
        Ok(OpenSessionResponse {
            session_id: id,
            protocol,
            can_send,
        })
    }

    /// Send a message and return its sequence number
    pub async fn send(
        &self,
        id: &str,
        data: Vec<u8>,
        pre_script: Option<String>,
    ) -> Result<u64, String> {
        let tx = {
            let m = self.sessions.lock().unwrap();
            let s = m.get(id).ok_or_else(|| "session not found".to_string())?;
            if s.closed.load(Ordering::SeqCst) {
                return Err("session is closed".into());
            }
            s.cmd_tx.clone()
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(SessionCommand::Send {
            data,
            pre_script,
            ack: ack_tx,
        })
        .await
        .map_err(|_| "session is closed".to_string())?;
        tokio::time::timeout(Duration::from_secs(15), ack_rx)
            .await
            .map_err(|_| "send timed out".to_string())?
            .map_err(|_| "send failed".to_string())?
    }

    /// Close a session
    pub async fn close(&self, id: &str) -> Result<(), String> {
        let tx = {
            let m = self.sessions.lock().unwrap();
            let s = m.get(id).ok_or_else(|| "session not found".to_string())?;
            if s.closed.load(Ordering::SeqCst) {
                return Err("session is closed".into());
            }
            s.cmd_tx.clone()
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(SessionCommand::Close { ack: ack_tx })
            .await
            .map_err(|_| "session is closed".to_string())?;
        let _ = tokio::time::timeout(Duration::from_secs(10), ack_rx).await;
        Ok(())
    }

    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<SessionEvent>> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.events_tx.subscribe())
    }

    pub fn messages(&self, id: &str) -> Vec<SessionMessage> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.log.lock().unwrap().clone())
            .unwrap_or_default()
    }

    /// Replay session history as an event sequence (backfilled on SSE connect to avoid losing messages during the subscription window)
    pub fn events_backlog(&self, id: &str) -> Vec<SessionEvent> {
        self.messages(id)
            .into_iter()
            .map(|m| match m.direction {
                "send" => SessionEvent::Sent {
                    session_id: id.to_string(),
                    seq: m.seq,
                    data: m.data,
                    text: m.text,
                    pre_logs: m.pre_logs,
                    time: m.time,
                },
                _ => SessionEvent::Received {
                    session_id: id.to_string(),
                    seq: m.seq,
                    data: m.data,
                    text: m.text,
                    decoded: m.decoded,
                    post_logs: m.post_logs,
                    sse: m.sse,
                    time: m.time,
                },
            })
            .collect()
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|s| !s.closed.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn can_send(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.can_send)
            .unwrap_or(false)
    }
}

// ─── Protocol session tasks ──────────────────────────────────

async fn run_websocket(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    is_binary: bool,
    close_after: u32,
    pre_script: Option<String>,
    post_script: Option<String>,
) {
    let connect = tokio_tungstenite::connect_async(&ctx.url).await;
    let (mut sink, mut stream) = match connect {
        Ok((ws, _)) => ws.split(),
        Err(e) => {
            let msg = format!("connection failed: {e}");
            let _ = ready_tx.send(Err(msg.clone()));
            ctx.emit_error(&msg);
            ctx.done();
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));
    let mut recv_count = 0u32;
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Send { data, pre_script: ps, ack }) => {
                        let script = ps.as_deref().or(pre_script.as_deref());
                        let empty_headers = HashMap::new();
                        let (payload, pre_logs) = match script {
                            Some(s) => match run_send_script(
                                s, &ctx.url, "", &empty_headers, &data, &ctx.env,
                            ) {
                                Ok(out) => (out.raw, out.logs),
                                Err(e) => {
                                    ctx.emit_error(&e);
                                    let _ = ack.send(Err(e));
                                    continue;
                                }
                            },
                            None => (data.clone(), Vec::new()),
                        };
                        let msg = if is_binary {
                            tokio_tungstenite::tungstenite::Message::Binary(payload)
                        } else {
                            tokio_tungstenite::tungstenite::Message::Text(
                                String::from_utf8_lossy(&payload).into_owned(),
                            )
                        };
                        match sink.send(msg).await {
                            Ok(_) => {
                                let seq = ctx.push_sent(&data, &pre_logs);
                                let _ = ack.send(Ok(seq));
                            }
                            Err(e) => {
                                let m = format!("send failed: {e}");
                                ctx.emit_error(&m);
                                let _ = ack.send(Err(m));
                            }
                        }
                    }
                    Some(SessionCommand::Close { ack }) => {
                        let _ = sink.close().await;
                        let _ = ack.send(Ok(()));
                        ctx.emit_closed("user closed");
                        break;
                    }
                    None => break,
                }
            }
            item = stream.next() => {
                use tokio_tungstenite::tungstenite::Message;
                match item {
                    Some(Ok(Message::Text(t))) => {
                        recv_count += 1;
                        ctx.push_received(t.as_bytes(), post_script.as_deref(), None);
                    }
                    Some(Ok(Message::Binary(b))) => {
                        recv_count += 1;
                        ctx.push_received(&b, post_script.as_deref(), None);
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Ok(Message::Close(_))) => {
                        ctx.emit_closed("peer closed");
                        break;
                    }
                    Some(Err(e)) => {
                        let m = format!("connection error: {e}");
                        ctx.emit_error(&m);
                        ctx.emit_closed(&m);
                        break;
                    }
                    None => {
                        ctx.emit_closed("connection closed");
                        break;
                    }
                }
                if close_after > 0 && recv_count >= close_after {
                    ctx.emit_closed("close_after");
                    break;
                }
            }
        }
    }
}

async fn run_tcp(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    framing: Option<FramingOptions>,
    pre_script: Option<String>,
    post_script: Option<String>,
) {
    let target = ctx
        .url
        .strip_prefix("tcp://")
        .unwrap_or(&ctx.url)
        .to_string();
    let stream = match TcpStream::connect(&target).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("connection failed: {e}");
            let _ = ready_tx.send(Err(msg.clone()));
            ctx.emit_error(&msg);
            ctx.done();
            return;
        }
    };
    stream.set_nodelay(true).ok();
    let (mut rd, mut wr) = tokio::io::split(stream);
    let _ = ready_tx.send(Ok(()));
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Send { data, pre_script: ps, ack }) => {
                        let script = ps.as_deref().or(pre_script.as_deref());
                        let empty_headers = HashMap::new();
                        let (payload, pre_logs) = match script {
                            Some(s) => match run_send_script(
                                s, &ctx.url, "", &empty_headers, &data, &ctx.env,
                            ) {
                                Ok(out) => (out.raw, out.logs),
                                Err(e) => {
                                    ctx.emit_error(&e);
                                    let _ = ack.send(Err(e));
                                    continue;
                                }
                            },
                            None => (data.clone(), Vec::new()),
                        };
                        match wr.write_all(&payload).await {
                            Ok(_) => {
                                let seq = ctx.push_sent(&data, &pre_logs);
                                let _ = ack.send(Ok(seq));
                            }
                            Err(e) => {
                                let m = format!("send failed: {e}");
                                ctx.emit_error(&m);
                                let _ = ack.send(Err(m));
                            }
                        }
                    }
                    Some(SessionCommand::Close { ack }) => {
                        let _ = wr.shutdown().await;
                        let _ = ack.send(Ok(()));
                        ctx.emit_closed("user closed");
                        break;
                    }
                    None => break,
                }
            }
            recv = read_tcp_frame(&mut rd, framing.clone()) => {
                match recv {
                    Ok(buf) => {
                        if buf.is_empty() {
                            ctx.emit_closed("connection closed");
                            break;
                        }
                        ctx.push_received(&buf, post_script.as_deref(), None);
                    }
                    Err(e) => {
                        let m = format!("receive failed: {e}");
                        ctx.emit_error(&m);
                        ctx.emit_closed(&m);
                        break;
                    }
                }
            }
        }
    }
}

/// Session runner for plugin protocols (native/wasm dynamic protocols): request-response model.
/// open = confirm the dynamic protocol client is available and ready; each send = build one request, execute it, and echo the response.
async fn run_plugin(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    connection: Option<serde_json::Value>,
    pre_script: Option<String>,
    post_script: Option<String>,
) {
    // open: the dynamic protocol client must exist (already validated by the caller; this is a fallback)
    let Some(mut client) = orbit_protocol::registry::build_client_by_id(&ctx.protocol) else {
        let m = format!("plugin protocol '{}' is not loaded", ctx.protocol);
        let _ = ready_tx.send(Err(m.clone()));
        ctx.emit_error(&m);
        ctx.done();
        return;
    };
    let _ = ready_tx.send(Ok(()));

    loop {
        match cmd_rx.recv().await {
            Some(SessionCommand::Send {
                data,
                pre_script: ps,
                ack,
            }) => {
                // send = execute one plugin request; payload is the bytes being sent (SQL/command)
                let script = ps.as_deref().or(pre_script.as_deref());
                let empty_headers = HashMap::new();
                let (payload, pre_logs) = match script {
                    Some(s) => {
                        match run_send_script(s, &ctx.url, "", &empty_headers, &data, &ctx.env) {
                            Ok(out) => (out.raw, out.logs),
                            Err(e) => {
                                ctx.emit_error(&e);
                                let _ = ack.send(Err(e));
                                continue;
                            }
                        }
                    }
                    None => (data.clone(), Vec::new()),
                };

                let request = orbit_protocol::types::ProtocolRequest {
                    target: ctx.url.clone(),
                    operation: String::new(),
                    metadata: vec![],
                    payload,
                    timeout: Some(std::time::Duration::from_secs(30)),
                    streaming_mode: None,
                    payload_format: None,
                    response_format: None,
                    options: orbit_protocol::types::ProtocolOptions::default(),
                    connection: connection.clone().map(|c| c.to_string()),
                };

                match client.execute(request).await {
                    Ok(resp) => {
                        let _ = ctx.push_sent(&data, &pre_logs);
                        ctx.push_received(&resp.payload, post_script.as_deref(), None);
                        let _ = ack.send(Ok(0));
                    }
                    Err(e) => {
                        let m = format!("plugin execution failed: {}", e);
                        ctx.emit_error(&m);
                        let _ = ack.send(Err(m));
                    }
                }
            }
            Some(SessionCommand::Close { ack }) => {
                let _ = ack.send(Ok(()));
                ctx.emit_closed("user closed");
                break;
            }
            None => break,
        }
    }
}

async fn run_udp(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    pre_script: Option<String>,
    post_script: Option<String>,
) {
    let target = ctx
        .url
        .strip_prefix("udp://")
        .unwrap_or(&ctx.url)
        .to_string();
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            let msg = format!("UDP bind failed: {e}");
            let _ = ready_tx.send(Err(msg.clone()));
            ctx.emit_error(&msg);
            ctx.done();
            return;
        }
    };
    if let Err(e) = socket.connect(&target).await {
        let msg = format!("UDP connect failed: {e}");
        let _ = ready_tx.send(Err(msg.clone()));
        ctx.emit_error(&msg);
        ctx.done();
        return;
    }
    let _ = ready_tx.send(Ok(()));
    let mut buf = vec![0u8; 65536];
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Send { data, pre_script: ps, ack }) => {
                        let script = ps.as_deref().or(pre_script.as_deref());
                        let empty_headers = HashMap::new();
                        let (payload, pre_logs) = match script {
                            Some(s) => match run_send_script(
                                s, &ctx.url, "", &empty_headers, &data, &ctx.env,
                            ) {
                                Ok(out) => (out.raw, out.logs),
                                Err(e) => {
                                    ctx.emit_error(&e);
                                    let _ = ack.send(Err(e));
                                    continue;
                                }
                            },
                            None => (data.clone(), Vec::new()),
                        };
                        match socket.send(&payload).await {
                            Ok(_) => {
                                let seq = ctx.push_sent(&data, &pre_logs);
                                let _ = ack.send(Ok(seq));
                            }
                            Err(e) => {
                                let m = format!("send failed: {e}");
                                ctx.emit_error(&m);
                                let _ = ack.send(Err(m));
                            }
                        }
                    }
                    Some(SessionCommand::Close { ack }) => {
                        let _ = ack.send(Ok(()));
                        ctx.emit_closed("user closed");
                        break;
                    }
                    None => break,
                }
            }
            recv = socket.recv(&mut buf) => {
                match recv {
                    Ok(n) => ctx.push_received(&buf[..n], post_script.as_deref(), None),
                    Err(e) => {
                        let m = format!("receive failed: {e}");
                        ctx.emit_error(&m);
                        ctx.emit_closed(&m);
                        break;
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_grpc(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    service: String,
    message_format: String,
    streaming: Option<StreamingMode>,
    initial_payload: Vec<u8>,
    headers: HashMap<String, String>,
    pre_script: Option<String>,
    post_script: Option<String>,
) {
    let is_json = message_format == "json";
    let mut grpc = GrpcClient::new();
    // The first message (the initial payload sent on sessionOpen) also runs the pre-request script:
    // rewrite the initial request body and collect script logs, ensuring the pre-request script applies to unary / the first streaming message.
    let (initial_payload, initial_pre_logs) = match pre_script.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            match run_send_script(s, &ctx.url, &service, &headers, &initial_payload, &ctx.env) {
                Ok(out) => (out.body, out.logs),
                Err(e) => {
                    ctx.emit_error(&e);
                    (initial_payload, Vec::new())
                }
            }
        }
        _ => (initial_payload, Vec::new()),
    };
    let request = ProtocolRequest {
        target: ctx.url.clone(),
        operation: service.clone(),
        metadata: headers.clone().into_iter().collect(),
        payload: initial_payload.clone(),
        payload_format: if is_json { Some("json".into()) } else { None },
        response_format: if is_json { Some("json".into()) } else { None },
        ..Default::default()
    };
    // Record the first sent message (including pre-request script logs) for display in the "Script" tab
    if !initial_payload.is_empty() || !initial_pre_logs.is_empty() {
        ctx.push_sent(&initial_payload, &initial_pre_logs);
    }

    if streaming.is_none() {
        let resp = match grpc.execute(request).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("gRPC call failed: {e}");
                let _ = ready_tx.send(Err(msg.clone()));
                ctx.emit_error(&msg);
                ctx.done();
                return;
            }
        };
        let _ = ready_tx.send(Ok(()));
        ctx.push_received(&resp.payload, post_script.as_deref(), None);
        ctx.emit_closed("unary complete");
        return;
    }

    // client_streaming / bidirectional: the connection supports interactive sends and response headers return late (with client_streaming the server only responds after the request stream is half-closed),
    // so connection setup does not block waiting for response headers; with server_streaming the server returns the stream immediately.
    let is_sendable = matches!(
        streaming,
        Some(StreamingMode::ClientStreaming) | Some(StreamingMode::Bidirectional)
    );
    let mut stream = if is_sendable {
        match grpc.open_sendable_stream(request, streaming.unwrap()).await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("gRPC streaming call failed: {e}");
                let _ = ready_tx.send(Err(msg.clone()));
                ctx.emit_error(&msg);
                ctx.done();
                return;
            }
        }
    } else {
        match grpc
            .open_persistent_stream(request, streaming.unwrap())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("gRPC streaming call failed: {e}");
                let _ = ready_tx.send(Err(msg.clone()));
                ctx.emit_error(&msg);
                ctx.done();
                return;
            }
        }
    };
    // Immediately notify that the connection is established (sendable streams do not wait for response headers; server_streaming already got the response at open time)
    let _ = ready_tx.send(Ok(()));

    let mut request_tx = stream.request_tx.take();

    // Consume the response body stream in the background (for sendable streams, client_streaming receives the server response after the request stream is half-closed; bidi streams in real time).
    // Response frame -> JSON message -> push_received.
    // Decode using the decode snapshot to avoid contention with the send main loop over `&mut grpc`.
    let decode_snapshot = grpc.decode_snapshot();
    {
        let body_ctx = ctx.clone();
        let service2 = service.clone();
        let post2 = post_script.clone();
        let is_json2 = is_json;
        // Move the stream into the background task, which waits for and consumes the response body stream
        tokio::spawn(async move {
            let mut stream = stream;
            let mut buffer: Vec<u8> = Vec::new();
            let body = match stream.take_body().await {
                Some(b) => b,
                None => {
                    body_ctx.emit_closed("stream ended");
                    return;
                }
            };
            let mut body = body;
            loop {
                match body.frame().await {
                    Some(Ok(f)) => {
                        if let Some(d) = f.data_ref() {
                            buffer.extend_from_slice(d);
                        }
                        while buffer.len() >= 5 {
                            let msg_len =
                                u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]])
                                    as usize;
                            if buffer.len() < 5 + msg_len {
                                break;
                            }
                            let raw = buffer[5..5 + msg_len].to_vec();
                            buffer.drain(..5 + msg_len);
                            let payload = if is_json2 {
                                decode_snapshot
                                    .as_ref()
                                    .and_then(|snap| {
                                        orbit_protocol::grpc::decode_message_snapshot(
                                            snap, &service2, &raw,
                                        )
                                    })
                                    .unwrap_or_else(|| raw.clone())
                            } else {
                                raw.clone()
                            };
                            body_ctx.push_received(&payload, post2.as_deref(), None);
                        }
                    }
                    Some(Err(e)) => {
                        let m = format!("gRPC stream error: {e}");
                        body_ctx.emit_error(&m);
                        body_ctx.emit_closed(&m);
                        return;
                    }
                    None => {
                        body_ctx.emit_closed("stream ended");
                        return;
                    }
                }
            }
        });
    }

    // Main loop: handle send / close commands (client_stream / bidi can send repeatedly)
    loop {
        match cmd_rx.recv().await {
            Some(SessionCommand::Send {
                data,
                pre_script: ps,
                ack,
            }) => {
                let script = ps.as_deref().or(pre_script.as_deref());
                // gRPC: the pre-request script can read/write method / url / headers(metadata) / body(JSON) via pm.request.
                // The send payload uses the script-rewritten text body (JSON) rather than the binary raw.
                let (payload, pre_logs) = match script {
                    Some(s) => {
                        match run_send_script(s, &ctx.url, &service, &headers, &data, &ctx.env) {
                            Ok(out) => (out.body, out.logs),
                            Err(e) => {
                                ctx.emit_error(&e);
                                let _ = ack.send(Err(e));
                                continue;
                            }
                        }
                    }
                    None => (data.clone(), Vec::new()),
                };
                let pb = if is_json {
                    match grpc.encode_json_message(&service, &payload) {
                        Ok(p) => p,
                        Err(e) => {
                            let m = format!("JSON to proto conversion failed: {e}");
                            ctx.emit_error(&m);
                            let _ = ack.send(Err(m));
                            continue;
                        }
                    }
                } else {
                    payload
                };
                match &request_tx {
                    Some(tx) => match tx.send(encode_grpc_frame(pb)).await {
                        Ok(_) => {
                            let seq = ctx.push_sent(&data, &pre_logs);
                            let _ = ack.send(Ok(seq));
                        }
                        Err(_) => {
                            let _ = ack.send(Err("request stream is closed".into()));
                        }
                    },
                    None => {
                        let _ =
                            ack.send(Err("server_streaming does not support sending again".into()));
                    }
                }
            }
            Some(SessionCommand::Close { ack }) => {
                let _ = ack.send(Ok(()));
                if streaming == Some(StreamingMode::ClientStreaming) {
                    // Half-close the request stream so the server returns the final response (the background task emits closed after consuming it)
                    drop(request_tx.take());
                    // Wait for the background task to finish consuming the response
                    break;
                } else {
                    ctx.emit_closed("user closed");
                    break;
                }
            }
            None => break,
        }
    }
}

async fn run_sse(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    max_events: usize,
    headers: HashMap<String, String>,
    post_script: Option<String>,
) {
    let mut http = HttpClient::new();
    let request = ProtocolRequest {
        target: ctx.url.clone(),
        operation: "GET".into(),
        metadata: headers.into_iter().collect(),
        ..Default::default()
    };
    let (_, _status, _resp_headers, mut body) = match http.stream_response(request).await {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("SSE connection failed: {e}");
            let _ = ready_tx.send(Err(msg.clone()));
            ctx.emit_error(&msg);
            ctx.done();
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));

    let mut buffer = String::new();
    let mut current_data = String::new();
    let mut current_id: Option<String> = None;
    let mut current_event: Option<String> = None;
    let mut current_retry: Option<u64> = None;
    let mut count = 0usize;
    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                if let Some(SessionCommand::Close { ack }) = cmd {
                    let _ = ack.send(Ok(()));
                    ctx.emit_closed("user closed");
                    break;
                }
            }
            frame = body.frame() => {
                match frame {
                    Some(Ok(f)) => {
                        if let Some(d) = f.data_ref() {
                            buffer.push_str(&String::from_utf8_lossy(d));
                        }
                        while let Some(line_end) = buffer.find('\n') {
                            let line = buffer[..line_end].trim_end_matches('\r').to_string();
                            buffer = buffer[line_end + 1..].to_string();
                            if line.is_empty() {
                                if !current_data.is_empty() {
                                    let data = std::mem::take(&mut current_data);
                                    ctx.push_received(
                                        data.as_bytes(),
                                        post_script.as_deref(),
                                        Some(SseFields {
                                            id: current_id.take(),
                                            event: current_event.take(),
                                            data: data.clone(),
                                            retry: current_retry.take(),
                                        }),
                                    );
                                    count += 1;
                                    if count >= max_events {
                                        ctx.emit_closed("max_events");
                                        return;
                                    }
                                }
                            } else if let Some(v) = line.strip_prefix("data:") {
                                if !current_data.is_empty() {
                                    current_data.push('\n');
                                }
                                current_data.push_str(v.trim());
                            } else if let Some(v) = line.strip_prefix("id:") {
                                current_id = Some(v.trim().to_string());
                            } else if let Some(v) = line.strip_prefix("event:") {
                                current_event = Some(v.trim().to_string());
                            } else if let Some(v) = line.strip_prefix("retry:") {
                                current_retry = v.trim().parse().ok();
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let m = format!("SSE stream error: {e}");
                        ctx.emit_error(&m);
                        ctx.emit_closed(&m);
                        break;
                    }
                    None => {
                        ctx.emit_closed("stream ended");
                        break;
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_graphql(
    ctx: SessionCtx,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    query: String,
    variables: Option<String>,
    operation_name: Option<String>,
    headers: HashMap<String, String>,
    post_script: Option<String>,
) {
    let mut gql = GraphqlClient::new();
    let mut metadata: Vec<(String, String)> = headers.into_iter().collect();
    if let Some(v) = variables {
        metadata.push(("graphql-variables".into(), v));
    }
    if let Some(op) = operation_name {
        metadata.push(("graphql-operation-name".into(), op));
    }
    let request = ProtocolRequest {
        target: ctx.url.clone(),
        operation: "POST".into(),
        metadata,
        payload: query.as_bytes().to_vec(),
        ..Default::default()
    };
    let resp = match gql.execute(request).await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("GraphQL call failed: {e}");
            let _ = ready_tx.send(Err(msg.clone()));
            ctx.emit_error(&msg);
            ctx.done();
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));
    ctx.push_received(&resp.payload, post_script.as_deref(), None);
    ctx.emit_closed("graphql complete");
    // Discard any commands that may have piled up
    while cmd_rx.try_recv().is_ok() {}
    ctx.done();
}

// ─── Helper functions ────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn bytes_to_b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn utf8_text(b: &[u8]) -> Option<String> {
    String::from_utf8(b.to_vec()).ok()
}

fn decode_payload(s: &str, ptype: &str) -> Result<Vec<u8>, String> {
    match ptype {
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|e| format!("base64 decode failed: {e}")),
        "hex" => {
            let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            if !clean.len().is_multiple_of(2) {
                return Err("hex length must be even".into());
            }
            let mut out = Vec::with_capacity(clean.len() / 2);
            for i in (0..clean.len()).step_by(2) {
                let b = u8::from_str_radix(&clean[i..i + 2], 16)
                    .map_err(|_| "hex decode failed".to_string())?;
                out.push(b);
            }
            Ok(out)
        }
        _ => Ok(s.as_bytes().to_vec()),
    }
}

fn bytes_to_byte_string(b: &[u8]) -> String {
    b.iter().map(|&x| x as char).collect()
}

fn byte_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

/// Pre-request script result: the rewritten request body (text body) and the raw payload (byte string).
struct SendScriptOut {
    /// Script-rewritten text request body (`pm.request.body.raw` -> for text protocols / gRPC JSON)
    body: Vec<u8>,
    /// Script-rewritten raw payload (`pm.request.raw` -> for binary protocols)
    raw: Vec<u8>,
    logs: Vec<orbit_js::ScriptLog>,
}

fn run_send_script(
    script: &str,
    url: &str,
    method: &str,
    headers: &HashMap<String, String>,
    payload: &[u8],
    env: &HashMap<String, String>,
) -> Result<SendScriptOut, String> {
    let sandbox = JsSandbox::new().map_err(|e| format!("script engine init failed: {e}"))?;
    let mut ctx = orbit_js::RequestContext {
        url: url.to_string(),
        method: method.to_string(),
        headers: headers.clone(),
        body: String::from_utf8_lossy(payload).into_owned(),
        raw: bytes_to_byte_string(payload),
    };
    let out = sandbox.run_pre_request(script, &mut ctx, Some(env), None);
    if !out.success {
        return Err(out
            .error
            .unwrap_or_else(|| "pre-request script failed".into()));
    }
    Ok(SendScriptOut {
        body: ctx.body.into_bytes(),
        raw: byte_string_to_bytes(&ctx.raw),
        logs: out.logs,
    })
}

fn run_recv_script(
    script: &str,
    payload: &[u8],
    env: &HashMap<String, String>,
) -> Result<(Option<String>, Vec<orbit_js::ScriptLog>), String> {
    let sandbox = JsSandbox::new().map_err(|e| format!("script engine init failed: {e}"))?;
    let ctx = orbit_js::ResponseContext {
        status: 0,
        body: String::from_utf8_lossy(payload).into_owned(),
        headers: HashMap::new(),
        duration_ms: 0,
        raw: bytes_to_byte_string(payload),
        decoded: None,
    };
    let out = sandbox.run_post_response(script, &ctx, Some(env), None);
    if !out.success {
        return Err(out
            .error
            .unwrap_or_else(|| "post-response script failed".into()));
    }
    Ok((out.decoded, out.logs))
}

fn parse_streaming(s: Option<&str>) -> Option<StreamingMode> {
    match s {
        Some("server_streaming") => Some(StreamingMode::ServerStreaming),
        Some("client_streaming") => Some(StreamingMode::ClientStreaming),
        Some("bidirectional") => Some(StreamingMode::Bidirectional),
        _ => None,
    }
}

fn parse_framing(v: Option<serde_json::Value>) -> Option<FramingOptions> {
    let obj = v?;
    let mode = obj
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("delimiter");
    let mode = match mode {
        "fixed" => FramingMode::Fixed,
        "length_prefix" => FramingMode::LengthPrefix,
        "read_until_close" => FramingMode::ReadUntilClose,
        _ => FramingMode::Delimiter,
    };
    Some(FramingOptions {
        mode,
        delimiter: obj
            .get("delimiter")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string()),
        fixed_len: obj
            .get("fixed_len")
            .and_then(|n| n.as_u64())
            .map(|n| n as u32),
        big_endian: obj
            .get("big_endian")
            .and_then(|b| b.as_bool())
            .unwrap_or(true),
    })
}
