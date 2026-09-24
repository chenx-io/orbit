//! Orbit 示例插件：状态码范围断言（二期）。
//!
//! 演示 orbit `assertion-plugin` world 的完整实现（纯函数）：
//! - `get-capabilities`：声明断言能力 `status-range`
//! - `run`：解析响应摘要中的 HTTP 状态码，检查是否落在配置区间
//!
//! 配置（options，JSON）：
//! ```json
//! { "min": 200, "max": 299 }
//! ```
//! 响应摘要（response，JSON）示例：
//! ```json
//! { "status": 200, "body": "..." }
//! ```

wit_bindgen::generate!({
    path: "../../../crates/orbit-plugin-api/wit/assertion/assertion.wit",
    world: "assertion-plugin",
});

use exports::orbit::assertion_plugin::assertion::{
    AssertionRequest, AssertionResult, Capability, Guest,
};

struct StatusAssertion;

/// 从响应摘要 JSON 中取 HTTP 状态码
fn status_of(response: Option<&str>) -> Option<i64> {
    let s = response?;
    serde_json::from_str::<serde_json::Value>(s)
        .ok()
        .and_then(|v| v.get("status").and_then(|x| x.as_i64()))
}

impl Guest for StatusAssertion {
    fn get_capabilities() -> Vec<Capability> {
        vec![Capability {
            name: "status-range".to_string(),
            display_name: "状态码范围".to_string(),
            description: "断言 HTTP 状态码落在配置区间（options: {min,max}）。".to_string(),
        }]
    }

    fn run(req: AssertionRequest) -> Result<AssertionResult, String> {
        // 解析配置区间
        let (min, max) = req
            .options
            .as_deref()
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .map(|v| {
                (
                    v.get("min").and_then(|x| x.as_i64()).unwrap_or(200),
                    v.get("max").and_then(|x| x.as_i64()).unwrap_or(299),
                )
            })
            .unwrap_or((200, 299));

        // 提取状态码（优先 response 摘要；也可从 actual 中找 status）
        let status = status_of(req.response.as_deref()).or_else(|| {
            serde_json::from_str::<serde_json::Value>(&req.actual)
                .ok()
                .and_then(|v| v.get("status").and_then(|x| x.as_i64()))
        });

        match status {
            Some(code) if code >= min && code <= max => Ok(AssertionResult {
                passed: true,
                message: format!("状态码 {} 在 [{}, {}] 内", code, min, max),
            }),
            Some(code) => Ok(AssertionResult {
                passed: false,
                message: format!("状态码 {} 不在 [{}, {}] 内", code, min, max),
            }),
            None => Err("响应中未找到 status 字段".to_string()),
        }
    }
}

export!(StatusAssertion);
