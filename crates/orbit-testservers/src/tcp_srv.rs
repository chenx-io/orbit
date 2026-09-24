//! Raw TCP echo server: echoes the received data verbatim, then closes the connection.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            match sock.read(&mut buf).await {
                Ok(0) => {}
                Ok(n) => {
                    let _ = sock.write_all(&buf[..n]).await;
                    let _ = sock.shutdown().await;
                }
                Err(_) => {}
            }
        });
    }
}

/// Length-prefixed frame echo server: 4-byte big-endian length + frame body, echoes the same frame, connection stays open for multiple frames
pub async fn serve_length_prefix(
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    loop {
        let (mut sock, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut header = [0u8; 4];
            loop {
                let mut got = 0usize;
                while got < 4 {
                    match sock.read(&mut header[got..]).await {
                        Ok(0) => return,
                        Ok(n) => got += n,
                        Err(_) => return,
                    }
                }
                let len = u32::from_be_bytes(header) as usize;
                let mut body = vec![0u8; len];
                let mut got = 0usize;
                while got < len {
                    match sock.read(&mut body[got..]).await {
                        Ok(0) => return,
                        Ok(n) => got += n,
                        Err(_) => return,
                    }
                }
                let mut frame = header.to_vec();
                frame.extend_from_slice(&body);
                if sock.write_all(&frame).await.is_err() {
                    return;
                }
            }
        });
    }
}
