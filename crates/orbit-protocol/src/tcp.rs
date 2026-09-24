//! TCP protocol client - based on tokio::net::TcpStream
//!
//! Supports single-message round trips and long-lived sessions (connect → per-message send_recv → disconnect),
//! Framing: read_until_close / delimiter / fixed / length_prefix.

use async_trait::async_trait;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::traits::ProtocolClient;
use crate::types::{
    FramingMode, ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings,
};

/// TCP protocol client
pub struct TcpClient {
    stream: Option<TcpStream>,
}

impl TcpClient {
    pub fn new() -> Self {
        Self { stream: None }
    }
}

impl Default for TcpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Read one response frame using the framing config (reused by the session layer for live reads on long connections)
pub async fn read_tcp_frame<S>(
    stream: &mut S,
    framing: Option<crate::types::FramingOptions>,
) -> Result<Vec<u8>, ProtocolError>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    match framing.as_ref().map(|f| f.mode) {
        Some(FramingMode::ReadUntilClose) => {
            let mut tmp = [0u8; 4096];
            loop {
                let n = stream
                    .read(&mut tmp)
                    .await
                    .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
        }
        Some(FramingMode::Fixed) => {
            let len = framing.as_ref().and_then(|f| f.fixed_len).unwrap_or(0) as usize;
            let mut tmp = vec![0u8; len];
            let mut read_total = 0usize;
            while read_total < len {
                let n = stream
                    .read(&mut tmp[read_total..])
                    .await
                    .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
                if n == 0 {
                    break;
                }
                read_total += n;
            }
            buf = tmp[..read_total].to_vec();
        }
        Some(FramingMode::LengthPrefix) => {
            let len_bytes = framing.as_ref().and_then(|f| f.fixed_len).unwrap_or(4) as usize;
            let big_endian = framing.as_ref().map(|f| f.big_endian).unwrap_or(true);
            let mut header = vec![0u8; len_bytes];
            let mut got = 0usize;
            while got < len_bytes {
                let n = stream
                    .read(&mut header[got..])
                    .await
                    .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            let mut msg_len = 0usize;
            if big_endian {
                for b in &header {
                    msg_len = (msg_len << 8) | *b as usize;
                }
            } else {
                for b in header.iter().rev() {
                    msg_len = (msg_len << 8) | *b as usize;
                }
            }
            let mut tmp = vec![0u8; msg_len];
            let mut read_total = 0usize;
            while read_total < msg_len {
                let n = stream
                    .read(&mut tmp[read_total..])
                    .await
                    .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
                if n == 0 {
                    break;
                }
                read_total += n;
            }
            buf = tmp[..read_total].to_vec();
        }
        Some(FramingMode::Delimiter) => {
            let delim = framing
                .as_ref()
                .and_then(|f| f.delimiter.clone())
                .unwrap_or_else(|| "\n".to_string())
                .into_bytes();
            let mut tmp = [0u8; 4096];
            loop {
                let n = stream
                    .read(&mut tmp)
                    .await
                    .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(delim.len()).any(|w| w == delim) {
                    break;
                }
            }
        }
        _ => {
            // No framing config: read once (preserves current behavior)
            let mut tmp = vec![0u8; 65536];
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| ProtocolError::Receive(format!("TCP recv: {}", e)))?;
            tmp.truncate(n);
            buf = tmp;
        }
    }
    Ok(buf)
}

#[async_trait]
impl ProtocolClient for TcpClient {
    fn name(&self) -> &str {
        "tcp"
    }
    fn description(&self) -> &str {
        "Raw TCP client (session + framing)"
    }

    async fn connect(&mut self, target: &str) -> Result<(), ProtocolError> {
        let stream = TcpStream::connect(target)
            .await
            .map_err(|e| ProtocolError::Connect(format!("TCP connect: {}", e)))?;
        stream.set_nodelay(true).ok();
        self.stream = Some(stream);
        Ok(())
    }

    async fn send_recv(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| ProtocolError::Connect("TCP not connected".into()))?;

        let send_start = Instant::now();
        stream
            .write_all(&request.payload)
            .await
            .map_err(|e| ProtocolError::Send(format!("TCP send: {}", e)))?;
        let send_duration = send_start.elapsed();

        let receive_start = Instant::now();
        let buf = read_tcp_frame(stream, request.options.framing.clone()).await?;
        let receive_duration = receive_start.elapsed();

        Ok(ProtocolResponse {
            status_code: 0,
            metadata: vec![("protocol".into(), "tcp".into())],
            payload: buf,
            message_count: 0,
            timings: ProtocolTimings {
                dns: None,
                tcp: None,
                tls: None,
                send: Some(send_duration),
                first_byte: None,
                receive: Some(receive_duration),
                total: total_start.elapsed(),
            },
        })
    }

    async fn disconnect(&mut self) -> Result<(), ProtocolError> {
        if let Some(mut s) = self.stream.take() {
            let _ = s.shutdown().await;
        }
        Ok(())
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();
        let connect_start = Instant::now();
        self.connect(&request.target).await?;
        let connect_duration = connect_start.elapsed();

        let mut resp = self.send_recv(request).await?;
        resp.timings.tcp = Some(connect_duration);
        resp.timings.total = total_start.elapsed();

        let _ = self.disconnect().await;
        Ok(resp)
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        Box::new(Self::new())
    }
}
