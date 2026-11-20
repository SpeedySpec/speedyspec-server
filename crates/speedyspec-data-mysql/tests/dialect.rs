use speedyspec_data::{Dialect, DialectKind, Prepared};
use speedyspec_data_mysql::MySqlDialect;
use speedyspec_data_query::{Query, parse_one};

fn one(prepared: Prepared) -> speedyspec_data::EmittedSql {
    match prepared {
        Prepared::Sql(mut sql) if sql.len() == 1 => sql.remove(0),
        other => panic!("expected one SQL statement, got {other:?}"),
    }
}

#[test]
fn emits_parsed_sql() {
    let stmt = parse_one("SELECT `a` FROM `t` LIMIT 0, 10").unwrap();
    assert_eq!(MySqlDialect.kind(), DialectKind::MySql);
    let emitted = one(MySqlDialect.prepare(&stmt).unwrap());
    assert_eq!(parse_one(&emitted.sql).unwrap().tree(), stmt.tree());
    assert!(emitted.params.is_empty());
}

#[test]
fn placeholders_stay_question_marks() {
    let stmt = Query::table("wp_options")
        .where_eq("option_name", "siteurl")
        .build()
        .unwrap();
    let emitted = one(MySqlDialect.prepare(&stmt).unwrap());
    assert_eq!(
        emitted.sql,
        "SELECT * FROM wp_options WHERE option_name = ?"
    );
    assert_eq!(emitted.params, stmt.params());
}

#[test]
fn backslash_escapes_survive_emission() {
    let stmt =
        parse_one(r#"SELECT 'C:\\new', 'it\'s', 'a\nb', 'x\0y', '50\%', "dq\\" FROM t"#).unwrap();
    let emitted = one(MySqlDialect.prepare(&stmt).unwrap());
    // Parsing the emitted text again gives the same values: nothing was unescaped twice.
    assert_eq!(
        parse_one(&emitted.sql).unwrap().tree(),
        stmt.tree(),
        "{}",
        emitted.sql
    );
    assert!(emitted.sql.contains(r#"'C:\\new'"#), "{}", emitted.sql);
}
