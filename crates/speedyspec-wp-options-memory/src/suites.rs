//! Suites every implementation of a port passes, whatever stores it.

use speedyspec_wp_options::{NetworkMeta, SerializedValue, SitemetaRepository};

fn meta(network_id: i64, key: &str, value: &[u8]) -> NetworkMeta {
    NetworkMeta {
        network_id,
        key: key.into(),
        value: SerializedValue::new(value.to_vec()),
    }
}

fn value_of(repo: &dyn SitemetaRepository, network_id: i64, key: &str) -> Option<Vec<u8>> {
    repo.find(network_id, key)
        .unwrap()
        .map(|m| m.value.as_bytes().to_vec())
}

/// `wp_sitemeta`'s behaviour: finds, isolation between networks, and row counts as MySQL's.
pub fn sitemeta_repository_suite(repo: &dyn SitemetaRepository) {
    // add_then_find
    assert!(
        repo.add(meta(1, "suite_a", b"one")).unwrap(),
        "add_then_find: not added"
    );
    assert_eq!(
        value_of(repo, 1, "suite_a").as_deref(),
        Some(b"one".as_slice()),
        "add_then_find"
    );

    // find_many_skips_missing
    assert!(repo.add(meta(1, "suite_b", b"two")).unwrap());
    let found = repo
        .find_many(
            1,
            &["suite_a".into(), "suite_missing".into(), "suite_b".into()],
        )
        .unwrap();
    let mut keys: Vec<&str> = found.iter().map(|m| m.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["suite_a", "suite_b"], "find_many_skips_missing");
    assert!(
        found.iter().all(|m| m.network_id == 1),
        "find_many_skips_missing: network"
    );

    // networks_are_isolated
    assert_eq!(value_of(repo, 2, "suite_a"), None, "networks_are_isolated");
    assert!(
        !repo.exists(2, "suite_a").unwrap(),
        "networks_are_isolated: exists"
    );

    // update_counts_changed_rows
    assert_eq!(
        repo.update(meta(1, "suite_a", b"uno")).unwrap(),
        1,
        "update_counts_changed_rows: new value"
    );
    assert_eq!(
        repo.update(meta(1, "suite_a", b"uno")).unwrap(),
        0,
        "update_counts_changed_rows: same value"
    );
    assert_eq!(
        value_of(repo, 1, "suite_a").as_deref(),
        Some(b"uno".as_slice())
    );

    // remove_counts_rows
    assert_eq!(
        repo.remove(1, "suite_b").unwrap(),
        1,
        "remove_counts_rows: first"
    );
    assert_eq!(
        repo.remove(1, "suite_b").unwrap(),
        0,
        "remove_counts_rows: second"
    );

    // exists_follows_rows
    assert!(
        repo.exists(1, "suite_a").unwrap(),
        "exists_follows_rows: present"
    );
    assert!(
        !repo.exists(1, "suite_b").unwrap(),
        "exists_follows_rows: removed"
    );
    repo.remove(1, "suite_a").unwrap();
    assert!(
        !repo.exists(1, "suite_a").unwrap(),
        "exists_follows_rows: after remove"
    );
}
