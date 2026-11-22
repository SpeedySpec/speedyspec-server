//! Builds the vendored PCRE2 (the static PHP's own copy, 10.44) with every external symbol renamed.
//!
//! The static PHP links its own PCRE2 into the same binary. It renames only PCRE2's public API
//! (`php_pcre2_*`); the internals keep their stock names (`_pcre2_ucd_records_8`, ...). A second PCRE2 with
//! stock names would collide with them, and the linker would keep one copy of each, so one side would read
//! the other build's tables. `prefix.h`, force-included into every file, renames every symbol this build
//! defines to `speedyspec_*`; the bindings link to those names. Nothing here uses a system libpcre2: the
//! Rust patterns always run on the same PCRE2 version PHP's `preg_*` functions do.

use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap();
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let upstream = manifest.join("upstream");
    let prefix = manifest.join("prefix.h");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=prefix.h");
    println!("cargo:rerun-if-changed=upstream");

    let mut builder = cc::Build::new();
    builder
        .define("PCRE2_CODE_UNIT_WIDTH", "8")
        .define("HAVE_STDLIB_H", "1")
        .define("HAVE_MEMMOVE", "1")
        .define("HAVE_CONFIG_H", "1")
        .define("PCRE2_STATIC", "1")
        .define("STDC_HEADERS", "1")
        .define("SUPPORT_PCRE2_8", "1")
        .define("SUPPORT_UNICODE", "1");
    if target.contains("windows") {
        builder.define("HAVE_WINDOWS_H", "1");
    }
    if jit_supported(&target) {
        builder.define("SUPPORT_JIT", "1");
    }
    if builder.get_compiler().is_like_msvc() {
        builder.flag(format!("/FI{}", prefix.display()));
    } else {
        builder.flag("-include").flag(prefix.to_str().unwrap());
    }

    builder
        .include(upstream.join("src"))
        .include(upstream.join("include"));
    // The files PHP's build compiles. pcre2_jit_match.c and pcre2_jit_misc.c are included by
    // pcre2_jit_compile.c, pcre2_ucptables.c by pcre2_tables.c, and pcre2_printint.c is pcre2test's.
    for result in std::fs::read_dir(upstream.join("src")).unwrap() {
        let path = result.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "c") {
            continue;
        }
        if [
            "pcre2_jit_match.c",
            "pcre2_jit_misc.c",
            "pcre2_ucptables.c",
            "pcre2_printint.c",
        ]
        .iter()
        .any(|skip| path.ends_with(skip))
        {
            continue;
        }
        builder.file(path);
    }

    if std::env::var("PCRE2_SYS_DEBUG").is_ok_and(|v| v == "1")
        || std::env::var("DEBUG").is_ok_and(|v| v == "1")
    {
        builder.debug(true);
    }
    builder.compile("pcre2");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!(
        "cargo:rustc-env=SPEEDYSPEC_PCRE2_LIB={}",
        out.join("libpcre2.a").display()
    );
}

/// Whether sljit supports the target; the same exclusions as the upstream pcre2-sys.
fn jit_supported(target: &str) -> bool {
    !(target == "aarch64-linux-android"
        || target == "armv7-linux-androideabi"
        || target == "aarch64-unknown-linux-musl"
        || target.contains("musleabi")
        || target.contains("apple-ios")
        || target.contains("apple-tvos"))
}
