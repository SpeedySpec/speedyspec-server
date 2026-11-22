//! The probe child as its own binary, for tests and development; the runtime's `media-probe` subcommand runs the same.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    speedyspec_wp_media_isolate::child::run(&args)
}
