//! Both attribute parsers return exactly what WordPress's `shortcode_parse_atts()` returns.

mod support;

use speedyspec_wp_shortcode::{HandAttsParser, RegexAttsParser};
use speedyspec_wp_shortcode_contracts::{AttKey, AttsParser};
use support::atts_cases::atts_cases;

fn check(parser: &dyn AttsParser) {
    for case in atts_cases() {
        let got = parser.parse(case.text);
        let entries: Vec<(AttKey, Vec<u8>)> = got.atts.iter().map(|(k, v)| (k.clone(), v.to_vec())).collect();
        assert_eq!(entries, case.expected, "text {:?}", String::from_utf8_lossy(case.text));
        assert_eq!(got.null_subject, case.null_subject, "text {:?}", String::from_utf8_lossy(case.text));
    }
}

#[test]
fn regex_parser_matches_table() {
    check(&RegexAttsParser::new());
}

#[test]
fn hand_parser_matches_table() {
    check(&HandAttsParser::new());
}
