//! The `speedyspec-server` binary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use semver::Version;
use speedyspec_server::boot::server_manifest;
use speedyspec_server::config::{Config, DEFAULT_CONFIG};
use speedyspec_server::control::{self, ControlRequest, ControlResponse};
use speedyspec_server_domain::AppId;

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
    /// Run one generation (started only by a keeper, with its channel on fd 3).
    #[command(hide = true)]
    Serve,
    /// List, check, install, switch, roll back and pin releases on the running server.
    Release {
        #[command(subcommand)]
        action: ReleaseAction,
    },
    /// Hand off to a new generation that re-reads the configuration (listener changes need a restart).
    Reload {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Show the server's version and mode, and every app's releases.
    Status {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
}

#[derive(Subcommand)]
enum ReleaseAction {
    /// Every app's active, kept, pinned and failed releases.
    List {
        app: Option<AppId>,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// The newest release each app could install.
    Check {
        app: Option<AppId>,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Download, verify and keep a release, without switching to it.
    Install {
        app: AppId,
        version: Option<Version>,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Switch one or several apps to kept releases, as one step.
    Switch {
        #[arg(required = true, value_parser = parse_target)]
        targets: Vec<(AppId, Version)>,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Switch back to the previous release, or to the given one.
    Rollback {
        app: AppId,
        version: Option<Version>,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Stop auto-update for an app at a kept release.
    Pin {
        app: AppId,
        version: Version,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Let auto-update run for an app again.
    Unpin {
        app: AppId,
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
}

/// `<app>@<version>`.
fn parse_target(value: &str) -> Result<(AppId, Version), String> {
    let (app, version) = value
        .split_once('@')
        .ok_or_else(|| format!("{value:?} is not <app>@<version>"))?;
    let app = AppId::parse(app).map_err(|err| err.to_string())?;
    let version =
        Version::parse(version).map_err(|err| format!("{version:?} is not a version: {err}"))?;
    Ok((app, version))
}

/// Sends one control request to the server the config at `config` describes, and prints the answer.
fn control_command(config: &PathBuf, request: ControlRequest) -> ExitCode {
    let result = Config::load(config)
        .map_err(|err| err.to_string())
        .and_then(|config| control::request(&control::socket_path(&config.run_dir), &request));
    match result {
        Ok(ControlResponse::Ok(value)) => {
            print!("{}", control::format(&request, &value));
            ExitCode::SUCCESS
        }
        Ok(ControlResponse::Error(message)) | Err(message) => {
            for line in message.lines() {
                eprintln!("speedyspec-server: {line}");
            }
            ExitCode::FAILURE
        }
    }
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
        Command::Status { config } => control_command(&config, ControlRequest::Status),
        Command::Reload { config } => control_command(&config, ControlRequest::Reload),
        Command::Release { action } => match action {
            ReleaseAction::List { app, config } => {
                control_command(&config, ControlRequest::List { app })
            }
            ReleaseAction::Check { app, config } => {
                control_command(&config, ControlRequest::Check { app })
            }
            ReleaseAction::Install {
                app,
                version,
                config,
            } => control_command(&config, ControlRequest::Install { app, version }),
            ReleaseAction::Switch { targets, config } => {
                control_command(&config, ControlRequest::Switch { targets })
            }
            ReleaseAction::Rollback {
                app,
                version,
                config,
            } => control_command(&config, ControlRequest::Rollback { app, version }),
            ReleaseAction::Pin {
                app,
                version,
                config,
            } => control_command(&config, ControlRequest::Pin { app, version }),
            ReleaseAction::Unpin { app, config } => {
                control_command(&config, ControlRequest::Unpin { app })
            }
        },
        Command::Serve => match speedyspec_server::generation::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("speedyspec-server: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Run { config } => match speedyspec_server::keeper::run(&config) {
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
