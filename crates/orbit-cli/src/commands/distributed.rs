//! `orbit distributed` — distributed load testing: Controller + Agent (gRPC)

use std::net::SocketAddr;

use orbit_distributed::{AgentConfig, AgentCore, AgentServer};

pub async fn distributed_serve(addr: String) -> anyhow::Result<()> {
    let addr: SocketAddr = addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid listen address '{}': {}", addr, e))?;

    let ctrl = std::sync::Arc::new(orbit_distributed::Controller::new());
    println!("🎛️  Controller started, listening on {}", addr);
    println!("   Agent report address: http://{}", addr);
    println!("   Press Ctrl-C to stop.");
    ctrl.serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Controller exited with an error: {}", e))?;
    Ok(())
}

pub async fn distributed_agent_start(
    mode: String,
    addr: Option<String>,
    controller: Option<String>,
    id: Option<String>,
    labels: Option<String>,
) -> anyhow::Result<()> {
    let agent_id = id.unwrap_or_else(|| {
        std::env::var("HOSTNAME").unwrap_or_else(|_| format!("agent-{}", std::process::id()))
    });
    let labels: Vec<(String, String)> = labels
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            Some((
                it.next()?.trim().to_string(),
                it.next().unwrap_or("").trim().to_string(),
            ))
        })
        .collect();
    let cfg = AgentConfig {
        id: agent_id.clone(),
        mode: mode.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        memory_mb: sysinfo_memory_mb(),
        labels,
    };

    match mode.as_str() {
        "server" => {
            let bind = addr.ok_or_else(|| anyhow::anyhow!("server mode requires --addr"))?;
            let socket: SocketAddr = bind
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid listen address '{}': {}", bind, e))?;
            println!(
                "🤖 Agent [{}] started in server mode, listening on {}",
                agent_id, socket
            );
            println!("   Press Ctrl-C to stop.");
            let core = AgentCore::new(cfg);
            AgentServer::new(core)
                .serve(socket)
                .await
                .map_err(|e| anyhow::anyhow!("Agent server exited: {}", e))?;
        }
        "client" => {
            let ctrl =
                controller.ok_or_else(|| anyhow::anyhow!("client mode requires --controller"))?;
            println!(
                "🤖 Agent [{}] started in client mode, connecting to controller {}",
                agent_id, ctrl
            );
            let core = AgentCore::new(cfg);
            orbit_distributed::run_client(ctrl, core)
                .await
                .map_err(|e| anyhow::anyhow!("Agent client exited: {}", e))?;
        }
        other => {
            return Err(anyhow::anyhow!(
                "unknown mode: {other} (supported: server|client)"
            ))
        }
    }
    Ok(())
}

fn sysinfo_memory_mb() -> u64 {
    use sysinfo::System;
    System::new().total_memory() / (1024 * 1024)
}
