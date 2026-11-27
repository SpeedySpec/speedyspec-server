//! `get_post()` and `get_post_meta()`, native against stock WordPress, in the embedded PHP over one MySQL database.
//!
//! - native: the post and meta functions are the Rust ones (the release `prepare_release` strips), with the native
//!   Hook API, `wpdb` and object cache beside them;
//! - stock: WordPress's own `post.php`, `meta.php`, hooks, `wpdb` and object cache from the unstripped release, in
//!   the same embedded PHP booted with no native areas.
//!
//! PHP boots once per process, so each side runs in its own process: with `SPEEDYSPEC_BENCH_SIDE` unset this binary
//! starts itself once for `native` and once for `stock`; with it set it benchmarks that side. Both write to
//! criterion's usual directory, under `<case>/<side>`.
//!
//! The loops run inside PHP and PHP times them (`hrtime`), so what is measured is the PHP function and nothing of
//! the Rust-to-PHP call. Needs MySQL: `SPEEDYSPEC_TEST_MYSQL_URL=mysql://user@host:port/` plus a database name; the
//! bench uses `<database>_posts_bench`, which the seed (compat/posts-read/setup.php) empties.
//!
//! Cases: `get_post(1)` on a warm cache; `get_post(1)` after `wp_cache_flush_group('posts')` before each call (the
//! flush is not timed; the read, the post's `posts` entry rebuild and its meta and term cache priming are);
//! `get_post_meta(1, 'k', true)` on a warm meta cache.

#[path = "../tests/support/mod.rs"]
mod support;

use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use criterion::Criterion;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_php_ext::Value;
use speedyspec_wp_posts_php::php_string;

/// The functions the cases call; the loops are in PHP, and each returns the nanoseconds the timed work took.
const FUNCTIONS: &str = r#"
function bench_warm($n) {
    get_post(1);
    $start = hrtime(true);
    for ($i = 0; $i < $n; $i++) { get_post(1); }
    return hrtime(true) - $start;
}
function bench_cold($n) {
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        wp_cache_flush_group('posts');
        $start = hrtime(true);
        get_post(1);
        $total += hrtime(true) - $start;
    }
    return $total;
}
function bench_meta($n) {
    get_post_meta(1, 'k', true);
    $start = hrtime(true);
    for ($i = 0; $i < $n; $i++) { get_post_meta(1, 'k', true); }
    return hrtime(true) - $start;
}
return true;
"#;

fn boot(side: &str) -> Php {
    let Some(url) = support::mysql("posts_bench") else {
        eprintln!("needs SPEEDYSPEC_TEST_MYSQL_URL");
        std::process::exit(2);
    };
    let release =
        speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("posts-read"))
            .expect("the pinned release");
    let (php, setup) = if side == "native" {
        let (_, root) = support::prepared("posts-bench");
        let (php, _container) = support::boot(&url);
        (php, support::setup_code(&root))
    } else {
        let php = Php::boot(BootOptions {
            hooks: Rc::new(MemoryHooks::new()),
            preload: &[],
            areas: vec![],
        })
        .expect("PHP boots");
        let setup = format!(
            "define('SPEEDYSPEC_COMPAT_WP_ROOT', {}); define('SPEEDYSPEC_COMPAT_PORT', false); \
             define('SPEEDYSPEC_COMPAT_MULTISITE', false); require {}; return true;",
            php_string(&format!("{}/", release.display())),
            php_string(
                &speedyspec_compat::compat_dir()
                    .join("posts-read/setup.php")
                    .display()
                    .to_string()
            )
        );
        (php, setup)
    };
    php.eval("speedyspec://bench/setup", &setup)
        .expect("the seed loads");
    php.eval("speedyspec://bench/functions", FUNCTIONS)
        .expect("the bench functions define");
    // Both sides must answer the same before anything is timed.
    let check = php
        .eval(
            "speedyspec://bench/check",
            "$p = get_post(1); return $p instanceof WP_Post && 1 === $p->ID && 'hello-world' === $p->post_name \
             && 'v' === get_post_meta(1, 'k', true);",
        )
        .expect("the check runs");
    assert!(matches!(check, Value::Bool(true)), "{side}: {check:?}");
    php
}

fn timed(php: &Php, function: &str, n: u64) -> Duration {
    match php.call(&Value::Str(function.into()), &[Value::Int(n as i64)]) {
        Ok(Value::Int(ns)) => Duration::from_nanos(ns as u64),
        other => panic!("{function}: {other:?}"),
    }
}

fn run(side: &str) {
    let php = boot(side);
    let mut c = Criterion::default()
        .measurement_time(Duration::from_secs(8))
        .configure_from_args();
    for (case, function) in [
        ("get_post_warm", "bench_warm"),
        ("get_post_cold", "bench_cold"),
        ("get_post_meta_single", "bench_meta"),
    ] {
        let mut group = c.benchmark_group(case);
        group.bench_function(side, |b| b.iter_custom(|n| timed(&php, function, n)));
        group.finish();
    }
    c.final_summary();
}

fn main() {
    if let Ok(side) = std::env::var("SPEEDYSPEC_BENCH_SIDE") {
        return run(&side);
    }
    let exe = std::env::current_exe().expect("the bench binary's path");
    let args: Vec<String> = std::env::args().skip(1).collect();
    for side in ["native", "stock"] {
        let status = Command::new(Path::new(&exe))
            .args(&args)
            .env("SPEEDYSPEC_BENCH_SIDE", side)
            .status()
            .expect("the bench runs");
        assert!(status.success(), "{side} failed");
    }
}
