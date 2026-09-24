//! Raw UDP echo server: echoes each datagram back verbatim.

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket = tokio::net::UdpSocket::bind(format!("127.0.0.1:{port}")).await?;
    let mut buf = vec![0u8; 65536];
    loop {
        let (n, peer) = socket.recv_from(&mut buf).await?;
        let _ = socket.send_to(&buf[..n], peer).await;
    }
}
