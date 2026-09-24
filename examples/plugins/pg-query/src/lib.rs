//! Orbit 协议插件：PostgreSQL 查询。
//!
//! 实现 PG wire protocol 的最小可运行客户端（握手 + trust/cleartext/md5/SCRAM-SHA-256 认证 + 简单查询 + 结果解析）：
//! - `get-capabilities`：声明协议 id = `pg`
//! - `execute`：从 `connection`(JSON) 解析连接参数，从 `payload`(UTF-8) 取 SQL，
//!   经 host-transport 建立 TCP 连接执行查询，返回 JSON 结果（列名 + 行值 + 行数）
//! - `disconnect`：关闭连接
//!
//! connection JSON 字段：`host` / `port` / `username` / `password` / `database`
//! requestConfigSchema 字段：`sql`（也可通过请求体 payload 传入 SQL，payload 优先）
//!
//! 构建：`cargo build --target wasm32-wasip2 --release`
//! 打包：`com.example.pg/`（manifest.json + pg_query.wasm）压缩为 zip 后安装。

wit_bindgen::generate!({
    path: "../../../crates/orbit-plugin-api/wit/protocol/protocol.wit",
    world: "protocol-plugin",
});

use exports::orbit::protocol_plugin::protocol::{Capability, Request, Response, Timing};
use orbit::protocol_plugin::host_transport::{
    close as host_close, connect as host_connect, recv as host_recv, send as host_send,
};

// ─── PG wire protocol 常量 ─────────────────────────────────

/// 协议版本号（3.0）
const PROTOCOL_VERSION: i32 = 196608; // (3 << 16) | 0

// 后端消息类型
const MSG_AUTHENTICATION: u8 = b'R';
const MSG_READY_FOR_QUERY: u8 = b'Z';
const MSG_ERROR_RESPONSE: u8 = b'E';
const MSG_ROW_DESCRIPTION: u8 = b'T';
const MSG_DATA_ROW: u8 = b'D';
const MSG_COMMAND_COMPLETE: u8 = b'C';
const MSG_EMPTY_QUERY: u8 = b'I';

// 前端消息类型
const MSG_PASSWORD: u8 = b'p';
const MSG_QUERY: u8 = b'Q';
const MSG_TERMINATE: u8 = b'X';

// 认证方式
const AUTH_OK: i32 = 0;
const AUTH_CLEARTEXT: i32 = 3;
const AUTH_MD5: i32 = 5;
const AUTH_SASL: i32 = 10;
const AUTH_SASL_CONTINUE: i32 = 11;
const AUTH_SASL_FINAL: i32 = 12;

// SCRAM-SHA-256 机制名
const SCRAM_SHA256: &str = "SCRAM-SHA-256";

// ─── 连接配置解析 ─────────────────────────────────

#[derive(Debug, Clone)]
struct PgConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    database: String,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5432,
            username: "postgres".into(),
            password: String::new(),
            database: "postgres".into(),
        }
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn parse_connection(conn: Option<&str>) -> PgConfig {
    let mut cfg = PgConfig::default();
    if let Some(s) = conn {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            cfg.host = json_str(&v, "host").if_not_empty_or(cfg.host);
            cfg.username = json_str(&v, "username").if_not_empty_or(cfg.username);
            cfg.username = json_str(&v, "user").if_not_empty_or(cfg.username);
            cfg.password = json_str(&v, "password");
            cfg.database = json_str(&v, "database").if_not_empty_or(cfg.database);
            cfg.database = json_str(&v, "dbname").if_not_empty_or(cfg.database);
            cfg.port = v
                .get("port")
                .and_then(|x| x.as_u64())
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(cfg.port);
        }
    }
    cfg
}

trait DefaultIfEmpty {
    fn if_not_empty_or(self, default: String) -> String;
}
impl DefaultIfEmpty for String {
    fn if_not_empty_or(self, default: String) -> String {
        if self.is_empty() {
            default
        } else {
            self
        }
    }
}

// ─── PG wire 编解码（基于 host-transport） ─────────────────

/// 读取 N 字节，直到读满；失败返回错误
fn read_n(conn: u32, n: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(n);
    let mut remaining = n;
    while remaining > 0 {
        let chunk = host_recv(conn, remaining as u64).map_err(|e| format!("recv: {}", e))?;
        if chunk.is_empty() {
            return Err("连接被对端关闭".into());
        }
        out.extend_from_slice(&chunk);
        remaining -= chunk.len();
    }
    Ok(out)
}

fn read_i32(conn: u32) -> Result<i32, String> {
    let b = read_n(conn, 4)?;
    Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// 读取一个后端消息：type 字节 + 长度头 + body
fn read_message(conn: u32) -> Result<(u8, Vec<u8>), String> {
    let ty = read_n(conn, 1)?[0];
    let len = read_i32(conn)?;
    if len < 4 {
        return Err(format!("非法消息长度 {}", len));
    }
    let body = read_n(conn, (len - 4) as usize)?;
    Ok((ty, body))
}

/// 发送前端消息：type + i32 len + body
fn send_message(conn: u32, ty: u8, body: &[u8]) -> Result<(), String> {
    let mut frame = Vec::with_capacity(1 + 4 + body.len());
    frame.push(ty);
    frame.extend_from_slice(&((body.len() as i32 + 4).to_be_bytes()));
    frame.extend_from_slice(body);
    host_send(conn, &frame).map_err(|e| format!("send: {}", e))
}

// ─── 握手与认证 ─────────────────────────────────

fn startup_message(cfg: &PgConfig) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    // 参数以 key\0value\0 形式
    let params: Vec<(&str, &str)> = vec![
        ("user", &cfg.username),
        ("database", &cfg.database),
        ("client_encoding", "UTF8"),
        ("application_name", "orbit-plugin"),
    ];
    for (k, v) in params {
        body.extend_from_slice(k.as_bytes());
        body.push(0);
        body.extend_from_slice(v.as_bytes());
        body.push(0);
    }
    body.push(0); // 参数区结束
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&((body.len() as i32 + 4).to_be_bytes()));
    frame.extend_from_slice(&body);
    frame
}

fn md5_hex(data: &[u8]) -> String {
    use md5::Digest;
    use md5::Md5;
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// ─── SCRAM-SHA-256（RFC 5802） ─────────────────────────────────

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use sha2::{Digest as Sha2Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("base64 decode: {}", e))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256(data: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().to_vec()
}

/// SASLprep 简化 + 用户名转义（`=`→`=3D`，`,`→`=2C`；空白字符 `n=,` 前的用户名为 UTF8）
fn scram_safe_name(user: &str) -> String {
    user.replace('=', "=3D").replace(',', "=2C")
}

/// 从 client/server message 中解析键值对（逗号分隔）
fn parse_scram_attrs(msg: &str) -> std::collections::HashMap<&str, &str> {
    let mut map = std::collections::HashMap::new();
    for part in msg.split(',') {
        if let Some(eq) = part.find('=') {
            let (k, v) = part.split_at(eq);
            map.insert(k, &v[1..]);
        }
    }
    map
}

/// 生成 client nonce（16 字节随机 → base64；wasip2 下经宿主 wasi:random）
fn client_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    base64_encode(&bytes)
}

/// SCRAM 认证主流程（PG SASL：SASLInitialResponse → Continue → Final）
///
/// `body`：`AuthenticationSASL` 消息体，NUL 分隔的机制列表。
fn scram_authenticate(conn: u32, cfg: &PgConfig, sasl_body: &[u8]) -> Result<(), String> {
    // 1) 确认服务器支持 SCRAM-SHA-256
    // sasl_body 前 4 字节是 auth type(10)，其后为 NUL 分隔的机制列表
    let mechs = if sasl_body.len() >= 4 {
        &sasl_body[4..]
    } else {
        sasl_body
    };
    let mechanisms: Vec<String> = mechs
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if !mechanisms.iter().any(|m| m == SCRAM_SHA256) {
        return Err(format!(
            "PG 未提供 SCRAM-SHA-256（实际: {}）",
            mechanisms.join(", ")
        ));
    }

    // 2) client-first-message: n=<user>,r=<nonce>
    let c_nonce = client_nonce();
    let user = scram_safe_name(&cfg.username);
    let client_first_bare = format!("n={},r={}", user, c_nonce);
    // client-first-message = gs2-header("n,,") + client-first-bare（无额外分隔）
    let client_first_message = format!("{}{}", "n,,", client_first_bare);

    // 3) 发送 SASLInitialResponse（`p`）：机制名 + \0 + int32 len + client-first
    let mut initial = SCRAM_SHA256.as_bytes().to_vec();
    initial.push(0);
    initial.extend_from_slice(&(client_first_message.len() as i32).to_be_bytes());
    initial.extend_from_slice(client_first_message.as_bytes());
    send_message(conn, MSG_PASSWORD, &initial)?;

    // 4) 读 AuthenticationSASLContinue (11) → server-first-message
    let server_first = {
        let (ty, body) = read_message(conn)?;
        if ty == MSG_ERROR_RESPONSE {
            return Err(format!("PG 认证失败: {}", parse_error_fields(&body)));
        }
        if ty != MSG_AUTHENTICATION {
            return Err(format!("认证期间收到意外消息 type={}", ty as char));
        }
        let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        if code != AUTH_SASL_CONTINUE {
            return Err(format!("期望 SASLContinue(11)，收到 {}", code));
        }
        String::from_utf8_lossy(&body[4..]).into_owned()
    };
    let s_first = parse_scram_attrs(&server_first);
    let s_nonce = s_first.get("r").ok_or("server-first 缺 r")?;
    let salt_b64 = s_first.get("s").ok_or("server-first 缺 s")?;
    let iterations: u32 = s_first
        .get("i")
        .ok_or("server-first 缺 i")?
        .parse()
        .map_err(|_| "迭代次数非法")?;

    // 校验 server nonce 以 client nonce 开头（防降级）
    if !s_nonce.starts_with(&c_nonce) {
        return Err("server nonce 与 client nonce 不匹配（可能被降级攻击）".into());
    }

    let salt = base64_decode(salt_b64)?;
    if salt.len() < 16 {
        return Err(format!("SCRAM salt 过短: {} 字节", salt.len()));
    }

    // 5) 计算 SaltedPassword = PBKDF2-HMAC-SHA256(password, salt, i, 32)
    let mut salted = [0u8; 32];
    pbkdf2_hmac::<Sha256>(cfg.password.as_bytes(), &salt, iterations, &mut salted);
    let salted = salted;

    // 6) ClientKey / StoredKey
    let client_key = hmac_sha256(&salted, b"Client Key");
    let stored_key = sha256(&client_key);

    // 7) client-final-without-proof
    let cbind = "biws"; // base64("n,,") — 无通道绑定
    let client_final_without_proof = format!("c={},r={}", cbind, s_nonce);

    // 8) AuthMessage = client-first-bare + "," + server-first + "," + client-final-without-proof
    let auth_message = format!(
        "{},{},{}",
        client_first_bare, server_first, client_final_without_proof
    );

    // 9) ClientSignature = HMAC(StoredKey, AuthMessage)；ClientProof = ClientKey XOR Signature
    let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes());
    let proof: Vec<u8> = client_key
        .iter()
        .zip(client_signature.iter())
        .map(|(a, b)| a ^ b)
        .collect();

    // 10) 发送 SASLResponse（`p`）：client-final-message
    let client_final = format!("{},p={}", client_final_without_proof, base64_encode(&proof));
    send_message(conn, MSG_PASSWORD, &client_final.as_bytes().to_vec())?;

    // 11) 读 AuthenticationSASLFinal (12) → server-final-message，验证 v=
    let (ty, body) = read_message(conn)?;
    if ty == MSG_ERROR_RESPONSE {
        return Err(format!("PG 认证失败: {}", parse_error_fields(&body)));
    }
    if ty != MSG_AUTHENTICATION {
        return Err(format!("认证期间收到意外消息 type={}", ty as char));
    }
    let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    if code != AUTH_SASL_FINAL {
        return Err(format!("期望 SASLFinal(12)，收到 {}", code));
    }
    let server_final = String::from_utf8_lossy(&body[4..]).into_owned();
    if server_final.starts_with('e') {
        return Err(format!("PG SCRAM 拒绝: {}", server_final));
    }
    let s_final = parse_scram_attrs(&server_final);
    let server_sig = s_final.get("v").ok_or("server-final 缺 v")?;

    // 验证 ServerSignature = HMAC(ServerKey, AuthMessage)
    let server_key = hmac_sha256(&salted, b"Server Key");
    let server_signature = hmac_sha256(&server_key, auth_message.as_bytes());
    let expected = base64_encode(&server_signature);
    if expected != *server_sig {
        return Err("服务器签名校验失败（ServerSignature 不匹配）".into());
    }

    Ok(())
}

/// 执行握手：发送启动消息 → 处理认证 → 读到 ReadyForQuery。
fn handshake(conn: u32, cfg: &PgConfig) -> Result<(), String> {
    // 1) 启动消息
    host_send(conn, &startup_message(cfg)).map_err(|e| format!("send startup: {}", e))?;

    // 2) 认证循环
    loop {
        let (ty, body) = read_message(conn)?;
        match ty {
            MSG_AUTHENTICATION => {
                if body.len() < 4 {
                    return Err("认证消息过短".into());
                }
                let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                match code {
                    AUTH_OK => { /* 认证通过，继续 */ }
                    AUTH_CLEARTEXT => {
                        let mut pw = cfg.password.clone().into_bytes();
                        pw.push(0);
                        send_message(conn, MSG_PASSWORD, &pw)?;
                    }
                    AUTH_MD5 => {
                        // 需要 server 的 salt（后 4 字节）
                        if body.len() < 8 {
                            return Err("md5 认证缺 salt".into());
                        }
                        let salt = &body[4..8];
                        // md5(md5(password + username) + salt)
                        let inner = format!("{}{}", cfg.password, cfg.username);
                        let inner_hash = md5_hex(inner.as_bytes());
                        let mut combined = inner_hash.clone().into_bytes();
                        combined.extend_from_slice(salt);
                        let outer_hash = md5_hex(&combined);
                        let mut pw = format!("md5{}", outer_hash).into_bytes();
                        pw.push(0);
                        send_message(conn, MSG_PASSWORD, &pw)?;
                    }
                    AUTH_SASL => {
                        // SCRAM-SHA-256（RFC 5802）：在本次消息交互内完成 initial → continue → final
                        scram_authenticate(conn, cfg, &body)?;
                    }
                    other => {
                        return Err(format!("不支持的认证方式 {}", other));
                    }
                }
            }
            MSG_ERROR_RESPONSE => {
                let msg = parse_error_fields(&body);
                return Err(format!("PG 认证失败: {}", msg));
            }
            MSG_READY_FOR_QUERY => {
                // 认证完成，连接就绪
                return Ok(());
            }
            _ => { /* 忽略 ParameterStatus / BackendKeyData / Notice 等 */ }
        }
    }
}

fn parse_error_fields(body: &[u8]) -> String {
    // ErrorResponse body: 若干 字段类型字节 + 字符串 + 末尾 0
    let mut msg = String::new();
    let mut i = 0;
    while i + 1 < body.len() {
        let code = body[i];
        let end = body[i + 1..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| i + 1 + p)
            .unwrap_or(body.len());
        let field = String::from_utf8_lossy(&body[i + 1..end]).into_owned();
        if code == b'M' {
            msg = field;
        }
        if end >= body.len() {
            break;
        }
        i = end + 1;
    }
    msg
}

// ─── 简单查询 ─────────────────────────────────

fn send_query(conn: u32, sql: &str) -> Result<(), String> {
    let mut body = sql.as_bytes().to_vec();
    body.push(0);
    send_message(conn, MSG_QUERY, &body)
}

fn read_cstring(body: &[u8], i: &mut usize) -> String {
    let start = *i;
    while *i < body.len() && body[*i] != 0 {
        *i += 1;
    }
    let s = String::from_utf8_lossy(&body[start..*i]).into_owned();
    if *i < body.len() {
        *i += 1; // 跳过 null
    }
    s
}

/// 执行查询并返回 JSON 结果字符串
fn simple_query(conn: u32, sql: &str) -> Result<String, String> {
    send_query(conn, sql)?;

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut command_tag = String::new();

    loop {
        let (ty, body) = read_message(conn)?;
        match ty {
            MSG_ROW_DESCRIPTION => {
                // 字段数 + 每个字段 FieldDescription（前 3 个有用：name, table_oid, attr_num, type_oid, size, type_modifier, format）
                if body.is_empty() {
                    return Err("RowDescription 为空".into());
                }
                let field_count = u16::from_be_bytes([body[0], body[1]]);
                let mut i = 2;
                columns.clear();
                for _ in 0..field_count {
                    let name = read_cstring(&body, &mut i);
                    // 跳过 table_oid(4) attr_num(2) type_oid(4) size(2) type_modifier(4) format(2)
                    if i + 18 <= body.len() {
                        i += 18;
                    } else {
                        break;
                    }
                    columns.push(name);
                }
            }
            MSG_DATA_ROW => {
                if body.len() < 2 {
                    return Err("DataRow 过短".into());
                }
                let col_count = u16::from_be_bytes([body[0], body[1]]);
                let mut i = 2;
                let mut row = serde_json::Map::new();
                for c in 0..col_count {
                    if i + 4 > body.len() {
                        break;
                    }
                    let len = i32::from_be_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
                    i += 4;
                    let value: serde_json::Value = if len < 0 {
                        serde_json::Value::Null // NULL
                    } else {
                        let raw = &body[i..i + len as usize];
                        i += len as usize;
                        let s = String::from_utf8_lossy(raw);
                        // 尝试按 JSON 解析（PG 文本协议中数字/布尔原样）；失败按字符串
                        serde_json::from_str(s.trim())
                            .unwrap_or_else(|_| serde_json::Value::String(s.into_owned()))
                    };
                    let col_name = columns
                        .get(c as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("col{}", c));
                    row.insert(col_name, value);
                }
                rows.push(serde_json::Value::Object(row));
            }
            MSG_COMMAND_COMPLETE => {
                command_tag = read_cstring(&body, &mut 0);
            }
            MSG_EMPTY_QUERY => { /* 空查询 */ }
            MSG_ERROR_RESPONSE => {
                return Err(format!("PG 错误: {}", parse_error_fields(&body)));
            }
            MSG_READY_FOR_QUERY => {
                break; // 查询完成
            }
            _ => { /* 忽略 Notice 等 */ }
        }
    }

    // 组装 JSON 结果
    let result = serde_json::json!({
        "columns": columns,
        "rows": rows,
        "rowCount": rows.len(),
        "commandTag": command_tag,
    });
    Ok(serde_json::to_string(&result).unwrap_or_default())
}

// ─── 插件入口 ─────────────────────────────────

struct PgQueryPlugin;

impl exports::orbit::protocol_plugin::protocol::Guest for PgQueryPlugin {
    fn get_capabilities() -> Vec<Capability> {
        vec![Capability {
            protocol_id: "pg".to_string(),
            display_name: "PostgreSQL".to_string(),
            supports_streaming: false,
            description: "PostgreSQL 查询插件（PG wire 握手 + 认证 + 查询）。".to_string(),
        }]
    }

    fn execute(req: Request) -> Result<Response, String> {
        let cfg = parse_connection(req.connection.as_deref());
        // SQL：优先 payload（UTF-8），否则 options.sql / operation
        let sql = if !req.payload.is_empty() {
            String::from_utf8_lossy(&req.payload).into_owned()
        } else if let Some(opts) = req.options.as_deref() {
            serde_json::from_str::<serde_json::Value>(opts)
                .ok()
                .and_then(|v| v.get("sql").and_then(|x| x.as_str()).map(String::from))
                .unwrap_or_default()
        } else {
            req.operation.clone()
        };

        let target = format!("{}:{}", cfg.host, cfg.port);
        let conn = host_connect(&target, "{}").map_err(|e| format!("connect {}: {}", target, e))?;

        let result = (|| -> Result<String, String> {
            handshake(conn, &cfg)?;
            let json = simple_query(conn, sql.trim())?;
            Ok(json)
        })();

        // 发送 terminate 并关闭连接
        let _ = send_message(conn, MSG_TERMINATE, &[]);
        let _ = host_close(conn);

        let payload = result.map_err(|e| e)?.into_bytes();
        Ok(Response {
            status_code: 200,
            metadata: vec![("content-type".to_string(), "application/json".to_string())],
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
        Ok(())
    }
}

export!(PgQueryPlugin);
