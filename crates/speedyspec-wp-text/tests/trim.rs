//! `wp_trim_words` and `wp_html_excerpt`, checked against the corpus.

mod common;

use speedyspec_wp_text::{wp_html_excerpt, wp_trim_words_core};
use speedyspec_wp_text_corpus::{Php, assert_corpus};

fn trim(args: &[Php], count_characters: bool) -> Php {
    let num_words = args.get(1).map_or(55, common::php_int);
    let more = match args.get(2) {
        None | Some(Php::Null) => b"&hellip;".to_vec(),
        Some(more) => common::text(more).to_vec(),
    };
    Php::Str(wp_trim_words_core(common::text(&args[0]), num_words, &more, count_characters))
}

#[test]
fn wp_trim_words_matches_wordpress() {
    assert_corpus("wp_trim_words", |args| trim(args, false));
}

#[test]
fn wp_trim_words_by_characters_matches_wordpress() {
    assert_corpus("speedyspec_wp_trim_words_characters", |args| trim(args, true));
}

#[test]
fn character_mode_counts_codepoints() {
    assert_eq!(wp_trim_words_core("héllo wörld".as_bytes(), 3, "…".as_bytes(), true), "hél…".as_bytes());
}

#[test]
fn character_mode_invalid_utf8() {
    // PHP's /./u match fails on invalid UTF-8, leaving no characters: the recorded result is "".
    assert_eq!(wp_trim_words_core(b"ab\xC0cd ef", 3, b"&hellip;", true), b"");
}

#[test]
fn wp_html_excerpt_matches_wordpress() {
    assert_corpus("wp_html_excerpt", |args| {
        let more = args.get(2).map_or(Vec::new(), |m| common::text(m).to_vec());
        Php::Str(wp_html_excerpt(common::text(&args[0]), common::php_int(&args[1]), &more))
    });
}
