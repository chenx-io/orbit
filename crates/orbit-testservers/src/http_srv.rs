//! HTTP server: JSON/text, an echo endpoint per data format (the server decodes to verify the request encoding), SSE, GraphQL.

use axum::{
    body::Bytes,
    extract::{Path, Query},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Write;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

async fn json_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "hello": "world",
        "n": 42,
        "flag": true,
        "arr": [1, 2, 3],
        "nested": { "a": 1 }
    }))
}

async fn text_handler() -> &'static str {
    "hello plain text"
}

/// Respond with a specific status code (no body)
async fn status_handler(Path(code): Path<u16>) -> (StatusCode, &'static str) {
    (
        StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        "",
    )
}

/// 302 redirect to /json
async fn redirect_handler() -> impl axum::response::IntoResponse {
    (StatusCode::FOUND, [("Location", "/json")], "redirecting")
}

/// gzip-compressed JSON response
async fn gzip_handler() -> impl axum::response::IntoResponse {
    let body = serde_json::to_vec(&serde_json::json!({"compressed": "gzip", "n": 1})).unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&body).unwrap();
    (
        [(axum::http::header::CONTENT_ENCODING, "gzip")],
        enc.finish().unwrap(),
    )
}

/// brotli-compressed JSON response
async fn brotli_handler() -> impl axum::response::IntoResponse {
    let body = serde_json::to_vec(&serde_json::json!({"compressed": "brotli", "n": 2})).unwrap();
    let mut enc = brotli::CompressorWriter::new(Vec::new(), 4096, 5, 22);
    enc.write_all(&body).unwrap();
    (
        [(axum::http::header::CONTENT_ENCODING, "br")],
        enc.into_inner(),
    )
}

/// zstd-compressed JSON response
async fn zstd_handler() -> impl axum::response::IntoResponse {
    let body = serde_json::to_vec(&serde_json::json!({"compressed": "zstd", "n": 3})).unwrap();
    (
        [(axum::http::header::CONTENT_ENCODING, "zstd")],
        zstd::encode_all(std::io::Cursor::new(body), 3).unwrap(),
    )
}

/// Slow response (?ms=500)
async fn slow_handler(Query(q): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let ms = q
        .get("ms")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(500);
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Json(serde_json::json!({ "slow": true, "ms": ms }))
}

/// Set a Cookie and return JSON
async fn setcookie_handler() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::SET_COOKIE,
            "session=abc123; Path=/; HttpOnly",
        )],
        Json(serde_json::json!({ "cookie_set": true })),
    )
}

/// HTML page (for css_selector assertions)
async fn html_handler() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html")],
        "<html><body><div class=\"card\"><span id=\"title\">Hello HTML</span></div></body></html>",
    )
}

/// XML response (for xpath assertions)
async fn xml_handler() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/xml")],
        "<root><item id=\"1\">orbit</item></root>",
    )
}

/// Large JSON response (for size_lt assertions)
async fn big_handler() -> Json<serde_json::Value> {
    let arr: Vec<serde_json::Value> = (0..500)
        .map(|i| serde_json::json!({ "i": i, "s": "padding-padding-padding" }))
        .collect();
    Json(serde_json::json!({ "items": arr }))
}

/// Echo the request headers as JSON
async fn echo_headers_handler(headers: axum::http::HeaderMap) -> Json<serde_json::Value> {
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        map.insert(
            k.as_str().to_string(),
            serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
        );
    }
    Json(serde_json::json!({ "headers": map }))
}

/// JSON echo: the request body must be valid JSON; it is decoded and echoed back as JSON
async fn echo_json(body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(v) => Ok(Json(serde_json::json!({ "echo": v }))),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// YAML echo: the request body must be valid YAML; it is decoded and echoed back as JSON
async fn echo_yaml(body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    match serde_yaml::from_slice::<serde_yaml::Value>(&body) {
        Ok(v) => {
            let json = serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
            Ok(Json(serde_json::json!({ "echo": json })))
        }
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// MsgPack echo: the request body must be valid msgpack; it is decoded and echoed back as JSON
async fn echo_msgpack(body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    match rmp_serde::from_slice::<serde_json::Value>(&body) {
        Ok(v) => Ok(Json(serde_json::json!({ "decoded": v }))),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// XML echo: echoes the body verbatim
async fn echo_xml(body: Bytes) -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/xml")],
        body,
    )
}

/// form-urlencoded echo: echoes the body verbatim
async fn echo_form(body: Bytes) -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )],
        body,
    )
}

/// protobuf echo: echoes the binary body verbatim
async fn echo_proto(body: Bytes) -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/protobuf")],
        body,
    )
}

/// SSE: pushes 3 events in a row
async fn sse_handler() -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(8);
    tokio::spawn(async move {
        for i in 0..3u32 {
            let mut event = Event::default()
                .id(format!("evt-{i}"))
                .event("message")
                .data(format!("data-{i}"));
            if i == 0 {
                // The first event carries a retry field (verifies full SSE format rendering)
                event = event.retry(std::time::Duration::from_millis(3000));
            }
            if tx.send(Ok(event)).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    Sse::new(ReceiverStream::new(rx))
}

/// GraphQL: parses { query } and returns a fixed schema
async fn graphql_handler(body: Bytes) -> Json<serde_json::Value> {
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    let query = req.get("query").and_then(|q| q.as_str()).unwrap_or("");
    let mut data = serde_json::Map::new();
    if query.contains("hello") {
        data.insert("hello".into(), serde_json::json!("world"));
    }
    if query.contains("country") {
        let vars = req
            .get("variables")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let code = vars
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or("CN")
            .to_uppercase();
        let name = if code == "CN" { "China" } else { code.as_str() };
        data.insert(
            "country".into(),
            serde_json::json!({ "name": name, "capital": "Beijing" }),
        );
    }
    Json(serde_json::json!({ "data": data }))
}

/// Game project case: HTTP login -> returns a token, used as the identity source for subsequent WebSocket connections
async fn login_handler(body: Bytes) -> (StatusCode, Json<serde_json::Value>) {
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    let username = req
        .get("username")
        .and_then(|u| u.as_str())
        .unwrap_or("player");
    if username.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "username required" })),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": "tkn-abc123",
            "expires_in": 3600,
            "user": username,
        })),
    )
}

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/json", get(json_handler))
        .route("/text", get(text_handler))
        .route("/status/{code}", get(status_handler))
        .route("/redirect", get(redirect_handler))
        .route("/gzip", get(gzip_handler))
        .route("/brotli", get(brotli_handler))
        .route("/zstd", get(zstd_handler))
        .route("/slow", get(slow_handler))
        .route("/setcookie", get(setcookie_handler))
        .route("/html", get(html_handler))
        .route("/xml", get(xml_handler))
        .route("/big", get(big_handler))
        .route("/echo/headers", post(echo_headers_handler))
        .route("/echo/json", post(echo_json))
        .route("/echo/yaml", post(echo_yaml))
        .route("/echo/msgpack", post(echo_msgpack))
        .route("/echo/xml", post(echo_xml))
        .route("/echo/form", post(echo_form))
        .route("/echo/proto", post(echo_proto));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn serve_sse(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new().route("/sse", get(sse_handler));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn serve_graphql(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new().route("/graphql", post(graphql_handler));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
