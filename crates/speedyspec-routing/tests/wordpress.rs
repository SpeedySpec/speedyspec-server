use speedyspec_routing::{EndpointMask, PermastructArgs};

#[test]
fn endpoint_mask_values_match_wordpress() {
    let expected = [
        (EndpointMask::NONE, 0),
        (EndpointMask::PERMALINK, 1),
        (EndpointMask::ATTACHMENT, 2),
        (EndpointMask::DATE, 4),
        (EndpointMask::YEAR, 8),
        (EndpointMask::MONTH, 16),
        (EndpointMask::DAY, 32),
        (EndpointMask::ROOT, 64),
        (EndpointMask::COMMENTS, 128),
        (EndpointMask::SEARCH, 256),
        (EndpointMask::CATEGORIES, 512),
        (EndpointMask::TAGS, 1024),
        (EndpointMask::AUTHORS, 2048),
        (EndpointMask::PAGES, 4096),
        (EndpointMask::ALL_ARCHIVES, 3644),
        (EndpointMask::ALL, 8191),
    ];
    for (mask, value) in expected {
        assert_eq!(mask.0, value);
    }
    assert!(EndpointMask::ALL.contains(EndpointMask::PAGES));
    assert!(!EndpointMask::ALL_ARCHIVES.contains(EndpointMask::PERMALINK));
}

#[test]
fn permastruct_defaults_match_add_permastruct() {
    let args = PermastructArgs::default();
    assert!(args.with_front);
    assert_eq!(args.ep_mask, EndpointMask::NONE);
    assert!(args.paged);
    assert!(args.feed);
    assert!(!args.forcomments);
    assert!(args.walk_dirs);
    assert!(args.endpoints);
}
