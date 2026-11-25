//! The checks every meta repository passes, so the memory and SQL repositories answer alike.

use crate::{Meta, MetaId, MetaKey, MetaRepository, ObjectId, SerializedValue};

fn ensure(check: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(ok, "meta repository check `{check}` failed: {}", detail());
}

fn key(k: &str) -> MetaKey {
    MetaKey::from(k)
}

fn val(v: &str) -> SerializedValue {
    SerializedValue::new(v.as_bytes().to_vec())
}

/// Runs every check against `repo`, which must start with an empty table. Panics naming the failing check.
pub fn meta_repository_suite(repo: &dyn MetaRepository) {
    let add = |object: u64, k: &MetaKey, v: Option<&SerializedValue>| {
        repo.add(ObjectId(object), k, v)
            .expect("add")
            .expect("the insert returns an id")
    };
    let ids_for = |object: u64, k: &MetaKey| repo.ids_for(ObjectId(object), k).expect("ids_for");

    // add_returns_increasing_ids
    let color = key("color");
    let a = add(1, &color, Some(&val("red")));
    let b = add(1, &color, Some(&val("blue")));
    let c = add(2, &color, Some(&val("green")));
    ensure("add_returns_increasing_ids", a < b && b < c, || {
        format!("{a:?} {b:?} {c:?}")
    });

    // count_for_counts_key_and_object
    let n1 = repo.count_for(ObjectId(1), &color).expect("count_for");
    let n2 = repo.count_for(ObjectId(2), &color).expect("count_for");
    let other_key = repo
        .count_for(ObjectId(1), &key("size"))
        .expect("count_for");
    let other_object = repo.count_for(ObjectId(9), &color).expect("count_for");
    ensure(
        "count_for_counts_key_and_object",
        (n1, n2, other_key, other_object) == (2, 1, 0, 0),
        || format!("{n1} {n2} {other_key} {other_object}"),
    );

    // ids_for_in_id_order
    let ids = ids_for(1, &color);
    ensure("ids_for_in_id_order", ids == [a, b], || format!("{ids:?}"));
    let none = ids_for(9, &color);
    ensure("ids_for_in_id_order", none.is_empty(), || {
        format!("{none:?}")
    });

    // update_by_key_all_rows: every row of the object and key changes
    let updated = repo
        .update_by_key(ObjectId(1), &color, Some(&val("pink")), None)
        .expect("update_by_key");
    let rows = repo.for_objects(&[ObjectId(1)]).expect("for_objects");
    let values: Vec<Option<&[u8]>> = rows
        .iter()
        .map(|m| m.value.as_ref().map(SerializedValue::as_bytes))
        .collect();
    ensure(
        "update_by_key_all_rows",
        updated == 2 && values == [Some(&b"pink"[..]), Some(&b"pink"[..])],
        || format!("{updated} {values:?}"),
    );
    let untouched = repo.by_mid(c).expect("by_mid");
    ensure(
        "update_by_key_all_rows",
        untouched.as_ref().and_then(|m| m.value.as_ref()) == Some(&val("green")),
        || format!("{untouched:?}"),
    );

    // update_same_bytes_is_zero: MySQL's changed-rows count (wpdb has no CLIENT_FOUND_ROWS), which
    // `update_metadata()` turns into false. Every row of object 1 holds "pink"; row `c` holds "green".
    let same_by_key = repo
        .update_by_key(ObjectId(1), &color, Some(&val("pink")), None)
        .expect("update_by_key");
    let same_by_mid = repo
        .update_by_mid(c, &color, Some(&val("green")))
        .expect("update_by_mid");
    ensure(
        "update_same_bytes_is_zero",
        same_by_key == 0 && same_by_mid == 0,
        || format!("{same_by_key} {same_by_mid}"),
    );

    // update_by_key_previous_matches_only
    let size = key("size");
    let s1 = add(3, &size, Some(&val("s")));
    let s2 = add(3, &size, Some(&val("m")));
    let none_matched = repo
        .update_by_key(ObjectId(3), &size, Some(&val("xl")), Some(&val("l")))
        .expect("update_by_key");
    let one_matched = repo
        .update_by_key(ObjectId(3), &size, Some(&val("xl")), Some(&val("m")))
        .expect("update_by_key");
    let first = repo.by_mid(s1).expect("by_mid");
    let second = repo.by_mid(s2).expect("by_mid");
    ensure(
        "update_by_key_previous_matches_only",
        none_matched == 0
            && one_matched == 1
            && first.and_then(|m| m.value) == Some(val("s"))
            && second.and_then(|m| m.value) == Some(val("xl")),
        || format!("{none_matched} {one_matched}"),
    );

    // update_by_key_none_writes_null
    let wrote = repo
        .update_by_key(ObjectId(3), &size, None, Some(&val("s")))
        .expect("update_by_key");
    let nulled = repo.by_mid(s1).expect("by_mid");
    ensure(
        "update_by_key_none_writes_null",
        wrote == 1 && nulled.as_ref().map(|m| m.value.is_none()) == Some(true),
        || format!("{wrote} {nulled:?}"),
    );

    // ids_matching_without_object: every object's rows for the key, in id order
    let tag = key("tag");
    let t1 = add(4, &tag, Some(&val("x")));
    let t2 = add(5, &tag, Some(&val("y")));
    let t3 = add(4, &tag, Some(&val("y")));
    let all = repo.ids_matching(&tag, None, None).expect("ids_matching");
    let mut sorted = all.clone();
    sorted.sort();
    ensure(
        "ids_matching_without_object",
        sorted == [t1, t2, t3],
        || format!("{all:?}"),
    );
    let of_object = repo
        .ids_matching(&tag, Some(ObjectId(4)), None)
        .expect("ids_matching");
    let mut of_object_sorted = of_object.clone();
    of_object_sorted.sort();
    ensure(
        "ids_matching_without_object",
        of_object_sorted == [t1, t3],
        || format!("{of_object:?}"),
    );

    // ids_matching_with_value
    let y_rows = repo
        .ids_matching(&tag, None, Some(&val("y")))
        .expect("ids_matching");
    let mut y_sorted = y_rows.clone();
    y_sorted.sort();
    let y_of_object = repo
        .ids_matching(&tag, Some(ObjectId(4)), Some(&val("y")))
        .expect("ids_matching");
    let no_value = repo
        .ids_matching(&tag, None, Some(&val("z")))
        .expect("ids_matching");
    ensure(
        "ids_matching_with_value",
        y_sorted == [t2, t3] && y_of_object == [t3] && no_value.is_empty(),
        || format!("{y_rows:?} {y_of_object:?} {no_value:?}"),
    );

    // objects_with_value
    let mut with_y = repo
        .objects_with(&tag, Some(&val("y")))
        .expect("objects_with");
    with_y.sort();
    let mut with_any = repo.objects_with(&tag, None).expect("objects_with");
    with_any.sort();
    with_any.dedup();
    ensure(
        "objects_with_value",
        with_y == [ObjectId(4), ObjectId(5)] && with_any == [ObjectId(4), ObjectId(5)],
        || format!("{with_y:?} {with_any:?}"),
    );
    let only_x = repo
        .objects_with(&tag, Some(&val("x")))
        .expect("objects_with");
    ensure("objects_with_value", only_x == [ObjectId(4)], || {
        format!("{only_x:?}")
    });

    // delete_ids_counts
    let deleted = repo.delete_ids(&[t1, t2]).expect("delete_ids");
    let again = repo.delete_ids(&[t1, t2]).expect("delete_ids");
    let empty = repo.delete_ids(&[]).expect("delete_ids");
    let left = repo.ids_matching(&tag, None, None).expect("ids_matching");
    ensure(
        "delete_ids_counts",
        deleted == 2 && again == 0 && empty == 0 && left == [t3],
        || format!("{deleted} {again} {empty} {left:?}"),
    );

    // by_mid_found_and_missing
    let found = repo.by_mid(t3).expect("by_mid");
    let expected = Meta {
        mid: t3,
        object_id: ObjectId(4),
        key: tag.clone(),
        value: Some(val("y")),
    };
    let missing = repo.by_mid(MetaId(u64::MAX)).expect("by_mid");
    let deleted_row = repo.by_mid(t1).expect("by_mid");
    ensure(
        "by_mid_found_and_missing",
        found.as_ref() == Some(&expected) && missing.is_none() && deleted_row.is_none(),
        || format!("{found:?} {missing:?} {deleted_row:?}"),
    );

    // null_value_round_trips
    let nullable = key("nullable");
    let n = add(6, &nullable, None);
    let row = repo.by_mid(n).expect("by_mid");
    let listed = repo.for_objects(&[ObjectId(6)]).expect("for_objects");
    let matched_null_only = repo
        .ids_matching(&nullable, Some(ObjectId(6)), None)
        .expect("ids_matching");
    ensure(
        "null_value_round_trips",
        row.as_ref().map(|m| m.value.is_none()) == Some(true)
            && listed.len() == 1
            && listed[0].value.is_none()
            && matched_null_only == [n],
        || format!("{row:?} {listed:?} {matched_null_only:?}"),
    );
    let empty_string = add(6, &key("empty"), Some(&val("")));
    let empty_row = repo.by_mid(empty_string).expect("by_mid");
    ensure(
        "null_value_round_trips",
        empty_row.and_then(|m| m.value) == Some(val("")),
        || "an empty value must not read back as NULL".to_owned(),
    );

    // update_by_mid_changes_key
    let renamed = key("renamed");
    let changed = repo
        .update_by_mid(n, &renamed, Some(&val("now set")))
        .expect("update_by_mid");
    let after = repo.by_mid(n).expect("by_mid");
    let to_null = repo
        .update_by_mid(n, &renamed, None)
        .expect("update_by_mid");
    let after_null = repo.by_mid(n).expect("by_mid");
    let missing_update = repo
        .update_by_mid(MetaId(u64::MAX), &renamed, None)
        .expect("update_by_mid");
    ensure(
        "update_by_mid_changes_key",
        changed == 1
            && after
                == Some(Meta {
                    mid: n,
                    object_id: ObjectId(6),
                    key: renamed.clone(),
                    value: Some(val("now set")),
                })
            && to_null == 1
            && after_null.map(|m| m.value.is_none()) == Some(true)
            && missing_update == 0,
        || format!("{changed} {after:?} {to_null} {missing_update}"),
    );

    // delete_by_mid_missing_is_zero
    let gone = repo.delete_by_mid(n).expect("delete_by_mid");
    let gone_again = repo.delete_by_mid(n).expect("delete_by_mid");
    let never = repo.delete_by_mid(MetaId(u64::MAX)).expect("delete_by_mid");
    ensure(
        "delete_by_mid_missing_is_zero",
        gone == 1 && gone_again == 0 && never == 0 && repo.by_mid(n).expect("by_mid").is_none(),
        || format!("{gone} {gone_again} {never}"),
    );

    // for_objects_ordered_by_id: rows of objects 7 and 3 interleaved come back in id order
    let order = key("order");
    let o1 = add(7, &order, Some(&val("a")));
    let o2 = add(3, &order, Some(&val("b")));
    let o3 = add(7, &order, Some(&val("c")));
    let o4 = add(3, &order, Some(&val("d")));
    let rows = repo
        .for_objects(&[ObjectId(7), ObjectId(3)])
        .expect("for_objects");
    let ours: Vec<(MetaId, ObjectId)> = rows
        .iter()
        .filter(|m| m.key == order)
        .map(|m| (m.mid, m.object_id))
        .collect();
    let mids: Vec<MetaId> = rows.iter().map(|m| m.mid).collect();
    let mut sorted_mids = mids.clone();
    sorted_mids.sort();
    ensure(
        "for_objects_ordered_by_id",
        ours == [
            (o1, ObjectId(7)),
            (o2, ObjectId(3)),
            (o3, ObjectId(7)),
            (o4, ObjectId(3)),
        ] && mids == sorted_mids,
        || format!("{ours:?} {mids:?}"),
    );
    let unlisted = repo.for_objects(&[ObjectId(7)]).expect("for_objects");
    ensure(
        "for_objects_ordered_by_id",
        unlisted.iter().all(|m| m.object_id == ObjectId(7)) && unlisted.len() == 2,
        || format!("{unlisted:?}"),
    );

    // for_objects_empty_list_is_empty
    let nothing = repo.for_objects(&[]).expect("for_objects");
    ensure(
        "for_objects_empty_list_is_empty",
        nothing.is_empty(),
        || format!("{nothing:?}"),
    );
}

/// Checks for repositories whose columns are binary-safe (memory, latin1 or binary tables); `meta_repository_suite`
/// leaves them out. On stock utf8mb4 tables `wpdb::process_fields()` strips invalid text and `$wpdb->insert` returns
/// false, so a faithful SQL repository answers `Ok(None)` from `add` and fails this suite by design. Like
/// `meta_repository_suite`, it needs an empty table.
pub fn binary_safe_suite(repo: &dyn MetaRepository) {
    let add = |object: u64, k: &MetaKey, v: Option<&SerializedValue>| {
        repo.add(ObjectId(object), k, v)
            .expect("add")
            .expect("the insert returns an id")
    };

    // non_utf8_key_and_value_round_trip
    let raw_key = MetaKey::new(vec![b'k', 0x00, 0xC2, 0xA0, 0xFF]);
    let raw_value = SerializedValue::new(vec![0x00, 0xFF, 0xFE, b'v', 0xC3]);
    let raw = add(8, &raw_key, Some(&raw_value));
    let raw_row = repo.by_mid(raw).expect("by_mid");
    let raw_count = repo.count_for(ObjectId(8), &raw_key).expect("count_for");
    let raw_ids = repo
        .ids_matching(&raw_key, Some(ObjectId(8)), Some(&raw_value))
        .expect("ids_matching");
    ensure(
        "non_utf8_key_and_value_round_trip",
        raw_row
            == Some(Meta {
                mid: raw,
                object_id: ObjectId(8),
                key: raw_key.clone(),
                value: Some(raw_value.clone()),
            })
            && raw_count == 1
            && raw_ids == [raw],
        || format!("{raw_row:?} {raw_count} {raw_ids:?}"),
    );
}
