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
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
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
        "speedyspec://test/wp-locale",
        &format!(
            "load_textdomain('default', {:?}, 'de_DE');
             $served = SpeedySpec\\WpI18n\\Native\\locale_strings(true);
             $cb = function ($t) {{ return $t; }};
             add_filter('gettext', $cb);
             $hooked = SpeedySpec\\WpI18n\\Native\\locale_strings(true);
             $l = new WP_Locale();
             remove_filter('gettext', $cb);
             unload_textdomain('default');
             $plugin = new class extends WP_Locale {{ public $extra; public function init() {{ parent::init(); $this->extra = 'x'; }} }};
             return json_encode(array((new ReflectionClass('WP_Locale'))->getFileName(), count($served), $served[0], $served[64],
                 $hooked, $l->weekday[0], $plugin->extra, $plugin->month['01']), JSON_UNESCAPED_SLASHES);",
            root.join("crates/speedyspec-wp-i18n-files/tests/data/rtl.mo").display().to_string()
        ),
    );
    check(
        "wp_locale_is_native",
        text(&r)
            == r#"["speedyspec://wp-i18n",65,"\u05d9\u05d5\u05dd \u05e8\u05d0\u05e9\u05d5\u05df","rtl",null,"\u05d9\u05d5\u05dd \u05e8\u05d0\u05e9\u05d5\u05df","x","January"]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/switcher",
        "global $wp_locale_switcher;
         $saved = $wp_locale_switcher;
         $wp_locale_switcher = new WP_Locale_Switcher();
         $wp_locale_switcher->init();
         $file = (new ReflectionClass('WP_Locale_Switcher'))->getFileName();
         $once = function () use (&$once) { remove_action('change_locale', $once, 5); switch_to_locale('de_DE'); };
         add_action('change_locale', $once, 5);
         $nested = array(switch_to_locale('es_ES'), $wp_locale_switcher->get_switched_locale(), restore_previous_locale(), restore_current_locale());
         $boom = function () { throw new DomainException('boom'); };
         add_action('change_locale', $boom);
         try { switch_to_locale('es_ES'); $thrown = 'none'; } catch (DomainException $e) { $thrown = get_class($e) . ':' . $e->getMessage(); }
         remove_action('change_locale', $boom);
         $after = array(is_locale_switched(), restore_current_locale(), is_locale_switched());
         remove_filter('locale', array($wp_locale_switcher, 'filter_locale'));
         remove_filter('determine_locale', array($wp_locale_switcher, 'filter_locale'));
         $wp_locale_switcher = $saved;
         unload_textdomain('default', true);
         return json_encode(array($file, $nested, $thrown, $after), JSON_UNESCAPED_SLASHES);",
    );
    check(
        "switcher_is_native",
        text(&r)
            == r#"["speedyspec://wp-i18n",[true,"de_DE","es_ES","en_US"],"DomainException:boom",[true,"en_US",false]]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/script",
        "$f = sys_get_temp_dir() . '/speedyspec-script-' . getmypid() . '.json';
         file_put_contents($f, '{\"v\":1}');
         $first = load_script_translations($f, 'h', 'default');
         file_put_contents($f, '{\"v\":22}');
         $second = load_script_translations($f, 'h', 'default');
         unlink($f);
         return json_encode(array((new ReflectionFunction('load_script_translations'))->isInternal(),
             (new ReflectionFunction('_load_script_textdomain_from_src'))->isInternal(), $first, $second));",
    );
    check(
        "script_functions_are_native",
        text(&r) == r#"[true,true,"{\"v\":1}","{\"v\":22}"]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/schema",
        "$o = new stdClass();
         $seen = array();
         $cb = function ($t, $text, $context, $domain) use (&$seen) { $seen[] = \"$text|$context|$domain\"; return strtoupper($t); };
         add_filter('gettext_with_context', $cb, 10, 4);
         $out = translate_settings_using_i18n_schema(
             (object) array('title' => 'block title', 'keywords' => array('kw'), '*' => 'star'),
             array('title' => 'Notice', 'keywords' => array('a', 'b'), 'other' => $o, 7 => 'seven'),
             'dom'
         );
         remove_filter('gettext_with_context', $cb, 10);
         return json_encode(array((new ReflectionFunction('translate_settings_using_i18n_schema'))->isInternal(),
             $out['title'], $out['keywords'], $out['other'] === $o, $out[7], $seen));",
    );
    check(
        "schema_translation_is_native",
        text(&r)
            == r#"[true,"NOTICE",["A","B"],true,"SEVEN",["Notice|block title|dom","a|kw|dom","b|kw|dom","seven|star|dom"]]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/schema-throws",
        "$cb = function () { throw new LengthException('no'); };
         add_filter('gettext_with_context', $cb);
         try { translate_settings_using_i18n_schema((object) array('a' => 'ctx'), array('a' => 'x'), 'dom'); $out = 'none'; }
         catch (LengthException $e) { $out = get_class($e) . ':' . $e->getMessage(); }
         remove_filter('gettext_with_context', $cb);
         return $out;",
    );
    check(
        "schema_translation_rethrows",
        text(&r) == "LengthException:no",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/schema-references",
        "$x = 'a'; $y = 'p';
         $r = translate_settings_using_i18n_schema((object) array('zz' => 'c', 'l' => array('c')), array('k' => &$x, 'l' => array(&$y)), 'd');
         $r['k'] = 'b'; $r['l'][0] = 'q';
         return $x . $y;",
    );
    check("schema_translation_copies_references", text(&r) == "ap", &r);

    let r = php.eval(
        "speedyspec://test/plural-forms",
        "$p = new Plural_Forms('n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2');
         try { new Plural_Forms('n # 2'); $thrown = 'none'; } catch (Exception $e) { $thrown = get_class($e) . ':' . $e->getMessage(); }
         $tampered = new class('n') extends Plural_Forms { public function parse($str) { $this->tokens = array(array('var'), array('value', 1), array('op', '^')); } };
         try { $tampered->get(3); $odd = 'none'; } catch (Exception $e) { $odd = $e->getMessage(); }
         return json_encode(array((new ReflectionClass('Plural_Forms'))->getFileName(), $p->get(22), $p->get('22'), $thrown, $odd), JSON_UNESCAPED_SLASHES);",
    );
    check(
        "plural_forms_is_native",
        text(&r)
            == r##"["speedyspec://wp-i18n",1,1,"Exception:Unknown symbol \"#\"","Unknown operator \"^\""]"##,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/file-export",
        &format!(
            "$before = SpeedySpec\\WpI18n\\Native\\export_stats();
             $mo = WP_Translation_File::transform({f:?}, 'mo');
             $php = WP_Translation_File::transform({f:?}, 'php');
             return json_encode(array(md5($mo), $php, SpeedySpec\\WpI18n\\Native\\export_stats() - $before));",
            f = root.join("tests/php/data/l10n/simple.mo").display().to_string()
        ),
    );
    check(
        "translation_file_export_is_native",
        text(&r).ends_with(",2]") && text(&r).contains("'messages'=>"),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/mo",
        &format!(
            "$before = SpeedySpec\\WpI18n\\Native\\pomo_stats();
             $mo = new MO(); $ok = $mo->import_from_file({f:?}); $bytes = $mo->export();
             $sub = new class extends MO {{ public $seen = 0; public function &make_entry($o, $t) {{ ++$this->seen; $e = &parent::make_entry($o, $t); return $e; }} }};
             $sub->import_from_file({f:?});
             $missing = @(new MO())->import_from_file('/nonexistent.mo');
             return json_encode(array($ok, count($mo->entries), strlen($bytes), $sub->seen, $missing, SpeedySpec\\WpI18n\\Native\\pomo_stats() - $before));",
            f = root.join("tests/php/data/pomo/simple.mo").display().to_string()
        ),
    );
    check("mo_is_native", text(&r) == "[true,2,202,2,false,3]", &r);

    let r = php.eval(
        "speedyspec://test/po",
        &format!(
            "$before = SpeedySpec\\WpI18n\\Native\\pomo_stats();
             $po = new PO(); $ok = $po->import_from_file({f:?});
             $after_native = SpeedySpec\\WpI18n\\Native\\pomo_stats() - $before;
             $sub = new class extends PO {{ public $reads = 0; public function read_entry($f, $lineno = 0) {{ ++$this->reads; return parent::read_entry($f, $lineno); }} }};
             $sub->import_from_file({f:?});
             return json_encode(array($ok, count($po->entries), $after_native, $sub->reads, SpeedySpec\\WpI18n\\Native\\pomo_stats() - $before,
                 PO::poify(\"a\\tb\\\"c\"), (new ReflectionMethod('PO', 'poify'))->getFileName() !== false));",
            f = root.join("tests/php/data/pomo/simple.po").display().to_string()
        ),
    );
    check("po_is_native", text(&r).starts_with("[true,6,1,8,1,"), &r);

    let r = php.eval(
        "speedyspec://test/create-as",
        &format!(
            "unload_textdomain('fast'); load_textdomain('fast', {mo:?}, 'de_DE');
             $f = tempnam(sys_get_temp_dir(), 'wpunit'); copy({src:?}, $f);
             $GLOBALS['speedyspec_includes'] = 0;
             file_put_contents($f, str_replace('<?php', '<?php ++$GLOBALS[\"speedyspec_includes\"];', file_get_contents($f)));
             $file = WP_Translation_File::create($f, 'php');
             $t = $file->translate('a');
             unlink($f);
             return json_encode(array($t, $GLOBALS['speedyspec_includes'], $GLOBALS['speedyspec_reentry']));",
            src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/reentrant.l10n.php").display().to_string(),
            mo = root.join("crates/speedyspec-wp-i18n-files/tests/data/simple.mo").display().to_string()
        ),
    );
    check(
        "a_file_read_as_another_type_is_read_once",
        text(&r) == r#"["b",1,"dyado"]"#,
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
