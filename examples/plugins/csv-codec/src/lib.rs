//! Orbit 示例插件：CSV/TSV 编解码（桩）。
//!
//! 演示 orbit `codec-plugin` world 的完整实现（纯函数、无网络）：
//! - `get-capabilities`：声明格式名 `tsv`
//! - `encode`：把 JSON 编码的 DataValue → TSV 字节（每对象一行，`\t` 分隔）
//! - `decode`：把 TSV 字节 → JSON 编码的 DataValue（对象数组）
//!
//! 构建：`cargo build --target wasm32-wasip2 --release`
//! 打包：`com.example.tsv-codec/`（manifest.json + tsv_codec.wasm）压缩为 zip 后安装。

wit_bindgen::generate!({
    path: "../../../crates/orbit-plugin-api/wit/codec/codec.wit",
    world: "codec-plugin",
});

use exports::orbit::codec_plugin::codec::{
    Capability, DecodeRequest, DecodeResult, EncodeRequest, EncodeResult, Guest,
};

struct TsvCodec;

fn field_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Guest for TsvCodec {
    fn get_capabilities() -> Vec<Capability> {
        vec![Capability {
            codec_name: "tsv".to_string(),
            mime_types: vec!["text/tab-separated-values".to_string()],
            display_name: "TSV (示例编解码)".to_string(),
        }]
    }

    fn encode(req: EncodeRequest) -> Result<EncodeResult, String> {
        // value 是 JSON 编码的 DataValue；支持：对象数组 / 单个对象
        let data: serde_json::Value =
            serde_json::from_str(&req.value).map_err(|e| format!("非法 DataValue: {}", e))?;
        let rows: Vec<&serde_json::Value> = match &data {
            serde_json::Value::Array(arr) => arr.iter().collect(),
            _ => vec![&data],
        };
        // 收集全部键（保持顺序），构造表头
        let mut headers: Vec<String> = Vec::new();
        for r in &rows {
            if let serde_json::Value::Object(map) = r {
                for k in map.keys() {
                    if !headers.iter().any(|h| h == k) {
                        headers.push(k.clone());
                    }
                }
            }
        }
        let mut lines: Vec<String> = Vec::new();
        if !headers.is_empty() {
            lines.push(headers.join("\t"));
        }
        for r in &rows {
            let mut cells = Vec::with_capacity(headers.len());
            for h in &headers {
                let cell = r.get(h).map(field_to_string).unwrap_or_default();
                cells.push(cell);
            }
            lines.push(cells.join("\t"));
        }
        Ok(EncodeResult {
            bytes: lines.join("\n").into_bytes(),
        })
    }

    fn decode(req: DecodeRequest) -> Result<DecodeResult, String> {
        let text = String::from_utf8(req.bytes).map_err(|e| format!("非法 UTF-8: {}", e))?;
        let mut lines = text.lines();
        // 首行为表头；无表头则逐行作为单列数组
        let header: Vec<&str> = lines
            .next()
            .map(|l| l.split('\t').collect())
            .unwrap_or_default();
        let mut rows: Vec<serde_json::Value> = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.split('\t').collect();
            if header.is_empty() {
                rows.push(serde_json::Value::Array(
                    cells
                        .iter()
                        .map(|c| serde_json::Value::String(c.to_string()))
                        .collect(),
                ));
                continue;
            }
            let mut map = serde_json::Map::new();
            for (i, h) in header.iter().enumerate() {
                let v = cells
                    .get(i)
                    .map(|c| serde_json::Value::String(c.to_string()));
                map.insert(h.to_string(), v.unwrap_or(serde_json::Value::Null));
            }
            rows.push(serde_json::Value::Object(map));
        }
        Ok(DecodeResult {
            value: serde_json::to_string(&serde_json::Value::Array(rows))
                .map_err(|e| format!("序列化失败: {}", e))?,
        })
    }
}

export!(TsvCodec);
