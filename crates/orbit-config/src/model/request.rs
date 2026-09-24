//! Protocol-agnostic request config and per-protocol Configs (including the hand-written RequestSpec deserialization discriminator)

use orbit_protocol::{FramingOptions, WsMessageType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Payload representation: text (UTF-8) / base64 / hex
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadType {
    #[default]
    Text,
    Base64,
    Hex,
}

/// One message in a long-lived connection message sequence (the connection is kept across the sequence; each message can be scripted)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSpec {
    /// Payload (represented per payload_type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Payload representation (text/base64/hex), defaults to text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    /// WebSocket only: whether to send a text frame or a binary frame
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<WsMessageType>,
    /// Pre-send script (custom encoding via pm.request.raw / pm.utf8/pm.hex/pm.b64)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_script: Option<String>,
    /// Post-receive script (custom decoding via pm.response.raw / pm.response.decoded)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_script: Option<String>,
    /// Wait between messages (milliseconds)
    #[serde(default)]
    pub wait_ms: u64,
}

/// WebSocket request config (new form: `url` is required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    pub url: String,
    /// Content to send; an empty message is sent by default
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Message type (text | binary)
    #[serde(default)]
    pub message_type: WsMessageType,
    /// Payload representation (text/base64/hex), defaults to text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    /// Close after receiving N messages (default 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_after: Option<u32>,
    /// Read timeout (e.g. "5s")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_timeout: Option<String>,
    /// Message sequence (the connection is kept across the sequence; defaults to a single message)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<MessageSpec>>,
}

/// gRPC request config (new form: `endpoint` + `service` are required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    /// gRPC endpoint address (e.g. "http://127.0.0.1:50051"; the legacy field name `endpoint` is accepted)
    #[serde(alias = "endpoint")]
    pub url: String,
    /// Fully qualified method name (e.g. "/pkg.Service/Method")
    pub service: String,
    /// Request message (JSON text; defaults to an empty message)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Whether gRPC Server Reflection is enabled
    #[serde(default)]
    pub use_reflection: bool,
    /// Message format (json | protobuf), defaults to json
    #[serde(default = "default_grpc_message_format")]
    pub message_format: String,
    /// Response format hint ("json" converts proto responses to JSON automatically; otherwise protobuf binary is kept)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    /// gRPC metadata (equivalent to HTTP headers)
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Streaming call mode (server/client/bidirectional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<orbit_protocol::types::StreamingMode>,
}

fn default_grpc_message_format() -> String {
    "json".to_string()
}

/// Raw TCP request config (new form: `target` is required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpConfig {
    /// Target address "host:port" (may carry the tcp:// prefix; the legacy field name `target` is accepted)
    #[serde(alias = "target")]
    pub url: String,
    /// Content to send (text); empty by default
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Payload representation (text/base64/hex), defaults to text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    /// Framing mode (delimiter / fixed / read_until_close)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<FramingOptions>,
    /// Read timeout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_timeout: Option<String>,
    /// Message sequence (the connection is kept across the sequence; defaults to a single message)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<MessageSpec>>,
}

/// Raw UDP request config (same shape as TCP; discriminated via `request.protocol: udp` or `step.protocol: udp`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpConfig {
    /// Target address "host:port" (may carry the udp:// prefix; the legacy field name `target` is accepted)
    #[serde(alias = "target")]
    pub url: String,
    /// Content to send
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Payload representation (text/base64/hex), defaults to text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
    /// Read timeout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_timeout: Option<String>,
    /// Message sequence (UDP has connectionless semantics: messages are sent/received one by one)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<MessageSpec>>,
}

/// SSE request config (new form: `url` is required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseConfig {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Maximum number of events to collect (default 50)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_events: Option<u32>,
    /// Subscription duration (e.g. "5s")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
}

/// GraphQL request config (new form: `url` is required)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphqlConfig {
    pub url: String,
    /// GraphQL query/mutation text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Variables (JSON string)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<String>,
    /// operation name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Protocol-agnostic request config - the carrier of `Step::Request.request`.
///
/// Deserialization discrimination order (hand-implemented to avoid untagged ambiguity):
/// 1. the explicit `request.protocol` key (http/websocket/grpc/tcp/udp/sse/graphql);
/// 2. structural traits: `endpoint`+`service` -> gRPC; `target` -> TCP;
///    `query`/`graphql-variables`/`operation_name` -> GraphQL;
/// 3. URL scheme: `ws`/`wss` -> WebSocket; `sse` -> SSE;
/// 4. everything else (including the legacy `method`+`url` form) -> HTTP.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum RequestSpec {
    /// HTTP (legacy form accepted: method + url are required; the legacy side-attached gRPC fields are kept)
    /// Boxed to avoid a bulky variant (HttpRequestConfig holds several HashMaps, far larger than the other protocol configs)
    Http(Box<HttpRequestConfig>),
    WebSocket(WebSocketConfig),
    Grpc(GrpcConfig),
    Tcp(TcpConfig),
    Udp(UdpConfig),
    Sse(SseConfig),
    Graphql(GraphqlConfig),
}

/// Intermediate state of RequestSpec discrimination (private)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecKind {
    Http,
    WebSocket,
    Grpc,
    Tcp,
    Udp,
    Sse,
    Graphql,
}

impl<'de> serde::Deserialize<'de> for RequestSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        let map = value.as_mapping().ok_or_else(|| {
            serde::de::Error::custom("request must be a mapping (key: value form)")
        })?;

        let explicit = map
            .get("protocol")
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase());
        let url = map.get("url").and_then(|v| v.as_str());
        let scheme = url
            .and_then(|u| u.split(':').next())
            .map(str::to_ascii_lowercase);

        let kind = match explicit.as_deref() {
            Some("http") | Some("https") => SpecKind::Http,
            Some("websocket") | Some("ws") | Some("wss") => SpecKind::WebSocket,
            Some("grpc") => SpecKind::Grpc,
            Some("tcp") => SpecKind::Tcp,
            Some("udp") => SpecKind::Udp,
            Some("sse") => SpecKind::Sse,
            Some("graphql") => SpecKind::Graphql,
            _ => {
                // Scheme takes priority: ws/wss -> WebSocket; sse -> SSE; tcp/udp -> the matching raw protocol
                if matches!(scheme.as_deref(), Some("ws") | Some("wss")) {
                    SpecKind::WebSocket
                } else if scheme.as_deref() == Some("sse") || map.contains_key("max_events") {
                    SpecKind::Sse
                } else if matches!(scheme.as_deref(), Some("tcp")) {
                    SpecKind::Tcp
                } else if matches!(scheme.as_deref(), Some("udp")) {
                    SpecKind::Udp
                } else if (map.contains_key("endpoint") || map.contains_key("url"))
                    && map.contains_key("service")
                {
                    SpecKind::Grpc
                } else if map.contains_key("query")
                    || map.contains_key("graphql-variables")
                    || map.contains_key("operation_name")
                {
                    SpecKind::Graphql
                } else if map.contains_key("message") || map.contains_key("message_type") {
                    // message/message_type without service is treated as WebSocket
                    SpecKind::WebSocket
                } else if map.contains_key("target")
                    || (map.contains_key("url") && !map.contains_key("method"))
                {
                    // Raw TCP/UDP (UDP needs an explicit protocol: udp or a udp:// scheme)
                    SpecKind::Tcp
                } else {
                    // The legacy form defaults to HTTP; HttpRequestConfig reports a clear error when method is missing
                    SpecKind::Http
                }
            }
        };

        // Drop the discriminator key, then deserialize by concrete type
        let mut m = map.clone();
        m.remove("protocol");
        let value = serde_yaml::Value::Mapping(m);
        let parsed = match kind {
            SpecKind::Http => serde_yaml::from_value::<HttpRequestConfig>(value)
                .map(Box::new)
                .map(RequestSpec::Http),
            SpecKind::WebSocket => {
                serde_yaml::from_value::<WebSocketConfig>(value).map(RequestSpec::WebSocket)
            }
            SpecKind::Grpc => serde_yaml::from_value::<GrpcConfig>(value).map(RequestSpec::Grpc),
            SpecKind::Tcp => serde_yaml::from_value::<TcpConfig>(value).map(RequestSpec::Tcp),
            SpecKind::Udp => serde_yaml::from_value::<UdpConfig>(value).map(RequestSpec::Udp),
            SpecKind::Sse => serde_yaml::from_value::<SseConfig>(value).map(RequestSpec::Sse),
            SpecKind::Graphql => {
                serde_yaml::from_value::<GraphqlConfig>(value).map(RequestSpec::Graphql)
            }
        };
        parsed.map_err(|e| serde::de::Error::custom(format!("failed to parse request config: {e}")))
    }
}

/// HTTP request config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestConfig {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_yaml::Value>,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    /// Request body format ("json", "protobuf", "msgpack", ...)
    /// When set to "json" with gRPC, the JSON body is serialized to protobuf binary automatically
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "request_format")]
    pub payload_format: Option<String>,
    /// Response format hint ("json", "msgpack", ...); inferred from the response Content-Type by default
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    /// gRPC service name (e.g. "/package.Service/Method")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc_service: Option<String>,
    /// Whether gRPC reflection is enabled
    #[serde(default)]
    pub grpc_use_reflection: bool,
}

fn default_timeout() -> String {
    "30s".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Step;

    #[test]
    fn test_request_spec_legacy_http() {
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          method: POST
          url: "https://example.com/api"
          headers: { X-Test: "1" }
          request_format: json
          response_format: msgpack
"#,
        )
        .unwrap();
        if let Step::Request { request, .. } = &plan.scenarios[0].steps[0] {
            match request {
                RequestSpec::Http(h) => {
                    assert_eq!(h.method, "POST");
                    assert_eq!(h.url, "https://example.com/api");
                    // request_format is an alias of payload_format
                    assert_eq!(h.payload_format.as_deref(), Some("json"));
                    assert_eq!(h.response_format.as_deref(), Some("msgpack"));
                }
                other => panic!("expected Http, got {:?}", other),
            }
        } else {
            panic!("expected Request step");
        }
    }

    #[test]
    fn test_request_spec_typed_variants() {
        // WebSocket: discriminated by URL scheme
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          url: "wss://echo.example.com/ws"
          message: "hello"
          message_type: binary
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::WebSocket(w),
                ..
            } => {
                assert_eq!(w.url, "wss://echo.example.com/ws");
                assert_eq!(w.message.as_deref(), Some("hello"));
                assert_eq!(w.message_type, WsMessageType::Binary);
            }
            other => panic!("expected WebSocket, got {:?}", other),
        }

        // gRPC: discriminated by the endpoint + service structure
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          url: "http://127.0.0.1:50051"
          service: "/pkg.Service/Method"
          message: '{"name":"x"}'
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Grpc(g),
                ..
            } => {
                assert_eq!(g.url, "http://127.0.0.1:50051");
                assert_eq!(g.service, "/pkg.Service/Method");
                assert_eq!(g.message.as_deref(), Some(r#"{"name":"x"}"#));
            }
            other => panic!("expected Grpc, got {:?}", other),
        }

        // The legacy field names endpoint / target are still accepted
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          endpoint: "http://127.0.0.1:50051"
          service: "/a.B/C"
      - type: request
        request:
          protocol: tcp
          target: "h:2"
"#,
        )
        .unwrap();
        assert!(matches!(
            &plan.scenarios[0].steps[0],
            Step::Request {
                request: RequestSpec::Grpc(_),
                ..
            }
        ));
        assert!(matches!(
            &plan.scenarios[0].steps[1],
            Step::Request {
                request: RequestSpec::Tcp(_),
                ..
            }
        ));

        // UDP: discriminated by an explicit protocol key inside request (TCP and UDP share the same shape)
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          protocol: udp
          target: "127.0.0.1:9000"
          payload: "PING"
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Udp(u),
                ..
            } => {
                assert_eq!(u.url, "127.0.0.1:9000");
                assert_eq!(u.payload.as_deref(), Some("PING"));
            }
            other => panic!("expected Udp, got {:?}", other),
        }

        // TCP: discriminated by the target structure
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          url: "tcp://127.0.0.1:9001"
          framing: { mode: delimiter, delimiter: "\r\n" }
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Tcp(t),
                ..
            } => {
                assert_eq!(t.url, "tcp://127.0.0.1:9001");
                let f = t.framing.as_ref().expect("framing");
                assert_eq!(f.mode, orbit_protocol::FramingMode::Delimiter);
            }
            other => panic!("expected Tcp, got {:?}", other),
        }

        // GraphQL: discriminated by the query structure
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          url: "https://api.example.com/graphql"
          query: "{ user { id } }"
          variables: '{"id":1}'
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Graphql(g),
                ..
            } => {
                assert_eq!(g.query.as_deref(), Some("{ user { id } }"));
                assert_eq!(g.variables.as_deref(), Some(r#"{"id":1}"#));
            }
            other => panic!("expected Graphql, got {:?}", other),
        }

        // SSE: discriminated by the max_events structure
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          url: "https://httpbin.org/stream/2"
          max_events: 10
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Sse(s),
                ..
            } => {
                assert_eq!(s.url, "https://httpbin.org/stream/2");
                assert_eq!(s.max_events, Some(10));
            }
            other => panic!("expected Sse, got {:?}", other),
        }
    }

    #[test]
    fn test_request_spec_typed_protocol_resolution() {
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { url: "wss://x/ws", message_type: text }
      - type: request
        request: { url: "http://h:1", service: "/a.B/C" }
      - type: request
        request: { url: "h:2", protocol: udp }
      - type: request
        request: { url: "https://h/graphql", query: "{x}" }
"#,
        )
        .unwrap();
        let steps = &plan.scenarios[0].steps;
        assert_eq!(
            steps[0].resolve_protocol(),
            orbit_protocol::ProtocolKind::WebSocket
        );
        assert_eq!(
            steps[1].resolve_protocol(),
            orbit_protocol::ProtocolKind::Grpc
        );
        assert_eq!(
            steps[2].resolve_protocol(),
            orbit_protocol::ProtocolKind::Udp
        );
        assert_eq!(
            steps[3].resolve_protocol(),
            orbit_protocol::ProtocolKind::Graphql
        );
    }

    #[test]
    fn test_message_sequence_parse() {
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request:
          protocol: tcp
          url: "127.0.0.1:9000"
          framing: { mode: length_prefix, fixed_len: 4 }
          messages:
            - payload: "hello"
              payload_type: text
              pre_script: "pm.request.raw = pm.utf8.encode(pm.request.body);"
              post_script: "pm.response.decoded = pm.utf8.decode(pm.response.raw);"
            - payload: "AA55"
              payload_type: hex
              wait_ms: 50
"#,
        )
        .unwrap();
        match &plan.scenarios[0].steps[0] {
            Step::Request {
                request: RequestSpec::Tcp(t),
                ..
            } => {
                let msgs = t.messages.as_ref().expect("messages");
                assert_eq!(msgs.len(), 2);
                assert_eq!(msgs[0].payload.as_deref(), Some("hello"));
                assert_eq!(msgs[0].payload_type, Some(PayloadType::Text));
                assert!(msgs[0].pre_script.is_some());
                assert_eq!(msgs[1].payload_type, Some(PayloadType::Hex));
                assert_eq!(msgs[1].wait_ms, 50);
            }
            other => panic!("expected Tcp, got {:?}", other),
        }
    }
}
