//! Every scanner yields exactly the matches PHP's `preg_match_all` finds.

mod support;

use speedyspec_wp_shortcode::FancyRegexScanner;
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
