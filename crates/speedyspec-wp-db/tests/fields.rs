use indexmap::IndexMap;
use speedyspec_hooks::Value;
use speedyspec_wp_db::{
    Field, FieldFormats, QueryClass, delete_sql, get_table_from_query, insert_sql, update_sql,
};

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn formats(data: Vec<(&str, Value)>, format: Option<Vec<&str>>) -> Vec<(String, Value, String)> {
    let mut types = IndexMap::new();
    types.insert("post_parent".to_string(), "%d".to_string());
    let data: Vec<(String, Value)> = data.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let format: Option<Vec<String>> = format.map(|f| f.into_iter().map(str::to_string).collect());
    FieldFormats {
        field_types: &types,
    }
    .process_field_formats(&data, format.as_deref())
    .into_iter()
    .map(|f| (f.name, f.value, f.format))
    .collect()
}

fn row(name: &str, value: Value, format: &str) -> (String, Value, String) {
    (name.to_string(), value, format.to_string())
}

#[test]
fn process_field_formats_matches_wordpress() {
    assert_eq!(
        formats(
            vec![("post_content", s("foo")), ("post_parent", Value::Int(0))],
            None
        ),
        vec![
            row("post_content", s("foo"), "%s"),
            row("post_parent", Value::Int(0), "%d")
        ]
    );
    assert_eq!(
        formats(
            vec![("post_content", s("foo")), ("post_parent", Value::Int(0))],
            Some(vec!["%d", "%s"])
        ),
        vec![
            row("post_content", s("foo"), "%d"),
            row("post_parent", Value::Int(0), "%s")
        ]
    );
    assert_eq!(
        formats(
            vec![
                ("this_is_not_a_core_field", s("foo")),
                ("this_is_not_either", Value::Int(0))
            ],
            None
        ),
        vec![
            row("this_is_not_a_core_field", s("foo"), "%s"),
            row("this_is_not_either", Value::Int(0), "%s")
        ]
    );
    assert_eq!(
        formats(
            vec![
                ("this_is_not_a_core_field", Value::Int(0)),
                ("this_is_not_either", Value::Float(1.2))
            ],
            Some(vec!["%d", "%f"])
        ),
        vec![
            row("this_is_not_a_core_field", Value::Int(0), "%d"),
            row("this_is_not_either", Value::Float(1.2), "%f")
        ]
    );
    assert_eq!(
        formats(
            vec![
                ("this_is_not_a_core_field", Value::Int(0)),
                ("this_is_not_either", s("s")),
                ("nor_this", Value::Int(1))
            ],
            Some(vec!["%d", "%s"])
        ),
        vec![
            row("this_is_not_a_core_field", Value::Int(0), "%d"),
            row("this_is_not_either", s("s"), "%s"),
            row("nor_this", Value::Int(1), "%d")
        ]
    );
    // An empty format list is no format.
    assert_eq!(
        formats(vec![("post_parent", Value::Int(3))], Some(vec![])),
        vec![row("post_parent", Value::Int(3), "%d")]
    );
}

fn field(name: &str, value: Value, format: &str) -> Field {
    Field {
        name: name.into(),
        value,
        format: format.into(),
        charset: None,
        length: None,
    }
}

#[test]
fn insert_and_update_text() {
    let (sql, args) = insert_sql(
        "wp_t",
        &[
            field("a", s("x"), "%s"),
            field("b", Value::Int(1), "%d"),
            field("c", Value::Null, "%s"),
        ],
        "INSERT",
    );
    assert_eq!(
        sql,
        "INSERT INTO `wp_t` (`a`, `b`, `c`) VALUES (%s, %d, NULL)"
    );
    assert_eq!(args, vec![s("x"), Value::Int(1)]);
    let (sql, args) = update_sql(
        "wp_t",
        &[field("a", s("y"), "%s"), field("b", Value::Null, "%d")],
        &[
            field("id", Value::Int(4), "%d"),
            field("d", Value::Null, "%s"),
        ],
    );
    assert_eq!(
        sql,
        "UPDATE `wp_t` SET `a` = %s, `b` = NULL WHERE `id` = %d AND `d` IS NULL"
    );
    assert_eq!(args, vec![s("y"), Value::Int(4)]);
    let (sql, args) = delete_sql("wp_t", &[field("id", Value::Int(4), "%d")]);
    assert_eq!(sql, "DELETE FROM `wp_t` WHERE `id` = %d");
    assert_eq!(args, vec![Value::Int(4)]);
}

#[test]
fn classify_like_wordpress() {
    assert_eq!(
        QueryClass::of(b"  insert into t values (1)"),
        QueryClass::Write { insert: true }
    );
    assert_eq!(
        QueryClass::of(b"REPLACE INTO t VALUES (1)"),
        QueryClass::Write { insert: true }
    );
    assert_eq!(
        QueryClass::of(b"update t set a = 1"),
        QueryClass::Write { insert: false }
    );
    assert_eq!(
        QueryClass::of(b"/* x */ INSERT INTO t VALUES (1)"),
        QueryClass::Read
    );
    assert_eq!(QueryClass::of(b"DROP TABLE t"), QueryClass::Ddl);
    assert_eq!(
        QueryClass::of(b"\tcreate\ttable t (a int)"),
        QueryClass::Ddl
    );
    assert_eq!(QueryClass::of(b"SHOW TABLES"), QueryClass::Read);
    assert_eq!(QueryClass::of(b"INSERT"), QueryClass::Read);
}

#[test]
fn table_from_query_matches_wordpress() {
    let table = "a_test_table_name";
    let more = [
        (
            "`a_test_db`.`another_test_table`",
            "a_test_db.another_test_table",
        ),
        ("a-test-with-dashes", "a-test-with-dashes"),
    ];
    let long = format!(
        "SELECT {} FROM (SELECT * FROM {table}) as subquery",
        "a".repeat(10000)
    );
    let mut queries: Vec<String> = [
        "SELECT * FROM @",
        "SELECT * FROM `@`",
        "SELECT * FROM (SELECT * FROM @) as subquery",
        "INSERT @",
        "INSERT IGNORE @",
        "INSERT IGNORE INTO @",
        "INSERT INTO @",
        "INSERT LOW_PRIORITY @",
        "INSERT DELAYED @",
        "INSERT HIGH_PRIORITY @",
        "INSERT LOW_PRIORITY IGNORE @",
        "INSERT LOW_PRIORITY INTO @",
        "INSERT LOW_PRIORITY IGNORE INTO @",
        "REPLACE @",
        "REPLACE INTO @",
        "REPLACE LOW_PRIORITY @",
        "REPLACE DELAYED @",
        "REPLACE LOW_PRIORITY INTO @",
        "UPDATE LOW_PRIORITY @",
        "UPDATE LOW_PRIORITY IGNORE @",
        "DELETE @",
        "DELETE IGNORE @",
        "DELETE IGNORE FROM @",
        "DELETE FROM @",
        "DELETE LOW_PRIORITY @",
        "DELETE QUICK @",
        "DELETE IGNORE @",
        "DELETE LOW_PRIORITY FROM @",
        "DELETE a FROM @ a",
        "DELETE `a` FROM @ a",
        "EXPLAIN SELECT * FROM @",
        "EXPLAIN EXTENDED SELECT * FROM @",
        "EXPLAIN EXTENDED SELECT * FROM `@`",
        "DESCRIBE @",
        "DESC @",
        "EXPLAIN @",
        "HANDLER @",
        "LOCK TABLE @",
        "LOCK TABLES @",
        "UNLOCK TABLE @",
        "RENAME TABLE @",
        "OPTIMIZE TABLE @",
        "BACKUP TABLE @",
        "RESTORE TABLE @",
        "CHECK TABLE @",
        "CHECKSUM TABLE @",
        "ANALYZE TABLE @",
        "REPAIR TABLE @",
        "TRUNCATE @",
        "TRUNCATE TABLE @",
        "CREATE TABLE @",
        "CREATE TEMPORARY TABLE @",
        "CREATE TABLE IF NOT EXISTS @",
        "ALTER TABLE @",
        "ALTER IGNORE TABLE @",
        "DROP TABLE @",
        "DROP TABLE IF EXISTS @",
        "CREATE INDEX foo(bar(20)) ON @",
        "CREATE UNIQUE INDEX foo(bar(20)) ON @",
        "CREATE FULLTEXT INDEX foo(bar(20)) ON @",
        "CREATE SPATIAL INDEX foo(bar(20)) ON @",
        "DROP INDEX foo ON @",
        "LOAD DATA INFILE 'wp.txt' INTO TABLE @",
        "LOAD DATA LOW_PRIORITY INFILE 'wp.txt' INTO TABLE @",
        "LOAD DATA CONCURRENT INFILE 'wp.txt' INTO TABLE @",
        "LOAD DATA LOW_PRIORITY LOCAL INFILE 'wp.txt' INTO TABLE @",
        "LOAD DATA INFILE 'wp.txt' REPLACE INTO TABLE @",
        "LOAD DATA INFILE 'wp.txt' IGNORE INTO TABLE @",
        "GRANT ALL ON TABLE @",
        "REVOKE ALL ON TABLE @",
        "SHOW COLUMNS FROM @",
        "SHOW FULL COLUMNS FROM @",
        "SHOW CREATE TABLE @",
        "SHOW INDEX FROM @",
        "SET STATEMENT max_statement_time=1 FOR SELECT * FROM @",
    ]
    .iter()
    .map(|q| q.replace('@', table))
    .collect();
    queries.push(long);
    for query in &queries {
        assert_eq!(
            get_table_from_query(query.as_bytes()).as_deref(),
            Some(table.as_bytes()),
            "{query}"
        );
        for (name, expected) in more {
            let q = query.replace(table, name);
            assert_eq!(
                get_table_from_query(q.as_bytes()).as_deref(),
                Some(expected.as_bytes()),
                "{q}"
            );
        }
    }
    assert_eq!(
        get_table_from_query(format!("LOL THIS ISN'T EVEN A QUERY {table}").as_bytes()),
        None
    );
    for (query, expected) in [
        ("SHOW TABLE STATUS WHERE Name = 'test_name'", "test_name"),
        ("SHOW TABLE STATUS WHERE NAME=\"test_name\"", "test_name"),
        ("SHOW TABLES WHERE Name = \"test_name\"", "test_name"),
        ("SHOW FULL TABLES WHERE Name='test_name'", "test_name"),
        ("SHOW TABLE STATUS LIKE 'test\\_prefix\\_%'", "test_prefix_"),
        (
            "SHOW TABLE STATUS LIKE \"test\\_prefix\\_%\"",
            "test_prefix_",
        ),
        ("SHOW TABLES LIKE 'test\\_prefix\\_%'", "test_prefix_"),
        (
            "SHOW FULL TABLES LIKE \"test\\_prefix\\_%\"",
            "test_prefix_",
        ),
    ] {
        assert_eq!(
            get_table_from_query(query.as_bytes()).as_deref(),
            Some(expected.as_bytes()),
            "{query}"
        );
    }
}
