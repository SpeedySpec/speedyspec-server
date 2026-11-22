//! `speedyspec-php-runtime`: the PHP runtime the server starts for every PHP app's workers.
//!
//! - `worker --spec <json>` serves FastCGI on the spec's socket until the server goes away.
//! - `manifest` prints the [`RuntimeManifest`] the server reads before using this binary.

mod database;
mod wake;
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
        #[arg(
            long,
            conflicts_with = "spec_stdin",
            required_unless_present = "spec_stdin"
        )]
        spec: Option<String>,
        /// Read the worker spec, as JSON, from standard input (how the server passes it: the app's credentials never
        /// show in the process list).
        #[arg(long)]
        spec_stdin: bool,
    },
    /// Print this runtime's manifest as JSON.
    Manifest,
}

/// The spec, from the argument or from standard input.
fn read_spec(spec: Option<String>, from_stdin: bool) -> Result<String, String> {
    match spec {
        Some(spec) if !from_stdin => Ok(spec),
        _ => {
            let mut spec = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut spec)
                .map_err(|err| err.to_string())?;
            Ok(spec)
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Worker { spec, spec_stdin } => match read_spec(spec, spec_stdin).and_then(|spec| {
            serde_json::from_str::<WorkerSpec>(&spec).map_err(|err| err.to_string())
        }) {
            Ok(spec) => ExitCode::from(u8::try_from(worker::run_worker(spec)).unwrap_or(1)),
            Err(err) => {
                eprintln!("speedyspec-php-runtime worker: --spec is not valid: {err}");
                ExitCode::from(2)
            }
        },
        Command::Manifest => {
            let sodium = match speedyspec_wp_crypto_sodium::Sodium::init() {
                Ok(sodium) => speedyspec_wp_crypto_contracts::CryptoLibrary::version(&sodium),
                Err(err) => {
                    eprintln!("speedyspec-php-runtime manifest: {err}; WordPress needs libsodium");
                    return ExitCode::from(1);
                }
            };
            let manifest = RuntimeManifest::new(
                env!("CARGO_PKG_VERSION")
                    .parse()
                    .expect("the package version is SemVer"),
                vec![CONTRACT_MAJOR],
                speedyspec_php::PHP_VERSION,
            )
            .with_sodium(sodium);
            println!(
                "{}",
                serde_json::to_string(&manifest).expect("a manifest serializes")
            );
            ExitCode::SUCCESS
        }
    }
}
