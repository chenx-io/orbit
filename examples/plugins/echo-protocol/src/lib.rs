//! Orbit 示例插件：echo 协议（桩）。
//!
//! 演示 orbit `protocol-plugin` world 的完整实现：
//! - `get-capabilities`：声明协议 id = `echo`
//! - `execute`：把入参原样回显（含 metadata / payload）
//! - `disconnect`：断开由宿主持有的连接
//!
//! 构建：`cargo build --target wasm32-wasip2 --release`
//! 产物：`target/wasm32-wasip2/release/echo_protocol.wasm`
//! 打包：`com.example.echo/`（manifest.json + echo.wasm）压缩为 zip 后安装。

wit_bindgen::generate!({
    path: "../../../crates/orbit-plugin-api/wit/protocol/protocol.wit",
    world: "protocol-plugin",
});

use exports::orbit::protocol_plugin::protocol::{Capability, Request, Response, Timing};

/// 把 JSON 字符串解析为对象，取指定字段；失败返回默认值。
fn json_get(s: Option<&str>, key: &str) -> String {
    let Some(s) = s else { return String::new() };
    serde_json::from_str::<serde_json::Value>(s)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_str()).map(String::from))
        .unwrap_or_default()
}

struct EchoProtocol;

// 接口 protocol Guest：导出 get-capabilities / execute / disconnect
impl exports::orbit::protocol_plugin::protocol::Guest for EchoProtocol {
    fn get_capabilities() -> Vec<Capability> {
        vec![Capability {
            protocol_id: "echo".to_string(),
            display_name: "Echo (示例)".to_string(),
            supports_streaming: false,
            description: "回显入参的协议桩，用于验证插件链路。".to_string(),
        }]
    }

    fn execute(req: Request) -> Result<Response, String> {
        // 连接配置（connectionConfigSchema 的键）：本桩用 prefix 作为回显前缀
        let prefix = json_get(req.connection.as_deref(), "prefix");
        // 消息参数（requestConfigSchema 的键）：本桩用 separator 连接 metadata
        let separator = json_get(req.options.as_deref(), "separator");
        let sep = if separator.is_empty() {
            ","
        } else {
            separator.as_str()
        };

        // 组装回显 payload：prefix + 入参 metadata + 原始 payload
        let meta: Vec<String> = req
            .metadata
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let mut payload = prefix.as_bytes().to_vec();
        if !meta.is_empty() {
            if !payload.is_empty() {
                payload.extend_from_slice(b"|");
            }
            payload.extend_from_slice(meta.join(sep).as_bytes());
        }
        payload.extend_from_slice(&req.payload);

        Ok(Response {
            status_code: 200,
            metadata: vec![("x-plugin".to_string(), "echo".to_string())],
            payload,
            timings: Timing {
                dns_ms: Some(0),
                tcp_ms: Some(1),
                tls_ms: None,
                send_ms: Some(0),
                ttfb_ms: Some(1),
                receive_ms: Some(0),
                total_ms: 1,
            },
        })
    }

    fn disconnect() -> Result<(), String> {
        // 宿主持有连接句柄，本桩无需显式关闭
        Ok(())
    }
}

export!(EchoProtocol);
