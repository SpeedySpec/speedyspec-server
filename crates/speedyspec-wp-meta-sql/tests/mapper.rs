use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_meta_contracts::{
    MetaError, MetaId, MetaKey, MetaType, ObjectId, SerializedValue,
};
use speedyspec_wp_meta_sql::MetaMapper;

fn row(cols: &[&str], values: Vec<DbValue>) -> Row {
    let cols: Rc<[String]> = cols.iter().map(|c| c.to_string()).collect();
    Row::new(cols, values)
}

fn mapper(t: &str) -> MetaMapper {
    MetaMapper::new(MetaType::new(t).unwrap())
}

#[test]
fn post_row_to_entity() {
    let r = row(
        &["meta_id", "post_id", "meta_key", "meta_value"],
        vec![
            DbValue::UInt(7),
            DbValue::UInt(3),
            DbValue::Text("k".into()),
            DbValue::Text("v".into()),
        ],
    );
    let m = mapper("post").to_entity(&r).unwrap();
    assert_eq!(m.mid, MetaId(7));
    assert_eq!(m.object_id, ObjectId(3));
    assert_eq!(m.key, MetaKey::from("k"));
    assert_eq!(m.value, Some(SerializedValue::new(b"v".to_vec())));
}

#[test]
fn user_row_uses_umeta_id() {
    let r = row(
        &["umeta_id", "user_id", "meta_key", "meta_value"],
        vec![
            DbValue::Int(9),
            DbValue::Int(2),
            DbValue::Text("k".into()),
            DbValue::Text("v".into()),
        ],
    );
    let m = mapper("user").to_entity(&r).unwrap();
    assert_eq!(m.mid, MetaId(9));
    assert_eq!(m.object_id, ObjectId(2));
}

#[test]
fn null_value_is_none() {
    let r = row(
        &["meta_id", "post_id", "meta_key", "meta_value"],
        vec![
            DbValue::Int(1),
            DbValue::Int(1),
            DbValue::Text("k".into()),
            DbValue::Null,
        ],
    );
    assert_eq!(mapper("post").to_entity(&r).unwrap().value, None);
}

#[test]
fn text_ids_parse() {
    let r = row(
        &["meta_id", "post_id", "meta_key", "meta_value"],
        vec![
            DbValue::Text("12".into()),
            DbValue::Bytes(b"34".to_vec()),
            DbValue::Text("k".into()),
            DbValue::Text("".into()),
        ],
    );
    let m = mapper("post").to_entity(&r).unwrap();
    assert_eq!(m.mid, MetaId(12));
    assert_eq!(m.object_id, ObjectId(34));
    assert_eq!(m.value, Some(SerializedValue::new(Vec::new())));
}

#[test]
fn missing_id_is_db_error() {
    let r = row(
        &["post_id", "meta_key", "meta_value"],
        vec![DbValue::Int(1), DbValue::Text("k".into()), DbValue::Null],
    );
    assert!(matches!(
        mapper("post").to_entity(&r),
        Err(MetaError::Db(_))
    ));
    let bad = row(
        &["meta_id", "post_id", "meta_key", "meta_value"],
        vec![
            DbValue::Text("x".into()),
            DbValue::Int(1),
            DbValue::Text("k".into()),
            DbValue::Null,
        ],
    );
    assert!(matches!(
        mapper("post").to_entity(&bad),
        Err(MetaError::Db(_))
    ));
}

#[test]
fn insert_values_order_and_null() {
    let v = mapper("post").insert_values(ObjectId(5), &MetaKey::from("k"), None);
    assert_eq!(
        v,
        vec![
            ("post_id".to_string(), DbValue::UInt(5)),
            ("meta_key".to_string(), DbValue::Text("k".into())),
            ("meta_value".to_string(), DbValue::Null),
        ]
    );
    let w = mapper("widget").insert_values(
        ObjectId(5),
        &MetaKey::from("k"),
        Some(&SerializedValue::new(b"v".to_vec())),
    );
    assert_eq!(w[0].0, "widget_id");
    assert_eq!(w[2].1, DbValue::Text("v".into()));
}

#[test]
fn non_utf8_value_is_bytes() {
    let v = mapper("post").insert_values(
        ObjectId(1),
        &MetaKey::new(vec![0xFF]),
        Some(&SerializedValue::new(vec![0xC2])),
    );
    assert_eq!(v[1].1, DbValue::Bytes(vec![0xFF]));
    assert_eq!(v[2].1, DbValue::Bytes(vec![0xC2]));
}
