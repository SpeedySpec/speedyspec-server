//! Links the static PHP built by `scripts/build-php.sh` into this crate.
//!
//! The PHP install prefix (the build's `buildroot`) comes from `SPEEDYSPEC_PHP_PREFIX`, which is required: a
//! published crate has no repository around it to guess from. ext-php-rs's own build script, which runs first, finds
//! PHP through `PHP` and `PHP_CONFIG`; this script refuses to build if they point anywhere but the prefix, so headers,
//! bindings and library always agree. The PHP version it must be is pinned in this crate's own `php-version` file,
//! which ships in the package. On docs.rs (`DOCS_RS`) nothing is linked.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    println!("cargo:rerun-if-env-changed=SPEEDYSPEC_PHP_PREFIX");
    println!("cargo:rerun-if-env-changed=PHP");
    println!("cargo:rerun-if-env-changed=PHP_CONFIG");
    println!("cargo:rerun-if-env-changed=SODIUM_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    let version_file = manifest.join("php-version");
    println!("cargo:rerun-if-changed={}", version_file.display());
    let pinned = fs::read_to_string(&version_file)
        .unwrap_or_else(|e| fail(&format!("reading {}: {e}", version_file.display())));
    let pinned = pinned.trim();
    println!("cargo:rustc-env=SPEEDYSPEC_PHP_VERSION={pinned}");

    // docs.rs builds the documentation without any PHP; ext-php-rs uses its bundled bindings there.
    if env::var_os("DOCS_RS").is_some() {
        return;
    }

    let prefix = env::var_os("SPEEDYSPEC_PHP_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            fail(
                "SPEEDYSPEC_PHP_PREFIX is not set; point it at a static PHP built by scripts/build-php.sh \
                 (its buildroot), with PHP, PHP_CONFIG and SODIUM_LIB_DIR beside it",
            )
        });
    let php_config = prefix.join("bin/php-config");
    let libphp = prefix.join("lib/libphp.a");
    if !php_config.is_file() || !libphp.is_file() {
        fail(&format!(
            "no static PHP at {} (expected bin/php-config and lib/libphp.a); run scripts/build-php.sh",
            prefix.display()
        ));
    }
    println!("cargo:rerun-if-changed={}", libphp.display());

    for (var, expected) in [
        ("PHP_CONFIG", php_config.clone()),
        ("PHP", prefix.join("bin/php")),
    ] {
        let actual = env::var_os(var).map(PathBuf::from);
        if actual.as_deref().map(canonical) != Some(canonical(&expected)) {
            fail(&format!(
                "{var} is {actual:?} but must be {} so ext-php-rs builds against the same PHP; \
                 set {var} alongside SPEEDYSPEC_PHP_PREFIX (see .cargo/config.toml)",
                expected.display()
            ));
        }
    }

    // libsodium-sys-stable (the Rust crypto port) links libsodium.a from SODIUM_LIB_DIR. It must be this PHP's own
    // archive: two libsodium archives in one link resolve symbol by symbol, mixing objects of both versions.
    let sodium_dir = env::var_os("SODIUM_LIB_DIR").map(PathBuf::from);
    if sodium_dir.as_deref().map(canonical) != Some(canonical(&prefix.join("lib"))) {
        fail(&format!(
            "sodium: SODIUM_LIB_DIR is {sodium_dir:?} but must be {} so Rust links PHP's own libsodium; \
             set it alongside SPEEDYSPEC_PHP_PREFIX (see .cargo/config.toml)",
            prefix.join("lib").display()
        ));
    }

    let built = php_config_output(&php_config, "--version");
    if built != pinned {
        fail(&format!(
            "php-version pins {pinned} but {} is {built}; build PHP {pinned} with scripts/build-php.sh",
            prefix.display()
        ));
    }

    link(&prefix);
}

/// Links libphp.a and the libraries it needs, as listed in `link-flags.txt`,
/// which `scripts/build-php.sh` writes from `spc spc-config --libs`
/// (`php-config --libs` misses some of them). Every `-l` that the prefix has
/// as a static archive is linked statically; the rest are system libraries.
fn link(prefix: &Path) {
    let lib_dir = prefix.join("lib");
    let flags_file = prefix.join("link-flags.txt");
    println!("cargo:rerun-if-changed={}", flags_file.display());
    let flags = fs::read_to_string(&flags_file).unwrap_or_else(|e| {
        fail(&format!(
            "reading {}: {e}; rerun scripts/build-php.sh",
            flags_file.display()
        ))
    });

    // WordPress needs ext-sodium and the runtime guarantees it; a PHP built without it must not link.
    if !flags.split_whitespace().any(|token| token == "-lsodium") {
        fail(&format!(
            "sodium: {} has no -lsodium; ext-sodium is required, rerun scripts/build-php.sh",
            flags_file.display()
        ));
    }
    if !lib_dir.join("libsodium.a").is_file() {
        fail(&format!(
            "sodium: {} is missing; ext-sodium is required, rerun scripts/build-php.sh",
            lib_dir.join("libsodium.a").display()
        ));
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    let mut tokens = flags.split_whitespace();
    while let Some(token) = tokens.next() {
        if let Some(dir) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = token.strip_prefix("-l") {
            if lib_dir.join(format!("lib{name}.a")).is_file() {
                println!("cargo:rustc-link-lib=static:-bundle={name}");
            } else {
                println!("cargo:rustc-link-lib={name}");
            }
        } else if token == "-framework" {
            if let Some(name) = tokens.next() {
                println!("cargo:rustc-link-lib=framework={name}");
            }
        } else {
            fail(&format!(
                "unexpected link flag {token:?} in {}",
                flags_file.display()
            ));
        }
    }

    // PHP's opcache JIT detects CPU features with GCC's `__cpu_model` and
    // friends, which live in libgcc. gcc links libgcc by default, so spc does
    // not list it, but rustc links with `-nodefaultlibs`.
    // On macOS the same symbols come from clang's runtime, libclang_rt.osx.a, which
    // clang links by default and `-nodefaultlibs` drops too. Only x86_64 objects use
    // them (the CPU checks are x86 only), but the library is the one clang itself
    // would link on every Mac, so it is linked for both architectures.
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-link-lib=static:-bundle=gcc"),
        Ok("macos") => {
            let resource_dir = clang_resource_dir();
            let darwin = resource_dir.join("lib/darwin");
            if !darwin.join("libclang_rt.osx.a").is_file() {
                fail(&format!(
                    "clang's runtime library libclang_rt.osx.a is not in {}",
                    darwin.display()
                ));
            }
            println!("cargo:rustc-link-search=native={}", darwin.display());
            println!("cargo:rustc-link-lib=static:-bundle=clang_rt.osx");
        }
        _ => {}
    }
}

/// Where clang keeps its own headers and runtime libraries (`cc -print-resource-dir`).
fn clang_resource_dir() -> PathBuf {
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let output = Command::new(&cc)
        .arg("-print-resource-dir")
        .output()
        .unwrap_or_else(|e| fail(&format!("running {cc} -print-resource-dir: {e}")));
    if !output.status.success() {
        fail(&format!("{cc} -print-resource-dir failed"));
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

fn php_config_output(php_config: &Path, arg: &str) -> String {
    let output = Command::new(php_config)
        .arg(arg)
        .output()
        .unwrap_or_else(|e| fail(&format!("running {} {arg}: {e}", php_config.display())));
    if !output.status.success() {
        fail(&format!("{} {arg} failed", php_config.display()));
    }
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn fail(message: &str) -> ! {
    eprintln!("speedyspec-php: {message}");
    std::process::exit(1);
}
