//! Differences the final review found, each pinned to PHP 8.5.12's own answer.

use std::rc::Rc;

use proptest::prelude::*;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_text::email::{EmailCheck, is_email_core, sanitize_email_core};
use speedyspec_wp_text::esc_url::looks_like_php_script;
use speedyspec_wp_text::file_name::{MimeTable, default_special_chars_replace, sanitize_file_name_core};
use speedyspec_wp_text::links::{make_absolute_url, split_str_by_whitespace};
use speedyspec_wp_text::parse_str::parse_str;
use speedyspec_wp_text::preg::{Pattern, preg_match, preg_replace};

#[test]
fn split_counts_nul_as_whitespace() {
    // PHP: _split_str_by_whitespace( str_repeat('a',3000) . "\0" . 'www.example.com ' . str_repeat('b',8000), 2100 ).
    let text = [vec![b'a'; 3000], vec![0], b"www.example.com ".to_vec(), vec![b'b'; 8000]].concat();
    let lengths: Vec<usize> = split_str_by_whitespace(&text, 2100).iter().map(|c| c.len()).collect();
    assert_eq!(lengths, vec![3001, 16, 8000]);
}

/// `_split_str_by_whitespace()` transcribed literally: strtr to NULs, then strrpos/strpos.
fn split_reference(text: &[u8], goal: usize) -> Vec<Vec<u8>> {
    let mut text = text.to_vec();
    let mut nulled: Vec<u8> = text.iter().map(|&b| if b"\r\n\t\x0B\x0C ".contains(&b) { 0 } else { b }).collect();
    let mut chunks = Vec::new();
    while goal < nulled.len() {
        let pos = match nulled[..(goal + 1).min(nulled.len())].iter().rposition(|&b| b == 0) {
            Some(p) => p,
            None => match nulled[goal + 1..].iter().position(|&b| b == 0) {
                Some(p) => goal + 1 + p,
                None => break,
            },
        };
        chunks.push(text[..pos + 1].to_vec());
        text = text[pos + 1..].to_vec();
        nulled = nulled[pos + 1..].to_vec();
    }
    if !text.is_empty() && text != b"0" {
        chunks.push(text);
    }
    chunks
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn split_matches_reference(text in proptest::collection::vec(prop_oneof![Just(b'a'), Just(b' '), Just(0u8), Just(b'\t'), Just(b'0')], 0..40), goal in 1usize..12) {
        let got: Vec<Vec<u8>> = split_str_by_whitespace(&text, goal).iter().map(|c| c.to_vec()).collect();
        prop_assert_eq!(got, split_reference(&text, goal));
    }

    #[test]
    fn email_checks_match_patterns(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"a"[..], b"Z", b"0", b"-", b".", b"..", b"@", b"!", b"#", b"'", b"`", b"~", b" ", b"\n", b"_", b"(", b"\xC3\xA9", b"example", b"com",
    ]), 0..12).prop_map(|p| p.concat())) {
        prop_assert_eq!(is_email_core(&input), is_email_reference(&input));
        prop_assert_eq!(sanitize_email_core(&input), sanitize_email_reference(&input));
    }

    #[test]
    fn php_script_matches_pattern(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"a"[..], b"-", b"0", b".php", b".PHP", b".ph", b"/", b"?", b".",
    ]), 0..6).prop_map(|p| p.concat())) {
        let expected = matches!(preg_match(&Pattern::php("/^[a-z0-9-]+?\\.php/i"), &input), Ok(Some(_)));
        prop_assert_eq!(looks_like_php_script(&input), expected);
    }
}

/// PHP's `trim()` with a list of characters.
fn trim<'a>(s: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = s.iter().position(|b| !chars.contains(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !chars.contains(b)).map_or(start, |i| i + 1);
    &s[start..end]
}

fn matches(source: &str, subject: &[u8]) -> bool {
    matches!(preg_match(&Pattern::php(source), subject), Ok(Some(_)))
}

fn replace(source: &str, subject: &[u8]) -> Vec<u8> {
    preg_replace(&Pattern::php(source), subject, b"", None).unwrap().0.into_owned()
}

/// `is_email()` transcribed with WordPress's patterns.
fn is_email_reference(email: &[u8]) -> EmailCheck<'_> {
    if email.len() < 6 {
        return EmailCheck::Invalid("email_too_short");
    }
    if !email[1..].contains(&b'@') {
        return EmailCheck::Invalid("email_no_at");
    }
    let at = email.iter().position(|&b| b == b'@').unwrap();
    let (local, domain) = (&email[..at], &email[at + 1..]);
    if !matches("/^[a-zA-Z0-9!#$%&\\'*+\\/=?^_`{|}~\\.-]+$/", local) {
        return EmailCheck::Invalid("local_invalid_chars");
    }
    if matches("/\\.{2,}/", domain) {
        return EmailCheck::Invalid("domain_period_sequence");
    }
    if trim(domain, b" \t\n\r\0\x0B.") != domain {
        return EmailCheck::Invalid("domain_period_limits");
    }
    let subs: Vec<&[u8]> = domain.split(|&b| b == b'.').collect();
    if subs.len() < 2 {
        return EmailCheck::Invalid("domain_no_periods");
    }
    for sub in subs {
        if trim(sub, b" \t\n\r\0\x0B-") != sub {
            return EmailCheck::Invalid("sub_hyphen_limits");
        }
        if !matches("/^[a-z0-9-]+$/i", sub) {
            return EmailCheck::Invalid("sub_invalid_chars");
        }
    }
    EmailCheck::Valid(email)
}

/// `sanitize_email()` transcribed with WordPress's patterns.
fn sanitize_email_reference(email: &[u8]) -> (Vec<u8>, Option<&'static str>) {
    let invalid = |c| (Vec::new(), Some(c));
    if email.len() < 6 {
        return invalid("email_too_short");
    }
    if !email[1..].contains(&b'@') {
        return invalid("email_no_at");
    }
    let at = email.iter().position(|&b| b == b'@').unwrap();
    let local = replace("/[^a-zA-Z0-9!#$%&\\'*+\\/=?^_`{|}~\\.-]/", &email[..at]);
    if local.is_empty() {
        return invalid("local_invalid_chars");
    }
    let domain = replace("/\\.{2,}/", &email[at + 1..]);
    if domain.is_empty() {
        return invalid("domain_period_sequence");
    }
    let domain = trim(&domain, b" \t\n\r\0\x0B.").to_vec();
    if domain.is_empty() {
        return invalid("domain_period_limits");
    }
    let subs: Vec<&[u8]> = domain.split(|&b| b == b'.').collect();
    if subs.len() < 2 {
        return invalid("domain_no_periods");
    }
    let new: Vec<Vec<u8>> = subs
        .iter()
        .map(|s| replace("/[^a-z0-9-]+/i", trim(s, b" \t\n\r\0\x0B-")))
        .filter(|s| !s.is_empty())
        .collect();
    if new.len() < 2 {
        return invalid("domain_no_valid_subs");
    }
    ([local.as_slice(), b"@", &new.join(&b'.')].concat(), None)
}

#[test]
fn absolute_url_with_a_slashless_base_path_keeps_its_first_byte() {
    // PHP: WP_Http::make_absolute_url( 'foo.html', 'example.com' ) and ( 'q?x=1', 'mailto:bob' ).
    assert_eq!(make_absolute_url(b"foo.html", b"example.com"), b":///efoo.html");
    assert_eq!(make_absolute_url(b"q?x=1", b"mailto:bob"), b"mailto:///bq?x=1");
}

#[test]
fn parse_str_drops_an_append_past_the_last_index() {
    // PHP: parse_str( 'a[9223372036854775807]=1&a[]=2&b=3' ).
    let parsed = parse_str(b"a[9223372036854775807]=1&a[]=2&b=3");
    let expected = Value::Array(vec![
        (Key::Str("a".into()), Value::Array(vec![(Key::Int(i64::MAX), Value::Str("1".into()))])),
        (Key::Str("b".into()), Value::Str("3".into())),
    ]);
    assert_eq!(parsed, expected);
}

#[test]
fn check_filetype_falls_back_to_allowed_types_for_an_empty_list() {
    // wp_check_filetype( 'test.jpg', array() ) uses get_allowed_mime_types().
    let out = sanitize_file_name_core::<()>(
        b"jpg",
        default_special_chars_replace(),
        || Ok(Rc::new(Vec::new())),
        || Ok(Rc::new(MimeTable::wordpress_allowed().0)),
    )
    .unwrap();
    assert_eq!(out, b"unnamed-file.jpg");
}
