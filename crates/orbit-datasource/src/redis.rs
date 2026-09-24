//! Redis read-only command execution (connection manager + command allowlist + value stringification).

use std::time::Duration;

use orbit_config::DataSourceConfig;
use redis::aio::ConnectionManager;
use redis::{RedisResult, Value as RValue};

use crate::error::DsError;

/// Read-only command allowlist: the assertion context allows only reads, forbidding writes like FLUSHALL/SET/DEL.
const READONLY_COMMANDS: &[&str] = &[
    "GET",
    "MGET",
    "HGET",
    "HGETALL",
    "HMGET",
    "EXISTS",
    "TTL",
    "PTTL",
    "TYPE",
    "LLEN",
    "LRANGE",
    "SCARD",
    "SMEMBERS",
    "SISMEMBER",
    "SINTER",
    "SUNION",
    "ZSCORE",
    "ZCARD",
    "ZCOUNT",
    "ZRANGE",
    "STRLEN",
    "GETRANGE",
    "HEXISTS",
    "HKEYS",
    "HVALS",
    "HLEN",
    "DBSIZE",
    "KEYS",
    "RANDOMKEY",
    "PING",
    "ECHO",
];

/// Execute a command on the connection manager (`args[0]` is the command name).
///
/// When read-only is on, the command must be allowlisted; return values are uniformly stringified:
/// simple strings/integers as-is, `(nil)` for missing, bulk/arrays presented as JSON.
pub(crate) async fn command(
    manager: &mut ConnectionManager,
    cfg: &DataSourceConfig,
    args: &[String],
) -> Result<String, DsError> {
    let cmd_name = args.first().map(|s| s.as_str()).unwrap_or("");
    if cmd_name.is_empty() {
        return Err(DsError::Other("Redis command name is empty".to_string()));
    }
    if cfg.readonly && !READONLY_COMMANDS.contains(&cmd_name.to_ascii_uppercase().as_str()) {
        return Err(DsError::Readonly(format!(
            "{}: command `{cmd_name}` is forbidden in Redis read-only mode",
            cfg.name
        )));
    }
    let timeout = Duration::from_millis(cfg.query_timeout_ms.max(1));
    let mut cmd = redis::cmd(cmd_name);
    for a in args.iter().skip(1) {
        cmd.arg(a);
    }
    let fut = async {
        let v: RValue = cmd.query_async(manager).await?;
        Ok::<_, redis::RedisError>(v)
    };
    let value: RedisResult<RValue> = match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r,
        Err(_) => {
            return Err(DsError::Timeout {
                ctx: format!("{} Redis command {cmd_name} timed out", cfg.name),
            });
        }
    };
    match value {
        Ok(v) => Ok(stringify(&v)),
        Err(e) => Err(DsError::Query {
            name: cfg.name.clone(),
            detail: format!("{cmd_name}: {e}"),
        }),
    }
}

/// Redis return value -> text.
fn stringify(v: &RValue) -> String {
    to_json(v)
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| to_json(v).to_string())
}

/// redis::Value -> serde_json::Value (arrays/Map/Set converted recursively; scalars by text semantics).
fn to_json(v: &RValue) -> serde_json::Value {
    match v {
        RValue::Nil => serde_json::Value::String("(nil)".to_string()),
        RValue::Int(i) => serde_json::json!(*i),
        RValue::BulkString(bytes) => {
            serde_json::Value::String(String::from_utf8_lossy(bytes).into_owned())
        }
        RValue::SimpleString(s) => serde_json::Value::String(s.clone()),
        RValue::Okay => serde_json::Value::String("OK".to_string()),
        RValue::Double(d) => serde_json::json!(d),
        RValue::Boolean(b) => serde_json::json!(b),
        RValue::VerbatimString { text, .. } => serde_json::Value::String(text.clone()),
        RValue::Array(items) | RValue::Set(items) | RValue::Push { data: items, .. } => {
            serde_json::Value::Array(items.iter().map(to_json).collect())
        }
        RValue::Map(pairs) => {
            let map = pairs
                .iter()
                .filter_map(|(k, val)| match &k {
                    RValue::BulkString(b) => {
                        Some((String::from_utf8_lossy(b).into_owned(), to_json(val)))
                    }
                    other => {
                        let kt = to_json(other);
                        kt.as_str().map(|s| (s.to_string(), to_json(val)))
                    }
                })
                .collect();
            serde_json::Value::Object(map)
        }
        RValue::Attribute { data, .. } => to_json(data),
        RValue::BigNumber(n) => serde_json::Value::String(n.to_string()),
        RValue::ServerError(e) => serde_json::Value::String(format!("{e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stringify_basics() {
        assert_eq!(stringify(&RValue::Nil), "(nil)");
        assert_eq!(stringify(&RValue::Int(5)), "5");
        assert_eq!(stringify(&RValue::BulkString(b"hello".to_vec())), "hello");
        assert_eq!(
            stringify(&RValue::Array(vec![
                RValue::BulkString(b"a".to_vec()),
                RValue::Int(2),
            ])),
            r#"["a",2]"#
        );
    }
}
