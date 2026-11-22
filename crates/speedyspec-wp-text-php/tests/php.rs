//! The text ports over PHP, and the native formatting functions, from the embedded PHP. PHP boots once per process
//! (`harness = false`); each group of cases runs in a fresh request.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::KsesArea;
use speedyspec_wp_shortcode_contracts::ShortcodeTagNames;
use speedyspec_wp_text_contracts::{
    Diagnostics, LinkContext, Locale, MimeTypes, RandomSource, SiteSettings, Smilies, TexturizeOverrides, Translator,
    WordCountType,
};
use speedyspec_wp_text_php::TextArea;
use speedyspec_wp_text_php::adapters::*;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

/// The WordPress functions the adapters call, as a test site answers them.
const STUBS: &str = r#"
function get_option($name, $default = false) {
    return ['blog_charset' => 'ISO-8859-1', 'use_smilies' => '0', 'use_balanceTags' => '1'][$name] ?? $default;
}
function wp_get_word_count_type() { return 'characters_excluding_spaces'; }
function __($text, $domain = 'default') { return apply_filters('gettext', $text, $text, $domain); }
function _x($text, $context, $domain = 'default') { return apply_filters('gettext_with_context', $text, $text, $context, $domain); }
function get_locale() { return 'de_DE'; }
function wp_get_mime_types() { return apply_filters('mime_types', ['jpg|jpeg' => 'image/jpeg', 'txt' => 'text/plain']); }
function get_allowed_mime_types() { return ['txt' => 'text/plain']; }
function wp_is_internal_link($url) { return str_contains($url, 'example.org'); }
function includes_url($path = '') { return 'http://example.org/wp-includes/' . $path; }
function site_url() { return 'http://example.org'; }
function _deprecated_function($f, $v, $r = '') { $GLOBALS['seen'][] = "$f $v $r"; }
function _deprecated_argument($f, $v, $m = '') { $GLOBALS['seen'][] = "arg $f $v $m"; }
function wp_trigger_error($f, $m, $l = E_USER_NOTICE) { $GLOBALS['seen'][] = "error $f $m $l"; }
$GLOBALS['wpsmiliestrans'] = [':)' => 'smile.png'];
$GLOBALS['wp_smiliessearch'] = '/:\)/';
$GLOBALS['shortcode_tags'] = ['gallery' => '__return_empty_string', 'caption' => '__return_empty_string'];
$GLOBALS['wp_cockneyreplace'] = ["'tain't" => '&#8217;tain&#8217;t'];
return true;
"#;

fn main() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let text = TextArea::new(container.clone(), hooks.api(), kses.api());
    let api = text.api();
    let areas: Vec<Rc<dyn NativeArea>> =
        vec![twins_area(), Rc::new(ContainerArea::new("tests", container.clone())), Rc::new(hooks), Rc::new(kses), Rc::new(text)];
    let php = Php::boot(BootOptions { hooks: Rc::new(MemoryHooks::new()), preload: &[], areas }).expect("PHP boots");

    php.restart_request().expect("a request");
    check("stubs load", matches!(eval(STUBS), Ok(Value::Bool(true))), ());

    let settings = PhpSiteSettings;
    check("blog_charset", settings.blog_charset().ok().as_deref() == Some("ISO-8859-1"), settings.blog_charset());
    check("use_smilies", settings.use_smilies().ok() == Some(false), ());
    check("use_balance_tags", settings.use_balance_tags().ok() == Some(true), ());
    check("word_count_type", settings.word_count_type().ok() == Some(WordCountType::CharactersExcludingSpaces), ());
    check("locale", PhpLocale.locale().ok().as_deref() == Some("de_DE"), ());
    let mimes = PhpMimeTypes::uncached().mime_types().unwrap();
    check("mime_types", mimes.first().map(|(e, t)| (e.clone(), t.clone())) == Some((b"jpg|jpeg".to_vec(), b"image/jpeg".to_vec())), &mimes);
    check("allowed_mime_types", PhpMimeTypes::uncached().allowed_mime_types().unwrap().len() == 1, ());
    eval("mt_srand(1); return true;").unwrap();
    let draws: Vec<i64> = (0..5).map(|_| PhpRandom.rand(0, 2).unwrap()).collect();
    check("rand follows mt_srand", draws == vec![1, 2, 0, 2, 1], &draws);
    let smilies = PhpSmilies;
    check("smilies search", smilies.search().unwrap() == Some(b"/:\\)/".to_vec()), smilies.search());
    check("smilies keys", smilies.smileys().unwrap() == vec![b":)".to_vec()], ());
    check("smiley image", smilies.image(b":)").unwrap() == Some(b"smile.png".to_vec()), ());
    check("includes_url", smilies.includes_url(b"x").unwrap() == b"http://example.org/wp-includes/x", ());
    check("internal link", PhpLinkContext.is_internal_link(b"http://example.org/a").unwrap(), ());
    let tags = PhpShortcodeTags;
    let names: Vec<Vec<u8>> = tags.tag_names().iter().map(|t| t.as_bytes().to_vec()).collect();
    check("shortcode names", names == vec![b"gallery".to_vec(), b"caption".to_vec()], &names);
    let before = tags.generation();
    eval("$GLOBALS['shortcode_tags']['audio'] = 'x'; return true;").unwrap();
    check("shortcode generation changes", tags.generation() != before, ());
    let cockney = PhpTexturizeOverrides.cockney().unwrap();
    check("cockney", cockney.as_ref().map(|p| p.len()) == Some(1), &cockney);
    PhpDiagnostics.deprecated_function("f", "1.0", "g").unwrap();
    check("diagnostics", matches!(eval("return $GLOBALS['seen'][0] === 'f 1.0 g';"), Ok(Value::Bool(true))), ());

    eval("add_filter('gettext', function ($t) { return 'T:' . $t; }); return true;").unwrap();
    check("gettext reaches the translator", PhpTranslator.translate("a", "default").ok().as_deref() == Some("T:a"), ());

    eval("function wp_is_internal_link($u) { throw new RuntimeException('nope'); }").ok();
    php.restart_request().expect("a request");
    eval("function get_locale() { throw new RuntimeException('nope'); } return true;").unwrap();
    let error = PhpLocale.locale().unwrap_err();
    check("adapter exception keeps payload", error.payload.as_ref().is_some_and(|p| p.downcast_ref::<Value>().is_some()), &error);
    println!("all text-php adapter checks passed");

    natives(&php, &api);
    println!("all text-php native checks passed");
}

/// A fresh request with the stubs loaded.
fn request(php: &Php) {
    php.restart_request().expect("a request");
    check("stubs load", matches!(eval(STUBS), Ok(Value::Bool(true))), ());
    eval("function get_option($name, $default = false) { return ['blog_charset' => 'UTF-8', 'use_smilies' => '1'][$name] ?? $default; } return true;")
        .ok();
}

fn yes(name: &str, code: &str) {
    let r = eval(code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

fn natives(php: &Php, api: &std::rc::Rc<speedyspec_wp_text::TextApi>) {
    php.restart_request().expect("a request");
    let names = speedyspec_wp_text_php::native_names();
    check("natives listed", names.len() >= 75, names.len());
    for name in &names {
        let exists = eval(&format!("return function_exists('{}');", name.replace('\\', "\\\\")));
        check(&format!("native {name} exists"), matches!(exists, Ok(Value::Bool(true))), exists);
    }

    request(php);
    let cases: [(&str, &[u8]); 6] = [
        ("return wptexturize(\"'a' -- b\");", "&#8216;a&#8217; &#8212; b".as_bytes()),
        ("return wpautop(\"a\\n\\nb\");", b"<p>a</p>\n<p>b</p>\n"),
        ("return esc_html('<a href=\"x\">');", b"&lt;a href=&quot;x&quot;&gt;"),
        ("return esc_url('http://a.b/?c&d');", b"http://a.b/?c&#038;d"),
        ("return sanitize_title('Hello World');", b"hello-world"),
        ("return sanitize_file_name('a b.txt');", b"a-b.txt"),
    ];
    eval("add_filter('sanitize_title', 'sanitize_title_with_dashes', 10, 3); return true;").unwrap();
    for (code, expected) in cases {
        let got = eval(code);
        let bytes = match &got {
            Ok(Value::Str(s)) => s.as_bytes().to_vec(),
            Ok(Value::Bytes(b)) => b.clone(),
            _ => Vec::new(),
        };
        check(code, bytes == expected, (&got, String::from_utf8_lossy(expected)));
    }
    let texturized = api.wptexturize(&Value::Str("\"x\"".into()), false).unwrap();
    check("native equals rust", eval("return wptexturize('\"x\"');").ok() == Some(texturized), ());

    request(php);
    yes(
        "argument_count_error",
        r#"try { esc_html(); return false; } catch (ArgumentCountError $e) {
            return $e->getMessage() === 'Too few arguments to function esc_html(), 0 passed and exactly 1 expected'; }"#,
    );
    yes(
        "reentrant_filter",
        r#"add_filter('attribute_escape', function ($safe) {
            static $inside = false;
            if ($inside) { return $safe; }
            $inside = true; $inner = esc_attr('<'); $inside = false;
            return $inner . $safe;
        });
        $first = esc_attr('"');
        add_filter('the_content', function ($t) { return wptexturize($t); });
        add_filter('run_wptexturize', function ($run) {
            static $inside = false;
            if ($inside) { return $run; }
            $inside = true; $GLOBALS['inner'] = apply_filters('the_content', "'x'"); $inside = false;
            return $run;
        });
        $second = wptexturize("'y'", true);
        return $first === '&lt;&quot;' && $second === '&#8216;y&#8217;' && $GLOBALS['inner'] === '&#8216;x&#8217;';"#,
    );
    request(php);
    yes(
        "exception_in_callback_rethrows",
        r#"$thrown = new LogicException('no');
        add_filter('clean_url', function () use ($thrown) { throw $thrown; });
        try { esc_url('http://a.b'); return false; } catch (LogicException $e) { return spl_object_id($e) === spl_object_id($thrown); }"#,
    );
    request(php);
    let exited = eval("add_filter('clean_url', function () { exit(); }); esc_url('http://a.b'); return 'kept going';");
    check("exit_in_callback_unwinds", !matches!(&exited, Ok(Value::Str(s)) if s == "kept going"), &exited);
    request(php);
    yes("next request works after exit", "return esc_html('<') === '&lt;';");

    request(php);
    yes(
        "run_wptexturize_false_sticks",
        r#"add_filter('run_wptexturize', '__return_false');
        function __return_false() { return false; }
        $a = wptexturize("'a'");
        remove_all_filters('run_wptexturize');
        return $a === "'a'" && wptexturize("'b'") === "'b'";"#,
    );
    request(php);
    yes("run_wptexturize_is_per_request", "return wptexturize(\"'b'\") === '&#8216;b&#8217;';");

    request(php);
    yes(
        "wp_parse_str_by_reference",
        r#"wp_parse_str('a=1&b[]=2', $out); return $out === ['a' => '1', 'b' => ['2']];"#,
    );
    yes(
        "wp_unslash_walks_objects",
        r#"// WordPress's own, which stay PHP.
        function map_deep($value, $callback) {
            if (is_array($value)) { foreach ($value as $index => $item) { $value[$index] = map_deep($item, $callback); } }
            elseif (is_object($value)) { foreach (get_object_vars($value) as $n => $v) { $value->$n = map_deep($v, $callback); } }
            else { $value = call_user_func($callback, $value); }
            return $value;
        }
        function stripslashes_from_strings_only($value) { return is_string($value) ? stripslashes($value) : $value; }
        function stripslashes_deep($value) { return map_deep($value, 'stripslashes_from_strings_only'); }
        $o = new stdClass(); $o->x = "a\\'b"; $r = wp_unslash(['k' => $o]); return $r['k']->x === "a'b";"#,
    );
    yes(
        "stringable_objects_are_cast",
        r#"$o = new class { public function __toString(): string { return 'foo<bar'; } };
        wp_parse_str($o, $out);
        return esc_html($o) === 'foo&lt;bar' && $out === ['foo<bar' => ''];"#,
    );
    yes(
        "unstringable_object_throws",
        r#"try { esc_html(new stdClass()); return false; } catch (Error $e) { return str_contains($e->getMessage(), 'could not be converted to string'); }"#,
    );
    yes(
        "mime_types_filter_is_seen_on_every_call",
        r#"$before = sanitize_file_name('zzz');
        add_filter('mime_types', function ($m) { $m['zzz'] = 'x/y'; return $m; });
        $during = sanitize_file_name('zzz');
        remove_all_filters('mime_types');
        $after = sanitize_file_name('zzz');
        return $before === 'zzz' && $during === 'unnamed-file.zzz' && $after === 'zzz';"#,
    );
    yes("seems_utf8_is_bool", "return seems_utf8('a') === true;");
    yes("sanitize_hex_color_null", "return sanitize_hex_color('nope') === null && sanitize_hex_color('') === '';");
    yes("is_email_false", "return is_email('x') === false;");
    yes("wp_html_split_array", "return wp_html_split('a<b>c') === ['a', '<b>', 'c'];");
    yes("translate_smiley_array_arg", "return translate_smiley([':)']) !== '';");
    yes("wp_rel_callback_array_arg", r#"return wp_rel_callback(['<a href="/x">', 'href="/x"'], 'nofollow') === '<a href="/x" rel="nofollow">';"#);
}
