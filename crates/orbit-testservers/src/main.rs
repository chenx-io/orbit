//! Local protocol servers for Phase 1 acceptance testing.
//!
//! Starts HTTP / gRPC(reflection) / WebSocket / TCP / UDP / SSE / GraphQL servers in the same process
//! on fixed local ports, for hands-on runs of `orbit run examples/acceptance/*.yaml`.

mod grpc_srv;
mod http_srv;
mod tcp_srv;
mod udp_srv;
mod ws_srv;

use std::net::{Ipv4Addr, SocketAddr};

pub const HTTP_PORT: u16 = 18780;
pub const GRPC_PORT: u16 = 18781;
pub const WS_PORT: u16 = 18782;
pub const TCP_PORT: u16 = 18783;
pub const UDP_PORT: u16 = 18784;
pub const SSE_PORT: u16 = 18785;
pub const GRAPHQL_PORT: u16 = 18786;
pub const TCP_LP_PORT: u16 = 18787;
pub const WS_AUTH_PORT: u16 = 18788;

fn local(port: u16) -> SocketAddr {
    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // HTTP (including the /sse and /graphql routes)
    tokio::spawn(http_srv::serve(HTTP_PORT));
    tokio::spawn(http_srv::serve_sse(SSE_PORT));
    tokio::spawn(http_srv::serve_graphql(GRAPHQL_PORT));

    // gRPC (including Server Reflection)
    tokio::spawn(grpc_srv::serve(GRPC_PORT));

    // WebSocket / TCP / UDP
    tokio::spawn(ws_srv::serve(WS_PORT));
    tokio::spawn(ws_srv::serve_auth(WS_AUTH_PORT));
    tokio::spawn(tcp_srv::serve(TCP_PORT));
    tokio::spawn(tcp_srv::serve_length_prefix(TCP_LP_PORT));
    tokio::spawn(udp_srv::serve(UDP_PORT));

    // Wait for the ports to become ready
    for (name, port) in [
        ("http", HTTP_PORT),
        ("grpc", GRPC_PORT),
        ("ws", WS_PORT),
        ("tcp", TCP_PORT),
        ("udp", UDP_PORT),
        ("sse", SSE_PORT),
        ("graphql", GRAPHQL_PORT),
        ("tcp-lp", TCP_LP_PORT),
        ("ws-auth", WS_AUTH_PORT),
    ] {
        for _ in 0..50 {
            if std::net::TcpStream::connect(local(port)).is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        println!("READY {name} 127.0.0.1:{port}");
    }
    println!("ALL_READY");

    // Stay resident
    std::future::pending::<()>().await;
    Ok(())
}
