//! `wp_text_diff()` in embedded PHP: the facade over the native function, with WordPress 7.1.5's own functions
//! loaded around it.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_text_diff::contracts::{TEXT_DIFFER, TextDiffer};
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
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let text_diff = TextDiffArea::new(hooks.api(), container.clone());
    text_diff.bind_defaults();
    let container_area = TextDiffArea::ports(ContainerArea::new("test", container.clone()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(text_diff),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    // WordPress's functions around the facade; plugin.php is the native Hook API's.
    let load = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); define('WP_DEBUG', true); error_reporting(E_ALL & ~E_DEPRECATED);
         foreach (array('load.php', 'functions.php', 'formatting.php', 'kses.php', 'pomo/translations.php', 'l10n.php', 'pluggable.php') as $f) {{ require ABSPATH . WPINC . '/' . $f; }}",
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
        "speedyspec://test/classes",
        "return class_exists('Text_Diff', false) && class_exists('WP_Text_Diff_Renderer_Table', false);",
    );
    check(
        "classes_load_as_in_wordpress",
        matches!(r, Ok(HookValue::Bool(true))),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/unknown-arg",
        "$seen = array(); set_error_handler(function ($no, $msg) use (&$seen) { $seen[] = $msg; return true; }, E_USER_DEPRECATED);
         wp_text_diff('a', 'b', array('foo' => 1)); restore_error_handler();
         return implode(' | ', $seen);",
    );
    check(
        "unknown_arguments_keep_wordpress_deprecation",
        text(&r).contains("foo"),
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

    let r = php.eval(
        "speedyspec://test/query-string",
        "return wp_text_diff('a', 'b', 'title=A&show_split_view=0');",
    );
    check(
        "query_string_args_parse_as_wordpress",
        text(&r).starts_with("<table class='diff'>\n<caption class='diff-title'>A</caption>"),
        &r,
    );

    // Values Rust cannot read go to WordPress's classes, as they did before the port.
    let r = php.eval(
        "speedyspec://test/unreadable-arg-key",
        "$seen = array(); set_error_handler(function ($no, $msg) use (&$seen) { $seen[] = $msg; return true; }, E_USER_DEPRECATED);
         $r = wp_text_diff('a', 'b', array(\"caf\\xE9\" => 1)); restore_error_handler();
         return str_starts_with($r, \"<table class='diff is-split-view'>\") && count($seen) === 1;",
    );
    check(
        "unreadable_argument_keys_use_wordpress_classes",
        matches!(r, Ok(HookValue::Bool(true))),
        &r,
    );
    let r = php.eval(
        "speedyspec://test/unreadable-query-string",
        "$seen = array(); set_error_handler(function ($no, $msg) use (&$seen) { $seen[] = $msg; return true; }, E_USER_DEPRECATED);
         $r = wp_text_diff('a', 'b', 'caf%E9=1'); restore_error_handler();
         return str_starts_with($r, \"<table class='diff is-split-view'>\") && count($seen) === 1;",
    );
    check(
        "unreadable_parsed_arguments_use_wordpress_classes",
        matches!(r, Ok(HookValue::Bool(true))),
        &r,
    );
    let r = php.eval(
        "speedyspec://test/recursive-text",
        "$a = array(); $a[] = &$a;
         try { wp_text_diff($a, 'b'); return 'no error'; } catch (TypeError $e) { return $e->getMessage(); }",
    );
    check(
        "unreadable_texts_fail_as_in_wordpress",
        text(&r).starts_with("trim(): Argument #1"),
        &r,
    );

    // A PHP object bound to the twin replaces wp_text_diff() for the request, with WordPress's raw arguments.
    let r = php.eval(
        "speedyspec://test/bound-twin",
        "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpTextDiff\\TextDiffer::class, fn () => new class implements \\SpeedySpec\\WpTextDiff\\TextDiffer { public function diff($left_string, $right_string, $args) { return \"R:$left_string|$right_string|\" . var_export($args, true); } });
         return implode(' # ', [wp_text_diff('a', 'b'), wp_text_diff('a', 'b', 'title=A&show_split_view=0'), wp_text_diff(1, 'b', array('x' => 1))]);",
    );
    check(
        "bound_twin_answers_wp_text_diff",
        text(&r)
            == "R:a|b|NULL # R:a|b|'title=A&show_split_view=0' # R:1|b|array (\n  'x' => 1,\n)",
        &r,
    );
    let from_rust = container
        .make_port::<dyn TextDiffer>(TEXT_DIFFER)
        .map(|differ| {
            differ.diff(
                &HookValue::Str("x".into()),
                &HookValue::Str("y".into()),
                &HookValue::Null,
            )
        });
    check(
        "rust_callers_get_the_php_binding",
        matches!(&from_rust, Ok(Ok(HookValue::Str(s))) if s == "R:x|y|NULL"),
        from_rust
            .as_ref()
            .map(|r| r.as_ref().map(|_| ()).map_err(|e| e.to_string()))
            .map_err(|e| e.to_string()),
    );
    php.restart_request().expect("a fresh request");
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");

    let r = php.eval(
        "speedyspec://test/binding-ends",
        "return wp_text_diff('Hello world', 'Hello there world');",
    );
    check(
        "binding_ends_with_the_request",
        text(&r).contains("Hello<ins> there</ins> world"),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/throwing-twin",
        "$GLOBALS['thrown'] = new RuntimeException('boom');
         \\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpTextDiff\\TextDiffer::class, fn () => new class implements \\SpeedySpec\\WpTextDiff\\TextDiffer { public function diff($left_string, $right_string, $args) { throw $GLOBALS['thrown']; } });
         try { wp_text_diff('a', 'b'); return 'no exception'; } catch (RuntimeException $e) { return $e === $GLOBALS['thrown'] ? 'same' : 'different'; }",
    );
    check(
        "twin_exception_is_rethrown_unchanged",
        text(&r) == "same",
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
