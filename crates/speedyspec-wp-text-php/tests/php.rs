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
function wp_get_mime_types() { return ['jpg|jpeg' => 'image/jpeg', 'txt' => 'text/plain']; }
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
    let mimes = PhpMimeTypes.mime_types().unwrap();
    check("mime_types", mimes.first().map(|(e, t)| (e.clone(), t.clone())) == Some((b"jpg|jpeg".to_vec(), b"image/jpeg".to_vec())), &mimes);
    check("allowed_mime_types", PhpMimeTypes.allowed_mime_types().unwrap().len() == 1, ());
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
}
