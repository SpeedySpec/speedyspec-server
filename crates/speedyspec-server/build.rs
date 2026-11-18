//! Exports the Rust target triple, so the server installs its own platform's binaries.

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=SPEEDYSPEC_TARGET={target}");
}
