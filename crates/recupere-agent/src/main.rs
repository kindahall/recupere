// recupere-agent — headless remote recovery agent.
//
// V1 surface:
//   - `init`  : generate a fresh bearer token, print it once, store its hash
//   - `serve` : run the HTTP API
//
// HTTP endpoints (all require `Authorization: Bearer <token>`):
//   GET  /v1/health         → liveness probe (no auth)
//   GET  /v1/devices        → engine device enumeration
//   POST /v1/scans          → 501 (not implemented in V1 baseline)
//   GET  /v1/scans/:id/...  → 501
//
// Default bind is 127.0.0.1:7878 — meant to be exposed via `ssh -L`.

mod auth;
mod http;

use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "recupere-agent", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to the agent state directory (token hash, runtime files).
    #[arg(long, global = true, default_value = "/etc/recupere-agent")]
    state_dir: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate a new bearer token, persist its hash, print the plaintext once.
    Init,
    /// Run the HTTP API.
    Serve {
        /// Bind address. Defaults to 127.0.0.1:7878 — pair with `ssh -L`.
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: SocketAddr,
    },
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("recupere_agent=info,recupere_lib=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .try_init();
}

fn main() -> std::process::ExitCode {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Init => match auth::init_token(&cli.state_dir) {
            Ok(token) => {
                println!("recupere-agent: new bearer token generated.");
                println!();
                println!("  {}", token);
                println!();
                println!("Store this value now — it will not be shown again.");
                println!("State directory: {}", cli.state_dir.display());
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("recupere-agent: init failed: {error}");
                std::process::ExitCode::FAILURE
            }
        },
        Command::Serve { bind } => {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    eprintln!("recupere-agent: failed to start tokio runtime: {error}");
                    return std::process::ExitCode::FAILURE;
                }
            };

            match runtime.block_on(http::serve(bind, cli.state_dir)) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("recupere-agent: server error: {error}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
    }
}
