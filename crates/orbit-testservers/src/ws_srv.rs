//! WebSocket echo server: echoes text/binary frames verbatim.

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let ws = tokio_tungstenite::accept_async(stream).await;
            let Ok(mut ws) = ws else { return };
            while let Some(Ok(msg)) = ws.next().await {
                match msg {
                    Message::Text(t) => {
                        if ws.send(Message::Text(t)).await.is_err() {
                            break;
                        }
                    }
                    Message::Binary(b) => {
                        if ws.send(Message::Binary(b)).await.is_err() {
                            break;
                        }
                    }
                    Message::Ping(p) => {
                        if ws.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
    }
}

/// Game project case: the WebSocket server requires a token in the URL query (issued by the login endpoint).
/// On validation failure the handshake returns 401 directly, so clients can assert auth failure.
pub async fn serve_auth(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            #[allow(clippy::result_large_err)]
            let upgrade =
                tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp: Response| {
                    let token = req
                        .uri()
                        .query()
                        .and_then(|q| {
                            q.split('&').find_map(|p| {
                                let (k, v) = p.split_once('=')?;
                                (k == "token").then_some(v.to_string())
                            })
                        })
                        .unwrap_or_default();
                    if token == "tkn-abc123" {
                        Ok(resp)
                    } else {
                        Ok(Response::builder()
                            .status(http::StatusCode::UNAUTHORIZED)
                            .body(())
                            .unwrap_or(resp))
                    }
                })
                .await;
            let Ok(mut ws) = upgrade else { return };
            while let Some(Ok(msg)) = ws.next().await {
                match msg {
                    Message::Text(t) => {
                        if ws.send(Message::Text(t)).await.is_err() {
                            break;
                        }
                    }
                    Message::Binary(b) => {
                        if ws.send(Message::Binary(b)).await.is_err() {
                            break;
                        }
                    }
                    Message::Ping(p) => {
                        if ws.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
    }
}
