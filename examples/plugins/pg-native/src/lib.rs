//! Orbit 动态库（native）协议插件示例：PostgreSQL。
//!
//! 直接复用 sqlx 生态（自带连接池 PgPool），插件内部维护连接池并长期存活，
//! 宿主不介入网络/加密/连接池。这是 native 插件相比 wasm 插件的核心优势：
//! 开发者用 sqlx 等成熟库，无需手写 wire protocol 或 SCRAM。

use std::sync::OnceLock;

use base64::Engine;
use orbit_plugin_api_native::{
    NativePlugin, NativePluginInfo, NativeProtocolRequest, NativeProtocolResponse,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::{Column, Row, TypeInfo};

/// 插件实例：默认无状态，连接池经 `OnceLock` 缓存
#[derive(Default)]
struct PgNative;

/// 插件级全局 tokio runtime（进程内单例，sqlx 的 pool 与之绑定）
fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("pg-native runtime")
    })
}

/// 连接池缓存（按 database_url 池化；`&self` 并发安全）
static POOLS: OnceLock<std::sync::Mutex<std::collections::HashMap<String, sqlx::PgPool>>> =
    OnceLock::new();

fn pools() -> &'static std::sync::Mutex<std::collections::HashMap<String, sqlx::PgPool>> {
    POOLS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 解析连接配置为 sqlx URL：支持 connection.host/port/username/password/database 或 url 直传
/// 把一列 `PgRow` 的值转成 JSON，按 PG 类型用 sqlx 的正确解码（而非手动 binary 解码）。
/// 覆盖：整数/浮点/bool → 数字；时间戳/日期/时间 → 可读字符串（chrono）；
/// json/jsonb/uuid → 文本；bytea → hex；文本 → UTF-8。
/// 注：sqlx 0.8 的 chrono 集成成熟稳定，本示例插件保留 chrono；
/// 全 workspace 其余代码已迁移 jiff（jiff-sqlx 0.2 需 sqlx 0.9，未采用）。
fn pg_cell_to_json(row: &sqlx::postgres::PgRow, idx: usize, type_name: &str) -> serde_json::Value {
    use sqlx::Row;
    let tn = type_name.to_uppercase();
    match tn.as_str() {
        "INT2" => row
            .try_get::<i16, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "INT4" => row
            .try_get::<i32, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "INT8" => row
            .try_get::<i64, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "FLOAT4" => row
            .try_get::<f32, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "FLOAT8" => row
            .try_get::<f64, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "BOOL" => row
            .try_get::<bool, _>(idx)
            .map(|v| serde_json::json!(v))
            .unwrap_or(serde_json::Value::Null),
        "TIMESTAMP" | "TIMESTAMP WITHOUT TIME ZONE" => row
            .try_get::<chrono::NaiveDateTime, _>(idx)
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => row
            .try_get::<chrono::DateTime<chrono::Utc>, _>(idx)
            .map(|v| serde_json::Value::String(v.to_rfc3339()))
            .unwrap_or(serde_json::Value::Null),
        "DATE" => row
            .try_get::<chrono::NaiveDate, _>(idx)
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "TIME" => row
            .try_get::<chrono::NaiveTime, _>(idx)
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "TIMETZ" => row
            .try_get::<chrono::NaiveTime, _>(idx)
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "BYTEA" => row
            .try_get::<Vec<u8>, _>(idx)
            .map(|v| serde_json::Value::String(format!("0x{}", hex_encode(&v))))
            .unwrap_or(serde_json::Value::Null),
        "JSON" | "JSONB" => row
            .try_get::<serde_json::Value, _>(idx)
            .unwrap_or(serde_json::Value::Null),
        // 其余（text/varchar/uuid/char/enum/...）：按文本；若是 null 则保持 null
        _ => match row.try_get::<Option<String>, _>(idx) {
            Ok(None) => serde_json::Value::Null,
            Ok(Some(s)) => serde_json::from_str::<serde_json::Value>(&s)
                .unwrap_or(serde_json::Value::String(s)),
            Err(_) => serde_json::Value::Null,
        },
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn build_url(conn: &Option<serde_json::Value>) -> String {
    if let Some(v) = conn {
        if let Some(url) = v.get("url").and_then(|x| x.as_str()) {
            if !url.is_empty() {
                return url.to_string();
            }
        }
        let host = v
            .get("host")
            .and_then(|x| x.as_str())
            .unwrap_or("127.0.0.1");
        let port = v.get("port").and_then(|x| x.as_u64()).unwrap_or(5432);
        let user = v
            .get("username")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("user").and_then(|x| x.as_str()))
            .unwrap_or("postgres");
        let password = v.get("password").and_then(|x| x.as_str()).unwrap_or("");
        let database = v
            .get("database")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("dbname").and_then(|x| x.as_str()))
            .unwrap_or("postgres");
        return format!(
            "postgres://{}:{}@{}:{}/{}",
            user, password, host, port, database
        );
    }
    // 无连接配置 → 默认本地
    "postgres://postgres@127.0.0.1:5432/postgres".to_string()
}

impl NativePlugin for PgNative {
    fn info(&self) -> NativePluginInfo {
        NativePluginInfo {
            name: "PostgreSQL (native/sqlx)".into(),
            version: "1.0.0".into(),
            description:
                "PostgreSQL 动态库协议插件，使用 sqlx 连接池，支持 SCRAM/TLS 等全部 sqlx 能力。"
                    .into(),
            kind: "protocol".into(),
            capabilities: vec![orbit_plugin_api_native::NativeCapability {
                protocol_id: "pg".into(),
                display_name: "PostgreSQL".into(),
                description: "PostgreSQL 查询（sqlx 驱动）。".into(),
            }],
            connection_config_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "title": "主机", "default": "127.0.0.1" },
                    "port": { "type": "integer", "title": "端口", "default": 5432 },
                    "username": { "type": "string", "title": "用户名", "default": "postgres" },
                    "password": { "type": "string", "title": "密码" },
                    "database": { "type": "string", "title": "数据库", "default": "postgres" }
                }
            })),
            request_config_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "title": "SQL", "format": "textarea" }
                }
            })),
        }
    }

    fn execute(&self, req: NativeProtocolRequest) -> Result<NativeProtocolResponse, String> {
        let url = build_url(&req.connection);

        // 提取 SQL：优先 payload（UTF-8），否则 options.sql
        let sql = if !req.payload_b64.is_empty() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&req.payload_b64)
                .map_err(|e| format!("payload base64 解码失败: {}", e))?;
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            req.options
                .as_ref()
                .and_then(|o| o.get("sql").and_then(|x| x.as_str()))
                .unwrap_or("SELECT 1")
                .to_string()
        };

        // sqlx 是 async；整个池获取 + 查询都跑在插件级全局 runtime 的 async 上下文内，
        // 避免脱离 tokio context（动态库在宿主进程，block_on 全局 runtime 安全）。
        let (columns, rows) = rt()
            .block_on(async {
                // 复用连接池（sqlx PgPool 自带池化、TLS、SCRAM 认证）
                let pool = {
                    let cached = {
                        let g = pools().lock().map_err(|_| "连接池锁失败".to_string())?;
                        g.get(&url).cloned()
                    };
                    if let Some(p) = cached {
                        p
                    } else {
                        let p = PgPoolOptions::new()
                            .max_connections(5)
                            .connect(&url)
                            .await
                            .map_err(|e| format!("连接 {} 失败: {}", url, e))?;
                        let mut g = pools().lock().map_err(|_| "连接池锁失败".to_string())?;
                        g.entry(url.clone()).or_insert_with(|| p.clone());
                        p
                    }
                };

                let rows = sqlx::query(&sql)
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| e.to_string())?;
                // 提取列名（首行）
                let cols: Vec<String> = rows
                    .first()
                    .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
                    .unwrap_or_default();
                // 列类型名（首行）
                let col_types: Vec<String> = rows
                    .first()
                    .map(|r| {
                        r.columns()
                            .iter()
                            .map(|c| c.type_info().name().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let vals: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|r| {
                        let mut map = serde_json::Map::new();
                        for (idx, c) in cols.iter().enumerate() {
                            let tn = col_types.get(idx).cloned().unwrap_or_default();
                            let val = pg_cell_to_json(r, idx, &tn);
                            map.insert(c.clone(), val);
                        }
                        serde_json::Value::Object(map)
                    })
                    .collect();
                Ok::<(Vec<String>, Vec<serde_json::Value>), String>((cols, vals))
            })
            .map_err(|e| format!("SQL 执行失败: {}", e))?;

        let result = serde_json::json!({
            "columns": columns,
            "rows": rows,
            "rowCount": rows.len(),
        });

        Ok(NativeProtocolResponse {
            status_code: 200,
            payload_b64: base64::engine::general_purpose::STANDARD
                .encode(result.to_string().as_bytes()),
            metadata: vec![("content-type".to_string(), "application/json".to_string())],
            timings: serde_json::json!({}),
        })
    }
}

orbit_plugin_api_native::export_plugin!(PgNative);
