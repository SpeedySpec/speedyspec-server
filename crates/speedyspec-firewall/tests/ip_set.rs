use speedyspec_firewall::{IpSet, RequestFacts};

#[test]
fn ip_set_matches_cidr_and_single_ip() {
    let (set, issues) = IpSet::parse("10.0.0.0/8\n2001:db8::1\n\n# comment\n");
    assert!(issues.is_empty());
    assert!(set.contains("10.1.2.3".parse().unwrap()));
    assert!(set.contains("2001:db8::1".parse().unwrap()));
    assert!(!set.contains("11.0.0.1".parse().unwrap()));
    assert!(!set.contains("2001:db8::2".parse().unwrap()));
}

#[test]
fn ip_set_matches_ipv4_mapped_ipv6() {
    let params = vec![
        ("REMOTE_ADDR".to_string(), "::ffff:10.0.0.5".to_string()),
        ("REQUEST_URI".to_string(), "/".to_string()),
    ];
    let facts = RequestFacts::from_params(&params).unwrap();
    assert_eq!(
        facts.client,
        "10.0.0.5".parse::<std::net::IpAddr>().unwrap()
    );
    let (set, _) = IpSet::parse("10.0.0.0/24");
    assert!(set.contains(facts.client));
    assert!(set.contains("::ffff:10.0.0.9".parse().unwrap()));
}

#[test]
fn client_key_groups_ipv6_by_64_and_keeps_ipv4_whole() {
    use speedyspec_firewall::client_key;
    assert_eq!(client_key("192.0.2.7".parse().unwrap()), "192.0.2.7");
    assert_eq!(client_key("::ffff:192.0.2.7".parse().unwrap()), "192.0.2.7");
    assert_eq!(
        client_key("2001:db8:1:2:aaaa::1".parse().unwrap()),
        "2001:db8:1:2::/64"
    );
    assert_eq!(
        client_key("2001:db8:1:2:ffff::9".parse().unwrap()),
        client_key("2001:db8:1:2::1".parse().unwrap())
    );
    assert_ne!(
        client_key("2001:db8:1:3::1".parse().unwrap()),
        client_key("2001:db8:1:2::1".parse().unwrap())
    );
}
