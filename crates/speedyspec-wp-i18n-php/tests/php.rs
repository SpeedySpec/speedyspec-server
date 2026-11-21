//! The translation functions in embedded PHP: the native `__()` family, the controller and file shells, and
//! WordPress's own l10n code around them, replayed against WordPress 7.1.5 (`compat/i18n/cases.json`, recorded by
//! `compat/i18n/record.php` over the fixtures).
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_i18n_php::{I18nArea, prepare_release};
use speedyspec_wp_utf8_php::Utf8Area;

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
    let prepared = prepare_release(&release, &release.with_file_name("wordpress-i18n-port"))
        .expect("release prepared");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let i18n = I18nArea::new(container, hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(Utf8Area), Rc::new(i18n)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let load = format!(
        "define('SPEEDYSPEC_I18N_ROOT', {:?}); define('SPEEDYSPEC_I18N_DATA', {:?}); require {:?};",
        prepared.display().to_string(),
        root.join("crates/speedyspec-wp-i18n-files/tests/data")
            .display()
            .to_string(),
        root.join("compat/i18n/load.php").display().to_string(),
    );
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's l10n files load");

    let r = php.eval(
        "speedyspec://test/native",
        "return implode(',', array((new ReflectionFunction('__'))->isInternal() ? 'native' : 'php', (new ReflectionClass('WP_Translation_Controller'))->getFileName()));",
    );
    check(
        "underscores_is_native_and_classes_are_the_facade",
        text(&r) == "native,speedyspec://wp-i18n",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/fast-path",
        &format!(
            "load_textdomain('fast', {:?}, 'de_DE'); $before = SpeedySpec\\WpI18n\\Native\\hot_stats();
             $t = array(__('baba', 'fast'), _n('baba', 'babas', 2, 'fast'), __('missing', 'fast'));
             $after = SpeedySpec\\WpI18n\\Native\\hot_stats();
             return json_encode(array($t, $after[0] - $before[0], $after[1] - $before[1]));",
            root.join("crates/speedyspec-wp-i18n-files/tests/data/simple.mo").display().to_string()
        ),
    );
    check(
        "plain_calls_take_the_fast_path",
        text(&r) == r#"[["dyado","dyado","missing"],3,0]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/empty-context",
        "$seen = array(); $cb = function ($t) use (&$seen) { $seen[] = $t; return $t; };
         add_filter('gettext_with_context', $cb);
         $out = array(_x('baba', '', 'fast'), _nx('baba', 'babas', 2, '', 'fast'), count($seen));
         remove_filter('gettext_with_context', $cb);
         return json_encode($out);",
    );
    check(
        "empty_context_runs_the_context_filters",
        text(&r) == r#"["dyado","dyado",1]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/self-replacing",
        "class SpeedySpec_Lazy extends Translations {
             public function translate($singular, $context = null) {
                 $GLOBALS['l10n']['lazy'] = new SpeedySpec_Lazy();
                 $padding = str_repeat('x', 64);
                 return 'L' . $singular;
             }
         }
         $GLOBALS['l10n']['lazy'] = new SpeedySpec_Lazy();
         $out = array();
         for ($i = 0; $i < 200; $i++) { $out[] = __('w' . $i, 'lazy'); }
         return json_encode(array($out[0], $out[199]));",
    );
    check(
        "a_translations_object_may_replace_itself",
        text(&r) == r#"["Lw0","Lw199"]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/reentry",
        &format!(
            "load_textdomain('re', {:?}, 'de_DE'); return json_encode(array(__('a', 're'), $GLOBALS['speedyspec_reentry']));",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/reentrant.l10n.php").display().to_string()
        ),
    );
    check(
        "an_included_translation_file_may_translate",
        text(&r) == r#"["b","dyado"]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/replay",
        &format!(
            "$r = require {:?}; return json_encode($r);",
            root.join("compat/i18n/replay.php").display().to_string()
        ),
    );
    let report: serde_json::Value =
        serde_json::from_str(&text(&r)).unwrap_or(serde_json::Value::Null);
    check(
        "recorded_calls_replay",
        report["calls"].as_i64().unwrap_or(0) > 1000
            && report["mismatches"]
                .as_array()
                .is_some_and(|m| m.is_empty()),
        &report,
    );
}
