//! UDP protocol client - based on tokio::net::UdpSocket

use async_trait::async_trait;
use std::time::Instant;
use tokio::net::UdpSocket;

use crate::traits::ProtocolClient;
use crate::types::{ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings};

pub struct UdpClient;

impl UdpClient {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UdpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for UdpClient {
    fn name(&self) -> &str {
        "udp"
    }
    fn description(&self) -> &str {
        "Raw UDP client"
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();

        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| ProtocolError::Connect(format!("UDP bind: {}", e)))?;
        socket
            .connect(&request.target)
            .await
            .map_err(|e| ProtocolError::Connect(format!("UDP connect: {}", e)))?;

        let send_start = Instant::now();
        socket
            .send(&request.payload)
            .await
            .map_err(|e| ProtocolError::Send(format!("UDP send: {}", e)))?;
        let send_duration = send_start.elapsed();

        let receive_start = Instant::now();
        let mut buf = vec![0u8; 65536];
        let n = socket
            .recv(&mut buf)
            .await
            .map_err(|e| ProtocolError::Receive(format!("UDP recv: {}", e)))?;
        buf.truncate(n);
        let receive_duration = receive_start.elapsed();

        Ok(ProtocolResponse {
            status_code: 0,
            metadata: vec![("protocol".into(), "udp".into())],
            payload: buf,
            message_count: 0,
            timings: ProtocolTimings {
                dns: None, // UDP is connectionless
                tcp: None,
                tls: None,
                send: Some(send_duration),
                first_byte: None,
                receive: Some(receive_duration),
                total: total_start.elapsed(),
            },
        })
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        Box::new(Self::new())
    }
}
