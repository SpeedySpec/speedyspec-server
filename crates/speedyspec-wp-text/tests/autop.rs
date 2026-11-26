//! `wpautop`, `shortcode_unautop` and `wp_replace_in_html_tags`, checked against the corpus.

mod common;

use speedyspec_wp_shortcode_contracts::TagName;
use speedyspec_wp_text::{AutopOptions, DEFAULT_SPACES, shortcode_unautop, wp_replace_in_html_tags, wpautop_reference};
use speedyspec_wp_text_corpus::{Php, assert_corpus};

/// The shortcodes the corpus was recorded with, in WordPress's registration order.
fn tags() -> Vec<TagName> {
    ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect()
}

#[test]
fn wp_replace_in_html_tags_matches_wordpress() {
    assert_corpus("wp_replace_in_html_tags", |args| {
        let Php::Array(pairs) = &args[1] else { panic!("pairs") };
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = pairs
            .iter()
            .map(|(k, v)| {
                let key = match k {
                    speedyspec_wp_text_corpus::PhpKey::Str(s) => s.clone(),
                    speedyspec_wp_text_corpus::PhpKey::Int(i) => i.to_string().into_bytes(),
                };
                (key, common::text(v).to_vec())
            })
            .collect();
        let borrowed: Vec<(&[u8], &[u8])> = pairs.iter().map(|(a, b)| (&a[..], &b[..])).collect();
        Php::Str(wp_replace_in_html_tags(common::text(&args[0]), &borrowed).into_owned())
    });
}

#[test]
fn wpautop_matches_wordpress() {
    assert_corpus("wpautop", |args| {
        let br = args.get(1).is_none_or(|b| !common::php_empty(b));
        Php::Str(wpautop_reference(common::text(&args[0]), AutopOptions { br }).into_owned())
    });
}

#[test]
fn shortcode_unautop_matches_wordpress() {
    let tags = tags();
    assert_corpus("shortcode_unautop", |args| {
        Php::Str(shortcode_unautop(common::text(&args[0]), &tags, DEFAULT_SPACES).into_owned())
    });
}

#[test]
fn unautop_quotes_tag_names() {
    let tags: Vec<TagName> = ["a.b", "a+b"].iter().map(|t| TagName::new(t.as_bytes()).unwrap()).collect();
    assert_eq!(
        &*shortcode_unautop(b"<p>[a.b]</p><p>[axb]</p><p>[a+b]</p>", &tags, DEFAULT_SPACES),
        b"[a.b]<p>[axb]</p>[a+b]"
    );
}

#[test]
fn unautop_without_tags_is_identity() {
    assert!(matches!(shortcode_unautop(b"<p>[gallery]</p>", &[], DEFAULT_SPACES), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn unautop_cache_matches_the_free_function() {
    let tags = tags();
    let unautop = speedyspec_wp_text::Unautop::new();
    for text in [
        &b"<p>[gallery]</p>"[..],
        b"<p> \xC2\xA0&nbsp;[caption]x[/caption]&nbsp; </p>",
        b"a [gallery] <p>x</p>",
        b"<p>x[gallery]</p>",
        b"<p>&nbsp;\n[video /]\t</p><p>[audio]</p>",
    ] {
        assert_eq!(unautop.run(text, &tags, 1, DEFAULT_SPACES), shortcode_unautop(text, &tags, DEFAULT_SPACES));
    }
}
