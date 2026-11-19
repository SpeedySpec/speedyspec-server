//! `speedyspec-php-runtime`: the PHP runtime the server starts for every PHP app's workers.
//!
//! - `worker --spec <json>` serves FastCGI on the spec's socket until the server goes away.
//! - `manifest` prints the [`RuntimeManifest`] the server reads before using this binary.

mod worker;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use speedyspec_runtime_contract::{CONTRACT_MAJOR, RuntimeManifest, WorkerSpec};

#[derive(Parser)]
#[command(name = "speedyspec-php-runtime", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve FastCGI requests as one of an app's workers.
    Worker {
        /// The worker spec, as JSON.
        #[arg(long)]
        spec: String,
    },
    /// Print this runtime's manifest as JSON.
    Manifest,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Worker { spec } => match serde_json::from_str::<WorkerSpec>(&spec) {
            Ok(spec) => ExitCode::from(u8::try_from(worker::run_worker(spec)).unwrap_or(1)),
            Err(err) => {
                eprintln!("speedyspec-php-runtime worker: --spec is not valid: {err}");
                ExitCode::from(2)
            }
        },
        Command::Manifest => {
            let manifest = RuntimeManifest {
                version: env!("CARGO_PKG_VERSION")
                    .parse()
                    .expect("the package version is SemVer"),
                contracts: vec![CONTRACT_MAJOR],
                php: speedyspec_php::PHP_VERSION.to_string(),
            };
            println!(
                "{}",
                serde_json::to_string(&manifest).expect("a manifest serializes")
            );
            ExitCode::SUCCESS
        }
    }
}
