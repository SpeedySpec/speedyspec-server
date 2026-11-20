//! The SQL Store conformance suite.

use speedyspec_data::{DbError, DbValue, SqlStore, Statement, UpsertForm};
use speedyspec_data_query::{Query, parse_one};

fn check<T>(name: &str, result: Result<T, DbError>) -> T {
    result.unwrap_or_else(|e| panic!("conformance check `{name}` failed: {e}"))
}

fn ensure(name: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(ok, "conformance check `{name}` failed: {}", detail());
}

fn sql(name: &str, text: &str) -> Statement {
    check(name, parse_one(text))
}

fn value_of(store: &dyn SqlStore, table: &str, key: &str) -> Option<Vec<u8>> {
    let stmt = check(
        "lookup",
        Query::table(table)
            .select(&["value"])
            .where_eq("name", key)
            .build(),
    );
    let rows = check("lookup", store.query(&stmt));
    rows.rows
        .first()
        .and_then(|r| r.get_index(0))
        .and_then(DbValue::to_mysql_text)
}

fn insert(store: &dyn SqlStore, name: &str, table: &str, key: &str, value: &str) {
    let stmt = check(
        name,
        Query::insert_into(table, &["name", "value"])
            .values(vec![key.into(), value.into()])
            .build(),
    );
    check(name, store.execute(&stmt));
}

/// Runs every SQL Store check against `store`, using `scratch_table` (created and dropped here).
pub fn sql_store_suite(store: &dyn SqlStore, scratch_table: &str) {
    let t = scratch_table;

    // 1. create
    check(
        "create",
        store.execute(&sql("create", &format!("DROP TABLE IF EXISTS {t}"))),
    );
    check(
        "create",
        store.execute(&sql(
            "create",
            &format!(
                "CREATE TABLE {t} (id bigint(20) unsigned NOT NULL AUTO_INCREMENT, name varchar(191) NOT NULL \
                 DEFAULT '', value longtext NOT NULL, PRIMARY KEY (id), UNIQUE KEY name (name)) DEFAULT CHARSET=utf8mb4"
            ),
        )),
    );

    // 2. insert_id
    let stmt = check(
        "insert_id",
        Query::insert_into(t, &["name", "value"])
            .values(vec!["a".into(), "1".into()])
            .build(),
    );
    let result = check("insert_id", store.execute(&stmt));
    ensure(
        "insert_id",
        result.rows_affected == 1 && result.insert_id == Some(1),
        || format!("{result:?}"),
    );

    // 3. select_params
    let stmt = check(
        "select_params",
        Query::table(t)
            .select(&["name", "value"])
            .where_eq("name", "a")
            .build(),
    );
    let rows = check("select_params", store.query(&stmt));
    ensure("select_params", rows.rows.len() == 1, || {
        format!("{} rows", rows.rows.len())
    });
    let texts: Vec<Option<Vec<u8>>> = rows.rows[0]
        .values()
        .iter()
        .map(DbValue::to_mysql_text)
        .collect();
    ensure(
        "select_params",
        texts == vec![Some(b"a".to_vec()), Some(b"1".to_vec())],
        || format!("{texts:?}"),
    );
    ensure(
        "select_params",
        rows.columns.first().is_some_and(|c| c.name == "name"),
        || format!("{:?}", rows.columns),
    );

    // 4. changed_rows
    let same = check(
        "changed_rows",
        store.execute(&sql(
            "changed_rows",
            &format!("UPDATE {t} SET value = '1' WHERE name = 'a'"),
        )),
    );
    ensure("changed_rows", same.rows_affected == 0, || {
        format!("same value: {same:?}")
    });
    let changed = check(
        "changed_rows",
        store.execute(&sql(
            "changed_rows",
            &format!("UPDATE {t} SET value = '2' WHERE name = 'a'"),
        )),
    );
    ensure("changed_rows", changed.rows_affected == 1, || {
        format!("new value: {changed:?}")
    });

    // 5. upsert
    if store.capabilities().upsert != UpsertForm::None {
        let stmt = check(
            "upsert",
            Query::insert_into(t, &["name", "value"])
                .values(vec!["a".into(), "3".into()])
                .on_duplicate_update(&["name", "value"])
                .build(),
        );
        check("upsert", store.execute(&stmt));
        let count = check(
            "upsert",
            store.query(&sql(
                "upsert",
                &format!("SELECT COUNT(*) FROM {t} WHERE name = 'a'"),
            )),
        );
        ensure(
            "upsert",
            count.rows[0].get_index(0).and_then(DbValue::to_mysql_text) == Some(b"1".to_vec()),
            || format!("{:?}", count.rows),
        );
        ensure(
            "upsert",
            value_of(store, t, "a") == Some(b"3".to_vec()),
            || "value not updated".into(),
        );
    }

    // 6. rollback
    check("rollback", store.begin());
    insert(store, "rollback", t, "b", "x");
    check("rollback", store.rollback());
    ensure("rollback", value_of(store, t, "b").is_none(), || {
        "row b survived rollback".into()
    });
    ensure("rollback", !store.in_transaction(), || {
        "still in a transaction".into()
    });

    // 7. savepoint
    check("savepoint", store.begin());
    insert(store, "savepoint", t, "c", "x");
    check("savepoint", store.savepoint("s1"));
    insert(store, "savepoint", t, "d", "x");
    check("savepoint", store.rollback_to_savepoint("s1"));
    check("savepoint", store.commit());
    ensure("savepoint", value_of(store, t, "c").is_some(), || {
        "row c missing".into()
    });
    ensure("savepoint", value_of(store, t, "d").is_none(), || {
        "row d survived".into()
    });

    // 8. raw_transaction_tracked
    check(
        "raw_transaction_tracked",
        store.execute(&sql("raw_transaction_tracked", "START TRANSACTION")),
    );
    ensure("raw_transaction_tracked", store.in_transaction(), || {
        "START TRANSACTION not tracked".into()
    });
    insert(store, "raw_transaction_tracked", t, "e", "x");
    check("raw_transaction_tracked", store.reset());
    ensure("raw_transaction_tracked", !store.in_transaction(), || {
        "reset left a transaction".into()
    });
    ensure(
        "raw_transaction_tracked",
        value_of(store, t, "e").is_none(),
        || "row e survived reset".into(),
    );

    // 9. introspect
    let introspector = store.introspect();
    let tables = check("introspect", introspector.tables(Some(t)));
    ensure("introspect", tables.iter().any(|x| x == t), || {
        format!("{tables:?}")
    });
    let columns = check("introspect", introspector.columns(t));
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    ensure("introspect", names == ["id", "name", "value"], || {
        format!("{names:?}")
    });
    ensure("introspect", columns[0].key == "PRI", || {
        format!("{:?}", columns[0])
    });
    let indexes = check("introspect", introspector.indexes(t));
    ensure(
        "introspect",
        indexes.iter().any(|i| i.name == "name" && i.unique),
        || format!("{indexes:?}"),
    );

    // 10. parse_error_surfaces
    ensure(
        "parse_error_surfaces",
        matches!(parse_one("SELEC 1"), Err(DbError::Parse { .. })),
        || "SELEC 1 parsed".into(),
    );

    // 11. drop
    check(
        "drop",
        store.execute(&sql("drop", &format!("DROP TABLE {t}"))),
    );
}
