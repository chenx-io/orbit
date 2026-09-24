//! Orbit CLI — command-line entry point of the API testing workbench
//!
//! Command implementations are split per module under `commands/`; this file only holds the clap definitions and dispatch.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod commands;
mod util;

#[derive(Parser)]
#[command(name = "orbit")]
#[command(version)] // Follows the Cargo.toml version (maintained centrally by workspace.package)
#[command(about = "Orbit — All-in-one API testing toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a test plan
    Run {
        /// Path to the test plan YAML file
        file: PathBuf,

        /// Environment to use (e.g. staging, prod)
        #[arg(short, long)]
        env: Option<String>,

        /// Override VU count
        #[arg(long)]
        vus: Option<u32>,

        /// Override duration
        #[arg(long)]
        duration: Option<String>,

        /// Output format (text, json, csv, html, junit, jtl, raw-json)
        #[arg(short, long, default_value = "text")]
        output: String,

        /// Directory to write report files (html/junit/jtl/raw-json)
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },

    /// Validate a test plan YAML file
    Validate {
        /// Path to the test plan YAML file
        file: PathBuf,
    },

    /// Import from other tools (curl, postman, openapi, swagger, har, k6, jmeter)
    Import {
        /// Input file (or '-' for stdin)
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,

        /// Source format (required: curl, postman, openapi, har, k6, jmeter)
        #[arg(short = 'F', long)]
        from: String,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Start the web server
    Serve {
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Config file path (default: orbit-server.toml in the working directory -> ~/.orbit/config.toml)
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Start a mock server from a test plan
    Mock {
        file: PathBuf,
        #[arg(short, long, default_value = "3001")]
        port: u16,
    },

    /// Show request history (reads the app data snapshot)
    History {
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Export test plan to curl/wget/fetch/postman/openapi/swagger etc.
    Export {
        file: PathBuf,
        #[arg(short, long, default_value = "curl")]
        format: String,
    },

    /// Compare two load test result files
    Compare {
        baseline: PathBuf,
        current: PathBuf,
        #[arg(long, default_value = "10")]
        max_regression: f64,
        /// Enable statistical regression check (MAD z-score)
        #[arg(long)]
        regression: bool,
    },

    /// Distributed load testing: Controller + Agents over gRPC
    Distributed {
        #[command(subcommand)]
        cmd: DistributedCmd,
    },
}

#[derive(Subcommand)]
enum DistributedCmd {
    /// Start a Controller gRPC server that merges agent metrics
    Serve {
        /// Address to bind the Controller gRPC server (e.g. 0.0.0.0:50051)
        #[arg(short, long, default_value = "0.0.0.0:50051")]
        addr: String,
    },
    /// Start an Agent (two modes: server = the agent listens and the controller connects; client = the agent connects to the controller)
    Agent {
        /// Run mode: server | client
        #[arg(long, default_value = "server")]
        mode: String,
        /// Listen address in server mode (e.g. 0.0.0.0:9091)
        #[arg(long)]
        addr: Option<String>,
        /// Controller gRPC address in client mode (must include the scheme, e.g. http://127.0.0.1:50051)
        #[arg(long)]
        controller: Option<String>,
        /// Agent ID (auto-generated when omitted)
        #[arg(long)]
        id: Option<String>,
        /// Labels, comma-separated k=v (e.g. zone=a,group=b)
        #[arg(long)]
        labels: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr so stdout carries command output only (--output json can then be redirected safely)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orbit=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            file,
            env,
            vus,
            duration,
            output,
            out_dir,
        } => {
            commands::run::run_test(file, env, vus, duration, output, out_dir).await?;
        }
        Commands::Validate { file } => {
            commands::validate::validate_plan(file)?;
        }
        Commands::Import { file, from, output } => {
            commands::import::import_command(file, from, output)?;
        }
        Commands::Serve { port, config } => {
            commands::serve::serve_command(port, config).await?;
        }
        Commands::Mock { file, port } => {
            commands::mock::mock_command(file, port).await?;
        }
        Commands::History { limit } => {
            commands::history::history_command(limit)?;
        }
        Commands::Export { file, format } => {
            commands::export::export_command(file, format)?;
        }
        Commands::Compare {
            baseline,
            current,
            max_regression,
            regression,
        } => {
            if regression {
                commands::compare::regression_check(baseline, current, max_regression)?;
            } else {
                commands::compare::compare_command(baseline, current, max_regression)?;
            }
        }
        Commands::Distributed { cmd } => match cmd {
            DistributedCmd::Serve { addr } => {
                commands::distributed::distributed_serve(addr).await?
            }
            DistributedCmd::Agent {
                mode,
                addr,
                controller,
                id,
                labels,
            } => {
                commands::distributed::distributed_agent_start(mode, addr, controller, id, labels)
                    .await?;
            }
        },
    }

    Ok(())
}
