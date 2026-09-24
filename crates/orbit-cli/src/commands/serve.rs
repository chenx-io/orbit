//! `orbit serve` — start the web server

use std::path::PathBuf;

pub async fn serve_command(port: u16, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let config =
        orbit_config::ServerConfig::load_for_server(config_path).map_err(anyhow::Error::msg)?;
    orbit_server::run_server(port, config).await
}
