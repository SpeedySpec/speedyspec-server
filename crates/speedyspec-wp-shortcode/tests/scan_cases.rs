//! Every scanner yields exactly the matches PHP's `preg_match_all` finds.

mod support;

use speedyspec_wp_shortcode::{FancyRegexScanner, HandScanner, candidate_tokens};
use speedyspec_wp_shortcode_contracts::{ShortcodeScanner, TagName, TagSet};
use support::scan_cases::scan_cases;

fn check(scanner: &dyn ShortcodeScanner) {
    for case in scan_cases() {
        let tags = TagSet::new(case.tags.iter().map(|t| TagName::from_global(t.to_vec())).collect(), 1);
        let got: Vec<_> = scanner
            .scan(case.content, &tags)
            .map(|m| m.expect("scan").groups)
            .collect();
        assert_eq!(got, case.expected, "content {:?} tags {:?}", String::from_utf8_lossy(case.content), case.tags);
    }
}

#[test]
fn fancy_scanner_matches_table() {
    check(&FancyRegexScanner::new());
}

#[test]
fn hand_scanner_matches_table() {
    check(&HandScanner::new());
}

#[test]
fn candidate_tokens_take_the_longest_unreserved_run() {
    let got: Vec<&[u8]> = candidate_tokens(b"[a.b x] [[c] [=d] [e/f] [\xffg<h] [").collect();
    assert_eq!(got, vec![&b"a.b"[..], b"c", b"e", b"\xffg"]);
}

/// Openers with no closer must not make the scan quadratic (Review Focus 3).
#[test]
#[ignore = "timing; run with --release -- --ignored"]
fn unclosed_openers_are_linear() {
    let content = b"[a]".repeat(200_000);
    let tags = TagSet::new(vec![TagName::from_global(b"a".to_vec())], 1);
    let scanner = HandScanner::new();
    let started = std::time::Instant::now();
    assert_eq!(scanner.scan(&content, &tags).count(), 200_000);
    assert!(started.elapsed() < std::time::Duration::from_millis(50), "{:?}", started.elapsed());
    let open = b"[a ".repeat(200_000);
    let started = std::time::Instant::now();
    assert_eq!(scanner.scan(&open, &tags).count(), 0);
    assert!(started.elapsed() < std::time::Duration::from_millis(50), "{:?}", started.elapsed());
}
