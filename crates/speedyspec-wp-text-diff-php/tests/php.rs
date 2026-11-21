//! `wp_text_diff()` in embedded PHP: the facade over the native function, with WordPress 7.1.5's own functions
//! loaded around it.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_text_diff_php::TextDiffArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn text(result: &Result<HookValue, PhpError>) -> String {
    match result {
        Ok(HookValue::Str(s)) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn main() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container, Rc::new(MemoryHookStore::new()));
    let text_diff = TextDiffArea::new(hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(text_diff)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    // WordPress's functions around the facade; plugin.php is the native Hook API's.
    let load = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); error_reporting(E_ALL & ~E_DEPRECATED);
         foreach (array('load.php', 'functions.php', 'formatting.php', 'pomo/translations.php', 'l10n.php', 'pluggable.php') as $f) {{ require ABSPATH . WPINC . '/' . $f; }}",
        format!("{}/", release.display())
    );
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");

    let r = php.eval(
        "speedyspec://test/native",
        "return (new ReflectionFunction('wp_text_diff'))->getFileName();",
    );
    check(
        "wp_text_diff_is_the_facade",
        text(&r) == "speedyspec://wp-text-diff",
        &r,
    );

    // tests/phpunit/tests/pluggable/signatures.php's check for wp_text_diff().
    let r = php.eval(
        "speedyspec://test/signature",
        "$p = (new ReflectionFunction('wp_text_diff'))->getParameters();
         return count($p) === 3 && $p[0]->getName() === 'left_string' && !$p[0]->isOptional()
             && $p[1]->getName() === 'right_string' && !$p[1]->isOptional()
             && $p[2]->getName() === 'args' && $p[2]->isOptional() && $p[2]->getDefaultValue() === null;",
    );
    check(
        "signature_matches_pluggable",
        matches!(r, Ok(HookValue::Bool(true))),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/basic",
        "return wp_text_diff('Hello world', 'Hello there world');",
    );
    check(
        "inline_insertion",
        text(&r).contains("Hello<ins> there</ins> world\n</td>"),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/same",
        "return wp_text_diff(\"a  b\", 'a b');",
    );
    check("normalized_equal_is_empty", text(&r).is_empty(), &r);

    let r = php.eval(
        "speedyspec://test/legacy",
        "return wp_text_diff(\"a\\nb\\nc\\nd\\ne\", \"a\\nb\\nX\\nd\\ne\", array('leading_context_lines' => 1, 'trailing_context_lines' => 1));",
    );
    check(
        "renderer_arguments_use_wordpress_classes",
        !text(&r).contains(">a</td>") && text(&r).contains(">b</td>"),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/invalid",
        "try { wp_text_diff(\"a\\xffb c\", 'a b d'); return 'no error'; } catch (TypeError $e) { return 'TypeError'; }",
    );
    check(
        "invalid_utf8_throws_type_error",
        text(&r) == "TypeError",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/throwing-filter",
        "add_filter('process_text_diff_html', function () { throw new RuntimeException('stop'); });
         try { wp_text_diff('a', 'b'); wp_text_diff(\"x\\ny\", \"x\\nz\"); return 'no error'; } catch (RuntimeException $e) { return $e->getMessage(); }
         finally { remove_all_filters('process_text_diff_html'); }",
    );
    check("filter_exceptions_propagate", text(&r) == "stop", &r);

    let r = php.eval(
        "speedyspec://test/float-filter",
        "add_filter('process_text_diff_html', function () { return 0.1; });
         $r = wp_text_diff(\"x\\ny\", \"x\\nz\"); remove_all_filters('process_text_diff_html'); return $r;",
    );
    check(
        "float_filter_results_convert_as_php",
        text(&r).contains("</span>0.1</td>"),
        &r,
    );

    // Every recorded case, in order, through the facade.
    php.restart_request().expect("a fresh request");
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");
    let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/text-diff/cases.json");
    let replay = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/text-diff/replay.php");
    let code = format!(
        "define('SPEEDYSPEC_TEXT_DIFF_CASES', {:?}); return require {:?};",
        cases.display().to_string(),
        replay.display().to_string()
    );
    let r = php.eval("speedyspec://test/replay", &code);
    check(
        "recorded_cases_replay",
        text(&r).ends_with("all passed"),
        text(&r),
    );
    println!("{}", text(&r));

    // Not a check: the native function against WordPress's own PHP classes, same process, same calls.
    let timing = format!(
        "$c = json_decode(file_get_contents({:?}), true); $p = array();
         foreach ($c as $x) {{ if (is_string($x['left']) && is_string($x['right']) && !isset($x['filter']) && !isset($x['throws'])) {{ $p[] = array($x['left'], $x['right']); }} }}
         $time = function ($f) use ($p) {{ $best = INF; for ($k = 0; $k < 5; $k++) {{ $s = hrtime(true); foreach ($p as $q) {{ $f($q[0], $q[1]); }} $best = min($best, (hrtime(true) - $s) / 1e6); }} return $best; }};
         $native = $time(fn ($l, $r) => wp_text_diff($l, $r));
         $php = $time(fn ($l, $r) => \\SpeedySpec\\WpTextDiff\\legacy_wp_text_diff($l, $r, wp_parse_args(null, array('title' => '', 'title_left' => '', 'title_right' => '', 'show_split_view' => true))));
         return sprintf('%d calls: native %.2f ms, WordPress PHP %.2f ms (%.1fx)', count($p), $native, $php, $php / $native);",
        cases.display().to_string()
    );
    println!("{}", text(&php.eval("speedyspec://test/timing", &timing)));
}
