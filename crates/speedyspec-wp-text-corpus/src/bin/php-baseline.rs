//! `php-baseline <function>`: times stock WordPress's `<function>` in the embedded PHP over the corpus's inputs,
//! printing MB/s for the whole posts and for the long page, the same groups the Rust benches report.
//!
//! Each rate is given by wall time and by CPU time (`getrusage()`); on a busy machine the CPU-time rate is the one
//! to compare with the Rust benches, which measure thread CPU time.
//!
//! `php-baseline the_content` times `apply_filters( 'the_content', ... )` with only formatting's ported callbacks
//! registered as `default-filters.php` registers them (`wptexturize`, `wpautop`, `shortcode_unautop`, then
//! `capital_P_dangit` at 11, `convert_smilies` at 20), over `wptexturize`'s inputs.

use std::rc::Rc;
use std::time::Duration;

use speedyspec_compat::{DRIVER, compat_dir, wordpress_release};
use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_wp_text_corpus::{inputs, page, short_inputs, wordpress};

/// The PHP side's timing loop: runs `$function` over every input until `$seconds` pass, and answers bytes per second
/// by wall time and by CPU time.
const TIMER: &str = r#"
function speedyspec_formatting_cpu(): float
{
    $usage = getrusage();
    return $usage['ru_utime.tv_sec'] + $usage['ru_utime.tv_usec'] / 1e6 + $usage['ru_stime.tv_sec'] + $usage['ru_stime.tv_usec'] / 1e6;
}

function speedyspec_formatting_time(string $function, string $inputs_json, float $seconds): array
{
    $inputs = array_map('base64_decode', json_decode($inputs_json, true));
    if ('the_content' === $function) {
        add_filter('the_content', 'wptexturize');
        add_filter('the_content', 'wpautop');
        add_filter('the_content', 'shortcode_unautop');
        add_filter('the_content', 'capital_P_dangit', 11);
        add_filter('the_content', 'convert_smilies', 20);
        $run = static fn ($input) => apply_filters('the_content', $input);
    } else {
        $run = $function;
    }
    $bytes = 0;
    $start = hrtime(true);
    $cpu = speedyspec_formatting_cpu();
    $limit = $start + (int) ($seconds * 1e9);
    do {
        foreach ($inputs as $input) {
            $run($input);
            $bytes += strlen($input);
        }
    } while (hrtime(true) < $limit);
    return [$bytes / ((hrtime(true) - $start) / 1e9), $bytes / (speedyspec_formatting_cpu() - $cpu)];
}
"#;

fn main() {
    if let Err(why) = run() {
        eprintln!("{why}");
        std::process::exit(1);
    }
}

fn base64_list(items: &[Vec<u8>]) -> String {
    use base64::Engine;
    let encoded: Vec<String> = items
        .iter()
        .map(|item| base64::engine::general_purpose::STANDARD.encode(item))
        .collect();
    serde_json::to_string(&encoded).unwrap()
}

fn run() -> Result<(), String> {
    let function = std::env::args()
        .nth(1)
        .ok_or("usage: php-baseline <function>")?;
    let root = wordpress_release(&wordpress())?;
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .map_err(|e| e.to_string())?;
    php.eval("speedyspec://compat/driver", DRIVER)
        .map_err(|e| format!("loading the driver: {e}"))?;
    let setup = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', false); require {:?};",
        format!("{}/", root.display()),
        compat_dir().join("formatting/setup.php").display().to_string()
    );
    php.eval("speedyspec://compat/setup", &setup)
        .map_err(|e| format!("loading WordPress: {e}"))?;
    php.eval("speedyspec://formatting/timer", TIMER)
        .map_err(|e| format!("loading the timer: {e}"))?;
    let source = if function == "the_content" { "wptexturize" } else { function.as_str() };
    let posts: Vec<Vec<u8>> = inputs(source)
        .into_iter()
        .filter(|(scenario, _)| scenario.contains("/whole_posts/"))
        .map(|(_, bytes)| bytes)
        .collect();
    let seconds = Duration::from_secs(2).as_secs_f64();
    println!("WordPress {} {function} in the embedded PHP", wordpress());
    let mut groups = Vec::new();
    if !posts.is_empty() {
        groups.push(("whole_posts", posts));
        groups.push(("page", vec![page(source)]));
    }
    let short = short_inputs(source);
    if !short.is_empty() && function != "the_content" {
        groups.push(("short", short));
    }
    for (group, items) in groups {
        let rate = php
            .call(
                &Value::Str("speedyspec_formatting_time".into()),
                &[
                    Value::Str(function.clone()),
                    Value::Str(base64_list(&items)),
                    Value::Float(seconds),
                ],
            )
            .map_err(|e| format!("timing {group}: {e}"))?;
        let rates: Vec<f64> = match &rate {
            Value::Array(items) => items.iter().filter_map(|(_, v)| if let Value::Float(f) = v { Some(*f) } else { None }).collect(),
            _ => Vec::new(),
        };
        let [wall, cpu] = rates[..] else {
            return Err(format!("the timer returned {rate:?}"));
        };
        let mib = 1024.0 * 1024.0;
        println!("{function}/{group}: {:.1} MiB/s wall, {:.1} MiB/s CPU", wall / mib, cpu / mib);
    }
    Ok(())
}
