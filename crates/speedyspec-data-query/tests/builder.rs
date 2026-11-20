use speedyspec_data::{DbError, DbValue};
use speedyspec_data_query::{Order, Query};

#[test]
fn select_with_where_in() {
    let stmt = Query::table("wp_options")
        .select(&["option_name", "option_value"])
        .where_in("autoload", ["yes", "on"])
        .order_by("option_id", Order::Asc)
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT option_name, option_value FROM wp_options WHERE autoload IN (?, ?) ORDER BY option_id ASC"
    );
    assert_eq!(
        stmt.params(),
        &[DbValue::Text("yes".into()), DbValue::Text("on".into())]
    );
}

#[test]
fn insert_on_duplicate() {
    let stmt = Query::insert_into("wp_options", &["option_name", "option_value", "autoload"])
        .values(vec!["a".into(), "b".into(), "auto".into()])
        .on_duplicate_update(&["option_name", "option_value", "autoload"])
        .build()
        .unwrap();
    let text = stmt.tree().to_string();
    assert!(
        text.starts_with(
            "INSERT INTO wp_options (option_name, option_value, autoload) VALUES (?, ?, ?)"
        ),
        "{text}"
    );
    assert!(
        text.contains("ON DUPLICATE KEY UPDATE option_name = VALUES(option_name)"),
        "{text}"
    );
    assert_eq!(stmt.params().len(), 3);
}

#[test]
fn update_and_delete() {
    // `set` after `where_eq`: params still follow the text's placeholder order.
    let update = Query::update("wp_options")
        .where_eq("option_name", "siteurl")
        .set("option_value", "x")
        .build()
        .unwrap();
    assert_eq!(
        update.tree().to_string(),
        "UPDATE wp_options SET option_value = ? WHERE option_name = ?"
    );
    assert_eq!(
        update.params(),
        &[DbValue::Text("x".into()), DbValue::Text("siteurl".into())]
    );

    let delete = Query::delete_from("wp_options")
        .where_eq("option_name", "a")
        .build()
        .unwrap();
    assert_eq!(
        delete.tree().to_string(),
        "DELETE FROM wp_options WHERE option_name = ?"
    );
    assert_eq!(delete.params(), &[DbValue::Text("a".into())]);
}

#[test]
fn limit_offset_and_keyword_identifiers() {
    let stmt = Query::table("t")
        .where_eq("key", 1i64)
        .limit(10)
        .offset(20)
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT * FROM t WHERE `key` = ? LIMIT 10 OFFSET 20"
    );
}

#[test]
fn misuse_is_reported_at_build() {
    assert!(matches!(
        Query::table("t").set("a", 1i64).build(),
        Err(DbError::Unsupported { .. })
    ));
}
