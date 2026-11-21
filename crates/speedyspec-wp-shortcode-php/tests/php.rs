//! The shortcode functions in PHP, native over the Rust Shortcode API: the global is the registry, PHP and Rust
//! handlers both run, PHP filters apply, and PHP sees WordPress's notices and exceptions.
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
use speedyspec_wp_shortcode::ShortcodeApi;
use speedyspec_wp_shortcode_contracts::{AttKey, ParsedAtts, PhpValue, ShortcodeError, ShortcodeHandler};
use speedyspec_wp_shortcode_php::ShortcodeArea;

/// WordPress functions the adapters call, as WordPress defines them (translation is the identity here).
const WORDPRESS: &str = r#"
function __($text, $domain = 'default') { return $text; }
function _doing_it_wrong($function_name, $message, $version) { $GLOBALS['notices'][] = "$function_name|$message|$version"; }
function wp_html_split($input) { return preg_split('/(<[^>]*>)/', $input, -1, PREG_SPLIT_DELIM_CAPTURE); }
function wp_kses_attr_parse($element) { return false; }
function wp_kses_one_attr($attr, $element) { return $attr; }
"#;

type Case = fn(&Php) -> Result<(), String>;

const CASES: &[(&str, Case)] = &[
    ("functions_declared", case_functions_declared),
    ("global_is_registry", case_global_is_registry),
    ("php_handler_gets_atts_content_and_tag", case_php_handler),
    ("embed_swap", case_embed_swap),
    ("rust_handler_name", case_rust_handler_name),
    ("php_filters_apply", case_php_filters_apply),
    ("direct_do_shortcode_tag_unconverted", case_direct_do_shortcode_tag_unconverted),
    ("invalid_utf8_deprecation", case_invalid_utf8_deprecation),
    ("doing_it_wrong_reaches_php", case_doing_it_wrong),
    ("handler_exception_propagates", case_handler_exception),
    ("array_result_converts_like_php", case_array_result),
    ("recursive_array_result_converts_like_php", case_unconvertible_result),
    ("throwing_error_handler_escapes", case_throwing_error_handler),
    ("html_tags_with_empty_tagnames_uses_registry", case_html_tags_empty_tagnames),
];

fn run(php: &Php, code: &str) -> Result<Value, String> {
    php.eval("case", code).map_err(|e| e.to_string())
}

fn expect_php(php: &Php, code: &str, want: Value) -> Result<(), String> {
    let got = run(php, code)?;
    if got == want { Ok(()) } else { Err(format!("{code}\n  got  {got:?}\n  want {want:?}")) }
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn reset(php: &Php) -> Result<(), String> {
    run(php, "remove_all_shortcodes(); $GLOBALS['notices'] = []; return null;").map(|_| ())
}

fn case_functions_declared(php: &Php) -> Result<(), String> {
    let signatures = [
        ("add_shortcode", "tag,callback"),
        ("remove_shortcode", "tag"),
        ("remove_all_shortcodes", ""),
        ("shortcode_exists", "tag"),
        ("has_shortcode", "content,tag"),
        ("get_shortcode_tags_in_content", "content"),
        ("apply_shortcodes", "content,ignore_html"),
        ("do_shortcode", "content,ignore_html"),
        ("_filter_do_shortcode_context", ""),
        ("get_shortcode_regex", "tagnames"),
        ("do_shortcode_tag", "m"),
        ("do_shortcodes_in_html_tags", "content,ignore_html,tagnames"),
        ("unescape_invalid_shortcodes", "content"),
        ("get_shortcode_atts_regex", ""),
        ("shortcode_parse_atts", "text"),
        ("shortcode_atts", "pairs,atts,shortcode"),
        ("strip_shortcodes", "content"),
        ("strip_shortcode_tag", "m"),
    ];
    for (name, params) in signatures {
        let code = format!(
            "return implode(',', array_map(fn($p) => $p->getName(), (new ReflectionFunction('{name}'))->getParameters()));"
        );
        expect_php(php, &code, s(params))?;
    }
    Ok(())
}

fn case_global_is_registry(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(php, "add_shortcode('x', 'strtoupper'); return $GLOBALS['shortcode_tags']['x'];", s("strtoupper"))?;
    expect_php(php, "$GLOBALS['shortcode_tags'] = []; return shortcode_exists('x');", Value::Bool(false))?;
    expect_php(php, "$GLOBALS['shortcode_tags']['direct'] = fn() => 'D'; return do_shortcode('[direct]');", s("D"))?;
    expect_php(php, "remove_shortcode('direct'); return array_keys($GLOBALS['shortcode_tags']);", Value::Array(vec![]))
}

fn case_php_handler(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('t', function ($atts, $content, $tag) { return json_encode([$atts, $content, $tag]); });
           return do_shortcode('a [t x="1" 2]in[/t] b');"#,
        s(r#"a [{"x":"1","0":"2"},"in","t"] b"#),
    )
}

fn case_embed_swap(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('a', fn() => 'A'); add_shortcode('embed', fn($atts, $c) => "E($c)");
           do_shortcode('[a]');
           $saved = $GLOBALS['shortcode_tags'];
           $GLOBALS['shortcode_tags'] = ['embed' => $saved['embed']];
           $inside = do_shortcode('[a][embed]u[/embed]');
           $GLOBALS['shortcode_tags'] = $saved;
           return $inside . '|' . do_shortcode('[a][embed]v[/embed]');"#,
        s("[a]E(u)|AE(v)"),
    )
}

struct Caption;

impl ShortcodeHandler for Caption {
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        let width = atts.iter().find(|(k, _)| **k == AttKey::Str(b"width".to_vec())).map(|(_, v)| v.to_vec()).unwrap_or_default();
        Ok(PhpValue::Str(format!(
            "<figure w={}>{}</figure>",
            String::from_utf8_lossy(&width),
            String::from_utf8_lossy(content.unwrap_or(b""))
        )))
    }
}

fn case_rust_handler_name(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('caption', 'img_caption_shortcode');
           return [$GLOBALS['shortcode_tags']['caption'], do_shortcode('[caption width="5"]x[/caption]'), img_caption_shortcode(['width' => '7'], 'y')];"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("img_caption_shortcode")),
            (speedyspec_hooks::Key::Int(1), s("<figure w=5>x</figure>")),
            (speedyspec_hooks::Key::Int(2), s("<figure w=7>y</figure>")),
        ]),
    )
}

fn case_php_filters_apply(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('t', fn() => 'foo');
           $f = fn($out) => str_replace('oo', 'ee', $out);
           add_filter('do_shortcode_tag', $f);
           $one = do_shortcode('[t]');
           remove_filter('do_shortcode_tag', $f);
           $ctx = null;
           add_shortcode('c', function () use (&$ctx) { $ctx = apply_filters('wp_get_attachment_image_context', 'x'); return ''; });
           do_shortcode('[c]');
           return [$one, $ctx, has_filter('wp_get_attachment_image_context', '_filter_do_shortcode_context')];"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("fee")),
            (speedyspec_hooks::Key::Int(1), s("do_shortcode")),
            (speedyspec_hooks::Key::Int(2), Value::Bool(false)),
        ]),
    )
}

fn case_direct_do_shortcode_tag_unconverted(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('t', fn() => 'x');
           $f = fn() => [1];
           add_filter('do_shortcode_tag', $f);
           $r = do_shortcode_tag(['[t]', '', 't', '', '', '', '']);
           remove_filter('do_shortcode_tag', $f);
           return $r;"#,
        Value::Array(vec![(speedyspec_hooks::Key::Int(0), Value::Int(1))]),
    )
}

fn case_invalid_utf8_deprecation(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"$seen = [];
           set_error_handler(function ($no, $msg) use (&$seen) { $seen[] = $msg; return true; }, E_DEPRECATED);
           $atts = shortcode_parse_atts("\xff a=1");
           restore_error_handler();
           return [$atts, $seen];"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), Value::Array(vec![])),
            (
                speedyspec_hooks::Key::Int(1),
                Value::Array(vec![(
                    speedyspec_hooks::Key::Int(0),
                    s("preg_match_all(): Passing null to parameter #2 ($subject) of type string is deprecated"),
                )]),
            ),
        ]),
    )
}

fn case_doing_it_wrong(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('bad tag', 'f'); $GLOBALS['shortcode_tags']['gone'] = 'no_such_function';
           $out = do_shortcode('[gone]');
           return [$out, $GLOBALS['notices']];"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("[gone]")),
            (
                speedyspec_hooks::Key::Int(1),
                Value::Array(vec![
                    (speedyspec_hooks::Key::Int(0), s("add_shortcode|Invalid shortcode name: bad tag. Do not use spaces or reserved characters: & / < > [ ] =|4.4.0")),
                    (speedyspec_hooks::Key::Int(1), s("do_shortcode_tag|Attempting to parse a shortcode without a valid callback: gone|4.3.0")),
                ]),
            ),
        ]),
    )
}

fn case_handler_exception(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"class ShortcodeBoom extends Exception {}
           add_shortcode('boom', function () { throw new ShortcodeBoom('bang'); });
           try { do_shortcode('[boom]'); return 'no'; } catch (ShortcodeBoom $e) { return get_class($e) . ':' . $e->getMessage(); }"#,
        s("ShortcodeBoom:bang"),
    )
}

fn case_array_result(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('arr', fn() => [1]);
           $warned = '';
           set_error_handler(function ($no, $msg) use (&$warned) { $warned = $msg; return true; }, E_WARNING);
           $out = do_shortcode('<[arr]>');
           restore_error_handler();
           return [$out, $warned];"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("<Array>")),
            (speedyspec_hooks::Key::Int(1), s("Array to string conversion")),
        ]),
    )
}

fn case_unconvertible_result(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('rec', function () { $a = []; $a[] = &$a; return $a; });
           set_error_handler(fn() => true, E_WARNING);
           try { $r = do_shortcode('x[rec]y'); } finally { restore_error_handler(); }
           return $r;"#,
        s("xArrayy"),
    )
}

fn case_throwing_error_handler(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(
        php,
        r#"add_shortcode('t', fn() => 'T');
           set_error_handler(function ($no, $msg) { throw new ErrorException($msg, 0, $no); }, E_DEPRECATED | E_WARNING);
           $seen = [];
           try { do_shortcode("[t \xff a=1]"); $seen[] = 'rendered'; } catch (ErrorException $e) { $seen[] = $e->getMessage(); }
           try { do_shortcode_tag(['[t]', '', 't', '']); $seen[] = 'rendered'; } catch (ErrorException $e) { $seen[] = $e->getMessage(); }
           restore_error_handler();
           return $seen;"#,
        Value::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("preg_match_all(): Passing null to parameter #2 ($subject) of type string is deprecated")),
            (speedyspec_hooks::Key::Int(1), s("Undefined array key 6")),
        ]),
    )
}

fn case_html_tags_empty_tagnames(php: &Php) -> Result<(), String> {
    reset(php)?;
    expect_php(php, "add_shortcode('q', fn() => 'Q'); return do_shortcodes_in_html_tags('<[q]>', false, []);", s("<Q>"))
}

fn main() -> ExitCode {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let shortcodes = ShortcodeArea::new(container.clone(), hooks.api());
    shortcodes.register_rust_handler("img_caption_shortcode", Rc::new(Caption));
    let _api: Rc<ShortcodeApi> = shortcodes.api();
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(shortcodes)];
    let php = match Php::boot(BootOptions { hooks: Rc::new(MemoryHooks::new()), preload: &[], areas }) {
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
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
