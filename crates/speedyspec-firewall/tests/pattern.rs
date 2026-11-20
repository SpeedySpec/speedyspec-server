use speedyspec_firewall::{Field, PatternRule, RequestFacts};

fn facts(uri: &str, ua: &str) -> RequestFacts {
    let params = vec![
        ("REMOTE_ADDR".to_string(), "192.0.2.1".to_string()),
        ("REQUEST_URI".to_string(), uri.to_string()),
        ("HTTP_USER_AGENT".to_string(), ua.to_string()),
    ];
    RequestFacts::from_params(&params).unwrap()
}

#[test]
fn pattern_rule_matches_field() {
    let (rules, issues) = PatternRule::parse_lines("path 403 ^/wp-config\nua 429 (?i)sqlmap\nquery 403 union.+select");
    assert!(issues.is_empty());
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0].field, Field::Path);
    assert_eq!(rules[1].status, 429);
    assert!(rules[0].matches(&facts("/wp-config.php", "x")));
    assert!(!rules[0].matches(&facts("/a/wp-config.php", "x")));
    assert!(rules[1].matches(&facts("/", "SQLMap/1.0")));
    assert!(rules[2].matches(&facts("/?q=union%20all%20select", "x")));
    assert!(!rules[2].matches(&facts("/union-select", "x")));
}

#[test]
fn malformed_lines_are_skipped_and_reported() {
    let (rules, issues) = PatternRule::parse_lines("path 403 ^/ok\nbody 403 x\npath 403 (\npath 500 x");
    assert_eq!(rules.len(), 1);
    assert_eq!(issues.iter().map(|i| i.line).collect::<Vec<_>>(), vec![2, 3, 4]);
    assert!(issues.iter().all(|i| i.setting == "patterns"));

    let (set, issues) = speedyspec_firewall::IpSet::parse("999.1.1.1\n10.0.0.1");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, 1);
    assert!(set.contains("10.0.0.1".parse().unwrap()));
}
