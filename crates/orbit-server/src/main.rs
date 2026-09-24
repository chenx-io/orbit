//! orbit-server binary entry point.
//!
//! Usage:
//!   orbit-server                       # listen on 8788, config from the run dir orbit-server.toml -> ~/.orbit/config.toml
//!   orbit-server --port 9000
//!   orbit-server --config /etc/orbit/server.toml
//!
//! Environment variables: ORBIT_SERVER_PORT / ORBIT_EVENT_SINK

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "orbit-server", version, about = "Orbit API testing server")]
struct Cli {
    /// Listen port (overridable via ORBIT_SERVER_PORT)
    #[arg(short, long, env = "ORBIT_SERVER_PORT", default_value_t = 8788)]
    port: u16,
    /// Config file path (default: run dir orbit-server.toml -> ~/.orbit/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config =
        orbit_config::ServerConfig::load_for_server(cli.config).map_err(anyhow::Error::msg)?;
    orbit_server::run_server(cli.port, config).await
}
