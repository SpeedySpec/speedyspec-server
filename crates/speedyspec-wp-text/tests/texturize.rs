//! `wptexturize` on PCRE2, checked against the corpus.

mod common;

use speedyspec_wp_shortcode_contracts::TagName;
use speedyspec_wp_text::{DEFAULT_SPACES, PcreTexturizer, TexturizeScope, TexturizeTables, Texturizer};
use speedyspec_wp_text_corpus::{Php, assert_corpus};

fn tags() -> Vec<TagName> {
    ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect()
}

fn list(items: &[&str]) -> Vec<Vec<u8>> {
    items.iter().map(|s| s.as_bytes().to_vec()).collect()
}

fn english() -> TexturizeTables {
    TexturizeTables::build(&|text: &str, _: &str| text.to_string(), None, DEFAULT_SPACES).unwrap()
}

/// wpTexturize.php's filter_translate().
fn translate_1(text: &str, context: &str) -> String {
    match text {
        "&#8211;" => "!endash!",
        "&#8212;" => "!emdash!",
        "&#8216;" => "!openq1!",
        "&#8217;" if context == "apostrophe" => "!apos!",
        "&#8217;" => "!closeq1!",
        "&#8220;" => "!openq2!",
        "&#8221;" => "!closeq2!",
        "&#8242;" => "!prime1!",
        "&#8243;" => "!prime2!",
        "&#8217;tain&#8217;t,&#8217;twere,&#8217;twas,&#8217;tis,&#8217;twill,&#8217;til,&#8217;bout,&#8217;nuff,&#8217;round,&#8217;cause,&#8217;em" => {
            "!apos!tain!apos!t,!apos!twere,!apos!twas,!apos!tis,!apos!twill,!apos!til,!apos!bout,!apos!nuff,!apos!round,!apos!cause,!apos!em"
        }
        other => other,
    }
    .to_string()
}

/// wpTexturize.php's filter_translate2().
fn translate_2(text: &str, context: &str) -> String {
    match text {
        "&#8211;" => "!endash!",
        "&#8212;" => "!emdash!",
        "&#8216;" => "!q1!",
        "&#8217;" if context == "apostrophe" => "!apos!",
        "&#8217;" => "!q1!",
        "&#8220;" | "&#8221;" => "!q2!",
        "&#8242;" => "!prime1!",
        "&#8243;" => "!prime2!",
        other => other,
    }
    .to_string()
}

fn run(tables: &TexturizeTables, text: &[u8], shortcodes: &[&str], tags: &[TagName]) -> Php {
    let no_tags = list(&["pre", "code", "kbd", "style", "script", "tt"]);
    let no_shortcodes = list(shortcodes);
    let scope = TexturizeScope { no_texturize_tags: &no_tags, no_texturize_shortcodes: &no_shortcodes, shortcode_tags: tags };
    Php::Str(PcreTexturizer::default().texturize(text, tables, &scope).unwrap().into_owned())
}

#[test]
fn wptexturize_matches_wordpress() {
    let (tables, tags) = (english(), tags());
    assert_corpus("wptexturize", |args| run(&tables, common::text(&args[0]), &["code"], &tags));
}

#[test]
fn translated_wptexturize_matches_wordpress() {
    let one = TexturizeTables::build(&translate_1, None, DEFAULT_SPACES).unwrap();
    let two = TexturizeTables::build(&translate_2, None, DEFAULT_SPACES).unwrap();
    let tags = tags();
    assert_corpus("speedyspec_wptexturize_translated", |args| {
        let tables = if common::php_int(&args[1]) == 1 { &one } else { &two };
        run(tables, common::text(&args[0]), &["code"], &tags)
    });
}

#[test]
fn no_texturize_shortcodes_filter_matches_wordpress() {
    let (tables, tags) = (english(), tags());
    assert_corpus("speedyspec_wptexturize_no_texturize_shortcodes", |args| {
        run(&tables, common::text(&args[0]), &["code", "audio"], &tags)
    });
}

#[test]
fn cockney_override_replaces_the_list() {
    let tables = TexturizeTables::build(
        &|text: &str, _: &str| text.to_string(),
        Some(vec![(b"'tain't".to_vec(), b"X".to_vec())]),
        DEFAULT_SPACES,
    )
    .unwrap();
    assert_eq!(run(&tables, b"'tain't 'twas", &["code"], &[]), Php::Str(b"X &#8216;twas".to_vec()));
}

#[test]
fn translation_equal_to_ascii_drops_patterns() {
    let tables = TexturizeTables::build(
        &|text: &str, context: &str| if context == "apostrophe" { "'".into() } else { text.into() },
        None,
        DEFAULT_SPACES,
    )
    .unwrap();
    assert_eq!(run(&tables, b"don't", &["code"], &[]), Php::Str(b"don't".to_vec()));
}
