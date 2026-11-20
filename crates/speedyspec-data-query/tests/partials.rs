use speedyspec_data::DbError;
use speedyspec_data_query::{Clause, Partial, Query};

#[test]
fn spec_example() {
    let stmt = Query::parse("SELECT * FROM wp_posts WHERE post_status = 'publish'")
        .unwrap()
        .and_where(Partial::expr("post_type = 'page'").unwrap())
        .replace(
            Clause::OrderBy,
            Partial::order_by("post_date DESC").unwrap(),
        )
        .limit(10)
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT * FROM wp_posts WHERE post_status = 'publish' AND post_type = 'page' ORDER BY post_date DESC LIMIT 10"
    );
}

#[test]
fn or_where_parenthesises() {
    let stmt = Query::parse("SELECT * FROM t WHERE a = 1 AND b = 2")
        .unwrap()
        .or_where(Partial::expr("c = 3").unwrap())
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT * FROM t WHERE (a = 1 AND b = 2) OR c = 3"
    );
}

#[test]
fn remove_clause() {
    let stmt = Query::parse("SELECT * FROM t ORDER BY a LIMIT 5")
        .unwrap()
        .remove(Clause::OrderBy)
        .remove(Clause::Limit)
        .build()
        .unwrap();
    assert_eq!(stmt.tree().to_string(), "SELECT * FROM t");
}

#[test]
fn replace_projection_group_and_join() {
    let stmt = Query::parse("SELECT * FROM wp_posts p")
        .unwrap()
        .replace(
            Clause::Projection,
            Partial::projection("p.ID, COUNT(*) AS n").unwrap(),
        )
        .join(Partial::join("INNER JOIN wp_postmeta m ON m.post_id = p.ID").unwrap())
        .replace(Clause::GroupBy, Partial::group_by("p.ID").unwrap())
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT p.ID, COUNT(*) AS n FROM wp_posts p INNER JOIN wp_postmeta m ON m.post_id = p.ID GROUP BY p.ID"
    );
}

#[test]
fn partials_on_update_where() {
    let stmt = Query::parse("UPDATE t SET a = 1")
        .unwrap()
        .and_where(Partial::expr("b = 2").unwrap())
        .build()
        .unwrap();
    assert_eq!(stmt.tree().to_string(), "UPDATE t SET a = 1 WHERE b = 2");
}

#[test]
fn bad_partial_is_parse_error() {
    assert!(matches!(
        Partial::expr("AND AND"),
        Err(DbError::Parse { .. })
    ));
}

#[test]
fn partial_on_insert_is_unsupported() {
    let result = Query::parse("INSERT INTO t (a) VALUES (1)")
        .unwrap()
        .and_where(Partial::expr("a = 1").unwrap())
        .build();
    assert!(matches!(result, Err(DbError::Unsupported { .. })));
}

#[test]
fn and_where_keeps_or_precedence() {
    let stmt = Query::parse("SELECT * FROM t WHERE a = 1 OR b = 2")
        .unwrap()
        .and_where(Partial::expr("c = 3 OR d = 4").unwrap())
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT * FROM t WHERE (a = 1 OR b = 2) AND (c = 3 OR d = 4)"
    );
}

#[test]
fn partial_cannot_smuggle_other_clauses() {
    assert!(matches!(
        Partial::order_by("a LIMIT 1"),
        Err(DbError::Parse { .. })
    ));
    assert!(matches!(
        Partial::order_by("a; DROP TABLE t"),
        Err(DbError::Parse { .. })
    ));
}

#[test]
fn and_where_keeps_xor_precedence() {
    let stmt = Query::parse("SELECT * FROM t WHERE a = 1 XOR b = 2")
        .unwrap()
        .and_where(Partial::expr("c = 3").unwrap())
        .build()
        .unwrap();
    assert_eq!(
        stmt.tree().to_string(),
        "SELECT * FROM t WHERE (a = 1 XOR b = 2) AND c = 3"
    );
}
