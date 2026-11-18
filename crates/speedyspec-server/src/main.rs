//! The `speedyspec-server` binary.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use speedyspec_server::boot::{bind, prepare, server_manifest};
use speedyspec_server::config::{Config, DEFAULT_CONFIG, Mode};
use speedyspec_server::listeners::serve;
use speedyspec_server_domain::AppId;
use speedyspec_server_presentation::build_router;

#[derive(Parser)]
#[command(
    name = "speedyspec-server",
    version,
    about = "The SpeedySpec web server.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve, using the given configuration.
    Run {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Print this binary's release manifest as JSON.
    Manifest,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Command::Manifest => {
            let manifest = server_manifest(AppId::parse("server").expect("valid id"));
            println!(
                "{}",
                serde_json::to_string_pretty(&manifest).expect("manifests serialize")
            );
            ExitCode::SUCCESS
        }
        Command::Run { config } => match run(config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                for line in message.lines() {
                    eprintln!("speedyspec-server: {line}");
                }
                ExitCode::FAILURE
            }
        },
    }
}

fn run(path: PathBuf) -> Result<(), String> {
    let config = Config::load(&path).map_err(|err| err.to_string())?;
    if config.mode == Mode::Managed {
        return Err(
            "managed mode is not available in this build yet; use mode = \"immutable\"".to_string(),
        );
    }
    let state = prepare(&config).map_err(|err| err.to_string())?;
    let runtime =
        tokio::runtime::Runtime::new().map_err(|err| format!("starting the runtime: {err}"))?;

    runtime.block_on(async {
        let (tcp, udp) = bind(&config).map_err(|err| err.to_string())?;
        let router = Arc::new(build_router(Arc::new(state)));
        serve(tcp, udp, router, shutdown())
            .await
            .map_err(|err| format!("{err:#}"))
    })
}

/// Resolves on `SIGTERM` or `SIGINT`.
async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing SIGTERM");
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    tracing::info!(event = "shutting_down");
}
