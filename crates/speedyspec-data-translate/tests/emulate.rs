use speedyspec_data::{DbValue, EmulatedOp, Rows};
use speedyspec_data_translate::testing::StaticIntrospector;
use speedyspec_data_translate::{SessionVars, answer_emulated};

const WP_OPTIONS: &str = "CREATE TABLE wp_options ( option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY  (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";
const WP_POSTMETA: &str = "CREATE TABLE wp_postmeta ( meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY  (meta_id), KEY post_id (post_id), KEY meta_key (meta_key(191)) ) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci";

fn schema() -> StaticIntrospector {
    StaticIntrospector::from_mysql_ddl(&[WP_OPTIONS, WP_POSTMETA])
}

fn session() -> SessionVars {
    SessionVars::mysql_defaults("wordpress", "8.4.0")
}

fn answer(op: EmulatedOp) -> Rows {
    answer_emulated(&op, &schema(), &mut session(), 0).unwrap()
}

fn t(s: &str) -> DbValue {
    DbValue::Text(s.into())
}

fn names(rows: &Rows) -> Vec<String> {
    rows.columns.iter().map(|c| c.name.clone()).collect()
}

fn values(rows: &Rows, i: usize) -> Vec<DbValue> {
    rows.rows[i].values().to_vec()
}

#[test]
fn show_tables_like() {
    let rows = answer(EmulatedOp::ShowTables {
        like: Some(r"wp\_%".into()),
    });
    assert_eq!(names(&rows), [r"Tables_in_wordpress (wp\_%)"]);
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(values(&rows, 0), [t("wp_options")]);
    assert_eq!(values(&rows, 1), [t("wp_postmeta")]);
    assert_eq!(
        answer(EmulatedOp::ShowTables {
            like: Some("WP%".into())
        })
        .rows
        .len(),
        0,
        "table names are case-sensitive"
    );
    assert_eq!(
        names(&answer(EmulatedOp::ShowTables { like: None })),
        ["Tables_in_wordpress"]
    );
}

#[test]
fn show_full_columns_row() {
    let rows = answer(EmulatedOp::ShowColumns {
        table: "wp_options".into(),
        full: true,
        like: None,
    });
    assert_eq!(
        names(&rows),
        [
            "Field",
            "Type",
            "Collation",
            "Null",
            "Key",
            "Default",
            "Extra",
            "Privileges",
            "Comment"
        ]
    );
    assert_eq!(
        values(&rows, 0),
        [
            t("option_id"),
            t("bigint unsigned"),
            DbValue::Null,
            t("NO"),
            t("PRI"),
            DbValue::Null,
            t("auto_increment"),
            t("select,insert,update,references"),
            t("")
        ]
    );
    assert_eq!(values(&rows, 1)[2], t("utf8mb4_unicode_520_ci"));
}

#[test]
fn describe_postmeta() {
    let rows = answer(EmulatedOp::ShowColumns {
        table: "wp_postmeta".into(),
        full: false,
        like: None,
    });
    assert_eq!(
        names(&rows),
        ["Field", "Type", "Null", "Key", "Default", "Extra"]
    );
    assert_eq!(
        values(&rows, 0),
        [
            t("meta_id"),
            t("bigint unsigned"),
            t("NO"),
            t("PRI"),
            DbValue::Null,
            t("auto_increment")
        ]
    );
    assert_eq!(
        values(&rows, 1),
        [
            t("post_id"),
            t("bigint unsigned"),
            t("NO"),
            t("MUL"),
            t("0"),
            t("")
        ]
    );
    assert_eq!(
        values(&rows, 2),
        [
            t("meta_key"),
            t("varchar(255)"),
            t("YES"),
            t("MUL"),
            DbValue::Null,
            t("")
        ]
    );
    assert_eq!(
        values(&rows, 3),
        [
            t("meta_value"),
            t("longtext"),
            t("YES"),
            t(""),
            DbValue::Null,
            t("")
        ]
    );
    let one = answer(EmulatedOp::ShowColumns {
        table: "wp_postmeta".into(),
        full: false,
        like: Some("meta\\_k%".into()),
    });
    assert_eq!(one.rows.len(), 1);
}

#[test]
fn show_index_sub_part() {
    let rows = answer(EmulatedOp::ShowIndex {
        table: "wp_postmeta".into(),
    });
    assert_eq!(
        names(&rows),
        [
            "Table",
            "Non_unique",
            "Key_name",
            "Seq_in_index",
            "Column_name",
            "Collation",
            "Cardinality",
            "Sub_part",
            "Packed",
            "Null",
            "Index_type",
            "Comment",
            "Index_comment",
            "Visible",
            "Expression"
        ]
    );
    let row = rows
        .rows
        .iter()
        .find(|r| r.get("Key_name") == Some(&t("meta_key")))
        .expect("the meta_key index");
    assert_eq!(row.get("Non_unique"), Some(&DbValue::Int(1)));
    assert_eq!(row.get("Sub_part"), Some(&DbValue::Int(191)));
    assert_eq!(row.get("Null"), Some(&t("YES")));
    let primary = &rows.rows[0];
    assert_eq!(
        (
            primary.get("Key_name"),
            primary.get("Non_unique"),
            primary.get("Null")
        ),
        (Some(&t("PRIMARY")), Some(&DbValue::Int(0)), Some(&t("")))
    );
}

#[test]
fn show_create_table_text() {
    let rows = answer(EmulatedOp::ShowCreateTable {
        table: "wp_options".into(),
    });
    assert_eq!(names(&rows), ["Table", "Create Table"]);
    assert_eq!(
        values(&rows, 0)[1],
        t(
            "CREATE TABLE `wp_options` (\n  `option_id` bigint unsigned NOT NULL AUTO_INCREMENT,\n  `option_name` varchar(191) NOT NULL DEFAULT '',\n  `option_value` longtext NOT NULL,\n  `autoload` varchar(20) NOT NULL DEFAULT 'yes',\n  PRIMARY KEY (`option_id`),\n  UNIQUE KEY `option_name` (`option_name`),\n  KEY `autoload` (`autoload`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
        )
    );
    let meta = answer(EmulatedOp::ShowCreateTable {
        table: "wp_postmeta".into(),
    });
    let text = match &values(&meta, 0)[1] {
        DbValue::Text(s) => s.clone(),
        other => panic!("{other:?}"),
    };
    assert!(
        text.contains("`meta_key` varchar(255) DEFAULT NULL,"),
        "{text}"
    );
    assert!(text.contains("`meta_value` longtext,"), "{text}");
    assert!(text.contains("KEY `meta_key` (`meta_key`(191))"), "{text}");
}

#[test]
fn set_names_and_sql_mode() {
    let schema = schema();
    let mut session = session();
    answer_emulated(
        &EmulatedOp::SetSession {
            name: "names".into(),
            value: "utf8mb4 utf8mb4_unicode_520_ci".into(),
        },
        &schema,
        &mut session,
        0,
    )
    .unwrap();
    let rows = answer_emulated(
        &EmulatedOp::ShowVariables {
            like: Some("collation_connection".into()),
        },
        &schema,
        &mut session,
        0,
    )
    .unwrap();
    assert_eq!(names(&rows), ["Variable_name", "Value"]);
    assert_eq!(
        values(&rows, 0),
        [t("collation_connection"), t("utf8mb4_unicode_520_ci")]
    );
    answer_emulated(
        &EmulatedOp::SetSession {
            name: "sql_mode".into(),
            value: "NO_ENGINE_SUBSTITUTION".into(),
        },
        &schema,
        &mut session,
        0,
    )
    .unwrap();
    assert_eq!(session.get("SQL_MODE"), Some("NO_ENGINE_SUBSTITUTION"));
    session.reset();
    assert_eq!(
        session.get("collation_connection"),
        Some("utf8mb4_0900_ai_ci")
    );
    assert!(
        session
            .get("sql_mode")
            .unwrap()
            .starts_with("ONLY_FULL_GROUP_BY")
    );
}

#[test]
fn found_rows_value() {
    let rows = answer_emulated(&EmulatedOp::FoundRows, &schema(), &mut session(), 15).unwrap();
    assert_eq!(names(&rows), ["FOUND_ROWS()"]);
    assert_eq!(values(&rows, 0), [DbValue::Int(15)]);
}

#[test]
fn show_table_status_columns() {
    let rows = answer(EmulatedOp::ShowTableStatus {
        like: Some("wp_options".into()),
    });
    assert_eq!(rows.columns.len(), 18);
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0].get("Engine"), Some(&t("InnoDB")));
    assert_eq!(
        rows.rows[0].get("Collation"),
        Some(&t("utf8mb4_unicode_520_ci"))
    );
}

#[test]
fn noop_is_empty() {
    assert_eq!(answer(EmulatedOp::Noop), Rows::default());
}

#[test]
fn mysql_like_rules() {
    use speedyspec_data_translate::mysql_like;
    assert!(mysql_like(r"wp\_%", "wp_options", false));
    assert!(!mysql_like(r"wp\_%", "wpxoptions", false));
    assert!(mysql_like("wp_%", "wpxoptions", false));
    assert!(mysql_like("%a%b%", "xxaxxbxx", false));
    assert!(!mysql_like("%a%b", "xxaxxbxx", false));
    assert!(mysql_like("A%", "abc", true));
    assert!(!mysql_like("A%", "abc", false));
    assert!(mysql_like(r"100\%", "100%", false));
    assert!(mysql_like("", "", false));
}
