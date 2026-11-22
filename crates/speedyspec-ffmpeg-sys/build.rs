//! Links the static FFmpeg built by `scripts/build-ffmpeg.sh` (`FFMPEG_PREFIX`, set in `.cargo/config.toml`) and the
//! static PHP's own `libz.a` (`SPEEDYSPEC_PHP_PREFIX`), so PHP and FFmpeg share one zlib. Every other library FFmpeg
//! names in its `.pc` files must be a system library: a `-L` outside the two prefixes fails the build. On docs.rs
//! (`DOCS_RS`) nothing is linked.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const LIBS: [&str; 3] = ["avformat", "avcodec", "avutil"];

fn fail(message: String) -> ! {
    eprintln!("ffmpeg: {message}");
    process::exit(1);
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_PREFIX");
    println!("cargo:rerun-if-env-changed=SPEEDYSPEC_PHP_PREFIX");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    if env::var_os("DOCS_RS").is_some() {
        return;
    }

    let prefix = env::var_os("FFMPEG_PREFIX").map(PathBuf::from).unwrap_or_else(|| {
        fail("FFMPEG_PREFIX is not set; point it at the prefix scripts/build-ffmpeg.sh builds".into())
    });
    let php = env::var_os("SPEEDYSPEC_PHP_PREFIX").map(PathBuf::from).unwrap_or_else(|| {
        fail("SPEEDYSPEC_PHP_PREFIX is not set; FFmpeg links the static PHP's libz.a".into())
    });
    let lib_dir = prefix.join("lib");
    let php_lib = php.join("lib");
    for name in LIBS {
        let archive = lib_dir.join(format!("lib{name}.a"));
        if !archive.is_file() {
            fail(format!("{} is missing; run scripts/build-ffmpeg.sh", archive.display()));
        }
        println!("cargo:rerun-if-changed={}", archive.display());
    }
    println!("cargo:rerun-if-changed={}", prefix.join(".stamp").display());
    if !php_lib.join("libz.a").is_file() {
        fail(format!("{} is missing; FFmpeg links the static PHP's zlib", php_lib.join("libz.a").display()));
    }

    let allowed = [canonical(&lib_dir), canonical(&php_lib)];
    let mut system = Vec::new();
    for name in LIBS {
        let pc = lib_dir.join("pkgconfig").join(format!("lib{name}.pc"));
        let text = fs::read_to_string(&pc)
            .unwrap_or_else(|err| fail(format!("reading {}: {err}; run scripts/build-ffmpeg.sh", pc.display())));
        let mut tokens = text
            .lines()
            .filter(|line| line.starts_with("Libs:") || line.starts_with("Libs.private:"))
            .flat_map(|line| line.split_once(':').map(|(_, rest)| rest).unwrap_or("").split_whitespace())
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter();
        while let Some(token) = tokens.next() {
            if let Some(dir) = token.strip_prefix("-L") {
                let dir = dir.replace("${libdir}", &lib_dir.to_string_lossy());
                if !allowed.contains(&canonical(Path::new(&dir))) {
                    fail(format!("{} links from {dir}, outside the FFmpeg and PHP prefixes", pc.display()));
                }
            } else if let Some(lib) = token.strip_prefix("-l") {
                if !LIBS.contains(&lib) && lib != "z" && !system.contains(&lib.to_owned()) {
                    system.push(lib.to_owned());
                }
            } else if token == "-framework" {
                if let Some(framework) = tokens.next() {
                    println!("cargo:rustc-link-lib=framework={framework}");
                }
            } else if token == "-pthread" {
                system.push("pthread".to_owned());
            } else {
                fail(format!("unexpected link flag {token:?} in {}", pc.display()));
            }
        }
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", php_lib.display());
    for name in LIBS {
        println!("cargo:rustc-link-lib=static:-bundle={name}");
    }
    println!("cargo:rustc-link-lib=static:-bundle=z");
    for lib in system {
        println!("cargo:rustc-link-lib={lib}");
    }
}
