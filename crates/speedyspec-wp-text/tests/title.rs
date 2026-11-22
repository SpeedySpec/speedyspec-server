//! `remove_accents`, `sanitize_title_with_dashes`, `sanitize_user` and `sanitize_file_name`'s cores, against the
//! recorded corpus (the recorder's locale is `en_US`, and no filters are registered).

mod common;

use common::text;
use speedyspec_wp_text::file_name::{MimeTable, default_special_chars, sanitize_file_name_core, sanitize_file_name_prepare};
use speedyspec_wp_text::title::{SanitizeContext, remove_accents, sanitize_title_with_dashes, sanitize_user_core};
use speedyspec_wp_text::wp_strip_all_tags_core;
use speedyspec_wp_text_corpus::{Php, assert_corpus};

#[test]
fn remove_accents_corpus() {
    assert_corpus("remove_accents", |args| {
        let locale = args.get(1).map_or("", |l| std::str::from_utf8(text(l)).expect("a locale"));
        let locale = if locale.is_empty() { "en_US" } else { locale };
        Php::Str(remove_accents(text(&args[0]), locale).into_owned())
    });
}

#[test]
fn sanitize_title_with_dashes_corpus() {
    assert_corpus("sanitize_title_with_dashes", |args| {
        let context = SanitizeContext::from_bytes(args.get(2).map_or(&b"display"[..], text));
        Php::Str(sanitize_title_with_dashes(text(&args[0]), &context))
    });
}

#[test]
fn sanitize_user_corpus() {
    assert_corpus("sanitize_user", |args| {
        let strict = matches!(args.get(1), Some(Php::Bool(true)));
        Php::Str(sanitize_user_core(text(&args[0]), strict, "en_US", |t| wp_strip_all_tags_core(t, false).into_owned()))
    });
}

#[test]
fn sanitize_file_name_corpus() {
    let (mimes, allowed) = (MimeTable::wordpress(), MimeTable::wordpress_allowed());
    assert_corpus("sanitize_file_name", |args| {
        let accents = remove_accents(text(&args[0]), "en_US");
        let prepared = sanitize_file_name_prepare(&accents);
        Php::Str(sanitize_file_name_core(&prepared, &default_special_chars(), &mimes, &allowed))
    });
}

#[test]
fn nfd_sweep_matches_php() {
    let cases = speedyspec_wp_text_corpus::cases("remove_accents");
    let sweep = cases.iter().find(|c| c.scenario.ends_with("accents_nfd_sweep")).expect("the sweep literal");
    assert_eq!(Php::Str(remove_accents(text(&sweep.args[0]), "en_US").into_owned()), sweep.returned);
}

#[test]
fn title_save_context_strips_marks() {
    assert_eq!(sanitize_title_with_dashes("a\u{200B}b\u{00D7}c".as_bytes(), &SanitizeContext::Save), b"abxc");
    assert_eq!(sanitize_title_with_dashes("a\u{200B}b\u{00D7}c".as_bytes(), &SanitizeContext::Display), b"a%e2%80%8bb%c3%97c");
}

#[test]
fn invalid_utf8_titles_skip_uri_encoding() {
    assert_eq!(sanitize_title_with_dashes(b"Caf\xe9 \xc3\xa9", &SanitizeContext::Save), b"caf");
}

#[test]
fn sanitize_context_round_trips() {
    for name in [&b"display"[..], b"save", b"query", b"edit"] {
        assert_eq!(SanitizeContext::from_bytes(name).as_bytes(), name);
    }
}
