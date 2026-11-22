//! The theme.json classes in PHP, native over the Rust engine: construction, data access, cloning, merging through
//! `WP_Theme_JSON_Data`, constants, and bytes that are not UTF-8.
//!
//! PHP boots once per process, so this file has its own `main()` (`harness = false`).

use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_theme_json_php::ThemeJsonArea;

/// WordPress functions the adapters call (translation is the identity here).
const WORDPRESS: &str = r#"
function __($text, $domain = 'default') { return $text; }
function _deprecated_function($f, $v, $r = '') { $GLOBALS['notices'][] = "deprecated_function:$f|$v|$r"; }
function wp_trigger_error($f, $m, $l = E_USER_NOTICE) { $GLOBALS['notices'][] = "trigger_error:$f|$m"; }
function sanitize_title($t) { return strtolower($t); }
"#;

type Case = fn(&Php) -> Result<(), String>;

const CASES: &[(&str, Case)] = &[
    ("new_and_get_settings", case_new_and_get_settings),
    ("default_constructor", case_default_constructor),
    ("clone_does_not_share_state", case_clone),
    ("merge_reads_incoming_raw_data", case_merge_raw),
    (
        "data_update_with_returns_this_and_merges",
        case_data_update_with,
    ),
    ("invalid_utf8_value_round_trips", case_invalid_utf8),
    ("constants_exist", case_constants),
    ("static_methods", case_static_methods),
    ("schema_migrate", case_schema_migrate),
    ("set_spacing_sizes_notices", case_set_spacing_sizes),
    ("presets_keyed_by_origin", case_presets_keyed),
    ("serialize_round_trips_the_document", case_serialize),
    ("swapped_block_style_is_seen", case_swapped_block_style),
];

fn run(php: &Php, code: &str) -> Result<Value, String> {
    php.eval("case", code).map_err(|e| e.to_string())
}

fn expect_json(php: &Php, code: &str, want: &str) -> Result<(), String> {
    let got = run(
        php,
        &format!("return json_encode((function() {{ {code} }})());"),
    )?;
    let got = match got {
        Value::Str(s) => s,
        other => return Err(format!("{code}\n  got {other:?}")),
    };
    let (g, w): (serde_json::Value, serde_json::Value) = (
        serde_json::from_str(&got).map_err(|e| e.to_string())?,
        serde_json::from_str(want).map_err(|e| e.to_string())?,
    );
    if g == w {
        Ok(())
    } else {
        Err(format!("{code}\n  got  {got}\n  want {want}"))
    }
}

fn case_new_and_get_settings(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return (new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['custom'=>false]]]))->get_settings();",
        r#"{"color":{"custom":false}}"#,
    )
}

fn case_default_constructor(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return (new WP_Theme_JSON())->get_raw_data();",
        r#"{"version":3}"#,
    )
}

fn case_clone(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "$a = new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['custom'=>false]]]); $b = clone $a;
         $b->merge(new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['custom'=>true]]]));
         return [$a->get_settings(), $b->get_settings()];",
        r#"[{"color":{"custom":false}},{"color":{"custom":true}}]"#,
    )
}

fn case_merge_raw(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "$incoming = new class { public function get_raw_data() { return ['version'=>3,'styles'=>['color'=>['text'=>'red']]]; } };
         $a = new WP_Theme_JSON(['version'=>3,'styles'=>['color'=>['background'=>'blue']]]); $a->merge($incoming);
         return $a->get_raw_data();",
        r#"{"version":3,"styles":{"color":{"background":"blue","text":"red"}}}"#,
    )
}

fn case_data_update_with(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "$d = new WP_Theme_JSON_Data(['version'=>3,'settings'=>['color'=>['custom'=>false]]], 'theme');
         $same = $d->update_with(['version'=>3,'settings'=>['color'=>['text'=>true]]]) === $d;
         return [$same, $d->get_data(), $d->get_theme_json() instanceof WP_Theme_JSON];",
        r#"[true,{"version":3,"settings":{"color":{"custom":false,"text":true}}},true]"#,
    )
}

fn case_invalid_utf8(php: &Php) -> Result<(), String> {
    let got = run(
        php,
        "$d = new WP_Theme_JSON_Data(['version'=>3]); $d->update_with(['version'=>3,'settings'=>['custom'=>['x'=>\"\\xff\\xfe\"]]]);
         return bin2hex($d->get_data()['settings']['custom']['x']);",
    )?;
    if got == Value::Str("fffe".into()) {
        Ok(())
    } else {
        Err(format!("got {got:?}"))
    }
}

fn case_constants(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return [WP_Theme_JSON::LATEST_SCHEMA, WP_Theme_JSON::ROOT_BLOCK_SELECTOR, WP_Theme_JSON::VALID_ORIGINS,
                 WP_Theme_JSON::PRESETS_METADATA[4]['value_func'], count(WP_Theme_JSON::ELEMENTS),
                 WP_Theme_JSON_Schema::V1_TO_V2_RENAMED_PATHS['border.customRadius']];",
        r#"[3,"body",["default","blocks","theme","custom"],"wp_get_typography_font_size_value",13,"border.radius"]"#,
    )
}

fn case_static_methods(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return [WP_Theme_JSON::get_element_class_name('button'), WP_Theme_JSON::get_element_class_name('x'),
                 WP_Theme_JSON::get_from_editor_settings(['disableCustomColors'=>true]),
                 WP_Theme_JSON::get_viewport_media_queries(null, ['include_desktop'=>true])];",
        r#"["wp-element-button","",{"version":3,"settings":{"color":{"custom":false}}},{"@mobile":"@media (width <= 480px)","@tablet":"@media (480px < width <= 782px)","@desktop":"@media (width > 782px)"}]"#,
    )
}

fn case_schema_migrate(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return WP_Theme_JSON_Schema::migrate(['version'=>1,'settings'=>['border'=>['customRadius'=>true]]]);",
        r#"{"version":3,"settings":{"border":{"radius":true}}}"#,
    )
}

fn case_set_spacing_sizes(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "$GLOBALS['notices'] = []; $t = new WP_Theme_JSON(['version'=>3,'settings'=>['spacing'=>['spacingScale'=>['steps'=>3]]]]);
         $t->set_spacing_sizes(); return $GLOBALS['notices'];",
        r#"["deprecated_function:WP_Theme_JSON::set_spacing_sizes|6.6.0|","trigger_error:WP_Theme_JSON::set_spacing_sizes|Some of the theme.json settings.spacing.spacingScale values are invalid"]"#,
    )
}

fn case_presets_keyed(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "return (new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['palette'=>[['slug'=>'a','color'=>'#000']]]]], 'custom'))->get_settings();",
        r##"{"color":{"palette":{"custom":[{"slug":"a","color":"#000"}]}}}"##,
    )
}

/// A serialized document carries its data, not its handle: unserializing after the slot is reused yields the
/// original data, and destroying the copy leaves the other object alone.
fn case_serialize(php: &Php) -> Result<(), String> {
    expect_json(
        php,
        "$s = serialize(new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['custom'=>false]]]));
         $b = new WP_Theme_JSON(['version'=>3,'settings'=>['color'=>['text'=>true]]]);
         $x = unserialize($s); $copy = $x->get_settings(); unset($x);
         return [$copy, $b->get_settings(), str_contains($s, 'theme_json')];",
        r#"[{"color":{"custom":false}},{"color":{"text":true}},true]"#,
    )
}

/// Minimal block registries, enough for the adapter to read names and styles.
const REGISTRIES: &str = r#"
class WP_Block_Type_Registry { public static $i; public $blocks = []; public static function get_instance() { return self::$i ??= new self(); }
    public function get_all_registered() { return $this->blocks; } }
class WP_Block_Styles_Registry { public static $i; public $styles = []; public static function get_instance() { return self::$i ??= new self(); }
    public function get_all_registered() { return $this->styles; }
    public function get_registered_styles_for_block($name) { return $this->styles[$name] ?? []; } }
"#;

/// Swapping one block style for another (same counts) is seen by the next document, as WordPress sees it.
fn case_swapped_block_style(php: &Php) -> Result<(), String> {
    run(php, REGISTRIES)?;
    expect_json(
        php,
        "WP_Block_Type_Registry::get_instance()->blocks = ['core/button' => (object) ['styles' => [], 'selectors' => [], 'supports' => []]];
         WP_Block_Styles_Registry::get_instance()->styles = ['core/button' => ['outline' => ['name' => 'outline']]];
         new WP_Theme_JSON(['version'=>3]);
         WP_Block_Styles_Registry::get_instance()->styles = ['core/button' => ['fancy' => ['name' => 'fancy']]];
         $t = new WP_Theme_JSON(['version'=>3,'styles'=>['blocks'=>['core/button'=>['variations'=>['fancy'=>['color'=>['text'=>'red']]]]]]]);
         return $t->get_raw_data();",
        r#"{"version":3,"styles":{"blocks":{"core/button":{"variations":{"fancy":{"color":{"text":"red"}}}}}}}"#,
    )
}

fn main() -> ExitCode {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let theme_json = ThemeJsonArea::new(container.clone());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(theme_json)];
    let php = match Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    }) {
        Ok(php) => php,
        Err(err) => {
            println!("FAILED: boot: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = php.eval("wordpress", WORDPRESS) {
        println!("FAILED: WordPress stubs: {err}");
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for (name, case) in CASES {
        match case(&php) {
            Ok(()) => println!("ok: {name}"),
            Err(why) => {
                failed += 1;
                println!("FAILED: {name}: {why}");
            }
        }
    }
    drop(php);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
