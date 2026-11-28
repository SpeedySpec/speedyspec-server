//! `wp_insert_post()` and `wp_update_post()`, native against stock WordPress, in the embedded PHP over one MySQL
//! database.
//!
//! - native: the post writers are the Rust ones (the release `prepare_release` strips, with the native options), with
//!   the native Hook API, `wpdb`, object cache, meta and option functions beside them, as compat_write boots them;
//! - stock: WordPress's own `post.php`, `meta.php`, `option.php`, hooks, `wpdb` and object cache from the unstripped
//!   release, in the same embedded PHP booted with no native areas.
//!
//! PHP boots once per process, so each side runs in its own process: with `SPEEDYSPEC_BENCH_SIDE` unset this binary
//! starts itself once for `native` and once for `stock`; with it set it benchmarks that side. Both write to
//! criterion's usual directory, under `<case>/<side>`.
//!
//! The loops run inside PHP and PHP times them (`hrtime`), so what is measured is the PHP function and nothing of
//! the Rust-to-PHP call. Needs MySQL: `SPEEDYSPEC_TEST_MYSQL_URL=mysql://user@host:port/` plus a database name; the
//! bench uses `<database>_posts_write_bench`, which the seed (compat/posts-write/setup.php, single site, with the
//! default callbacks the writers run) empties. Both sides run as user 1, an administrator with `unfiltered_html`.
//!
//! Cases, each timing only the writer call:
//! - `wp_insert_post` of a published post (title, content, author 1); the post (and the revision
//!   `wp_save_post_revision_on_insert` adds) is deleted with `wp_delete_post($id, true)` after each call, untimed, so
//!   the table stays the seed's size and every insert takes the same slug;
//! - `wp_update_post` of post 6, a new title and content each call; the revision `wp_save_post_revision` adds is
//!   deleted after each call, untimed, so post 6 never gathers revisions;
//! - `wp_insert_post` as the first, with `meta_input` (one key) and `tags_input` (the seeded tag `tagged`); the
//!   untimed delete removes the meta and the term relationship too, and recounts the tag.

#[path = "../tests/support/mod.rs"]
mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use criterion::Criterion;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_php_ext::Value;
use speedyspec_wp_posts_php::php_string;

/// The functions the cases call; the loops are in PHP, and each returns the nanoseconds the timed work took.
const FUNCTIONS: &str = r#"
function bench_post_args() {
    return ['post_title' => 'Bench post', 'post_content' => 'Bench content.', 'post_status' => 'publish',
        'post_author' => 1];
}
function bench_delete($id) {
    if (!is_int($id) || $id < 1) { throw new RuntimeException('the insert failed: ' . var_export($id, true)); }
    if (!wp_delete_post($id, true)) { throw new RuntimeException("post $id was not deleted"); }
}
function bench_drop_revisions($parent) {
    global $wpdb;
    $ids = $wpdb->get_col($wpdb->prepare(
        "SELECT ID FROM $wpdb->posts WHERE post_parent = %d AND post_type = 'revision'", $parent));
    foreach ($ids as $id) { wp_delete_post((int) $id, true); }
}
function bench_insert($n) {
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        $args = bench_post_args();
        $start = hrtime(true);
        $id = wp_insert_post($args);
        $total += hrtime(true) - $start;
        bench_delete($id);
    }
    return $total;
}
function bench_update($n) {
    static $round = 0;
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        $round++;
        $args = ['ID' => 6, 'post_title' => "Taken $round", 'post_content' => "Content $round."];
        $start = hrtime(true);
        $id = wp_update_post($args);
        $total += hrtime(true) - $start;
        if (6 !== $id) { throw new RuntimeException('the update failed: ' . var_export($id, true)); }
        bench_drop_revisions(6);
    }
    return $total;
}
function bench_insert_meta($n) {
    $total = 0;
    for ($i = 0; $i < $n; $i++) {
        $args = bench_post_args() + ['meta_input' => ['bench_key' => 'bench value'], 'tags_input' => ['tagged']];
        $start = hrtime(true);
        $id = wp_insert_post($args);
        $total += hrtime(true) - $start;
        bench_delete($id);
    }
    return $total;
}
return true;
"#;

/// Both sides must write the same before anything is timed: an insert that takes the free slug, meta and tag, an
/// update that keeps post 6's slug, and the cleanup that leaves the table as seeded.
const CHECK: &str = r#"
global $wpdb;
$count = (int) $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
$id = wp_insert_post(bench_post_args() + ['meta_input' => ['bench_key' => 'bench value'], 'tags_input' => ['tagged']]);
$p = get_post($id);
$ok = $p instanceof WP_Post && 'bench-post' === $p->post_name && 'publish' === $p->post_status
    && 'bench value' === get_post_meta($id, 'bench_key', true)
    && ['tagged'] === wp_get_post_tags($id, ['fields' => 'slugs']);
bench_delete($id);
$ok = $ok && 6 === wp_update_post(['ID' => 6, 'post_title' => 'Checked', 'post_content' => 'Checked.'])
    && 'taken' === get_post(6)->post_name && 'Checked' === get_post(6)->post_title;
bench_drop_revisions(6);
return $ok && $count === (int) $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
"#;

fn boot(side: &str) -> Php {
    let Some(url) = support::mysql("posts_write_bench") else {
        eprintln!("needs SPEEDYSPEC_TEST_MYSQL_URL");
        std::process::exit(2);
    };
    let release =
        speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("posts-write"))
            .expect("the pinned release");
    let (php, setup) = if side == "native" {
        let (_, root) = support::prepared_with("posts-write-bench", true);
        let (php, _container) = support::boot_with(&url, false, true);
        (php, support::write_setup_code(&root))
    } else {
        let php = Php::boot(BootOptions {
            hooks: std::rc::Rc::new(MemoryHooks::new()),
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
                    .join("posts-write/setup.php")
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
    php.eval(
        "speedyspec://bench/user",
        "wp_set_current_user(1); return true;",
    )
    .expect("user 1 is current");
    let check = php
        .eval("speedyspec://bench/check", CHECK)
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
        ("wp_insert_post", "bench_insert"),
        ("wp_update_post", "bench_update"),
        ("wp_insert_post_meta_tags", "bench_insert_meta"),
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
