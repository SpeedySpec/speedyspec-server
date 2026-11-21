//! `php-baseline <function>`: times stock WordPress's `<function>` in the embedded PHP over the corpus's inputs,
//! printing MB/s for the whole posts and for the long page, the same groups the Rust benches report.

use std::rc::Rc;
use std::time::Duration;

use speedyspec_compat::{DRIVER, compat_dir, wordpress_release};
use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_wp_text_corpus::{WORDPRESS, inputs, page};

/// The PHP side's timing loop: runs `$function` over every input until `$seconds` pass, and answers bytes per second.
const TIMER: &str = r#"
function speedyspec_formatting_time(string $function, string $inputs_json, float $seconds): float
{
    $inputs = array_map('base64_decode', json_decode($inputs_json, true));
    $bytes = 0;
    $start = hrtime(true);
    $limit = $start + (int) ($seconds * 1e9);
    do {
        foreach ($inputs as $input) {
            $function($input);
            $bytes += strlen($input);
        }
    } while (hrtime(true) < $limit);
    return $bytes / ((hrtime(true) - $start) / 1e9);
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
    let root = wordpress_release(WORDPRESS)?;
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
    let posts: Vec<Vec<u8>> = inputs(&function)
        .into_iter()
        .filter(|(scenario, _)| scenario.contains("/whole_posts/"))
        .map(|(_, bytes)| bytes)
        .collect();
    let seconds = Duration::from_secs(2).as_secs_f64();
    println!("WordPress {WORDPRESS} {function} in the embedded PHP");
    for (group, items) in [("whole_posts", posts), ("page", vec![page(&function)])] {
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
        let Value::Float(rate) = rate else {
            return Err(format!("the timer returned {rate:?}"));
        };
        println!("{function}/{group}: {:.1} MiB/s", rate / (1024.0 * 1024.0));
    }
    Ok(())
}
