//! MySQL-shaped answers to the statements every engine emulates (`EmulatedOp`), from the schema and the session.

use std::rc::Rc;

use speedyspec_data::{
    ColumnDef, ColumnInfo, DbError, DbValue, EmulatedOp, IndexDef, Introspector, Row, Rows,
};

use crate::catalogue::is_catalogue_table;
use crate::like::mysql_like;
use crate::session::SessionVars;
use crate::{MySqlType, TypeFamily};

fn t(s: &str) -> DbValue {
    DbValue::Text(s.to_string())
}

fn opt(s: Option<&str>) -> DbValue {
    s.map_or(DbValue::Null, t)
}

/// Rows with these column names.
fn rows(names: &[&str], values: Vec<Vec<DbValue>>) -> Rows {
    let shared: Rc<[String]> = names.iter().map(|n| n.to_string()).collect();
    Rows {
        columns: names
            .iter()
            .map(|n| ColumnInfo {
                name: n.to_string(),
                org_name: n.to_string(),
                ..Default::default()
            })
            .collect(),
        rows: values
            .into_iter()
            .map(|v| Row::new(shared.clone(), v))
            .collect(),
    }
}

/// The charset a collation belongs to.
fn charset_of(collation: &str) -> &str {
    collation.split('_').next().unwrap_or(collation)
}

/// The table's indexes, each with MySQL's `Index_type`.
fn typed_indexes(
    introspector: &dyn Introspector,
    table: &str,
) -> Result<Vec<(IndexDef, String)>, DbError> {
    introspector
        .indexes(table)?
        .into_iter()
        .map(|i| {
            let kind = introspector
                .index_type(table, &i.name)?
                .unwrap_or_else(|| "BTREE".into());
            Ok((i, kind))
        })
        .collect()
}

/// `SHOW CREATE TABLE`'s text, in MySQL 8.4's layout.
pub fn render_create_table(
    table: &str,
    columns: &[ColumnDef],
    indexes: &[(IndexDef, String)],
    collation: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for c in columns {
        let mut line = format!("  `{}` {}", c.name, c.sql_type);
        if let Some(col) = c.collation.as_deref()
            && Some(col) != collation
        {
            line.push_str(&format!(" COLLATE {col}"));
        }
        let blob_or_text =
            c.sql_type.ends_with("text") || c.sql_type.ends_with("blob") || c.sql_type == "json";
        if !c.nullable {
            line.push_str(" NOT NULL");
        }
        match c.default.as_deref() {
            Some("CURRENT_TIMESTAMP") => line.push_str(" DEFAULT CURRENT_TIMESTAMP"),
            Some(d) => line.push_str(&format!(" DEFAULT '{}'", d.replace('\'', "''"))),
            None if c.nullable && !blob_or_text => line.push_str(" DEFAULT NULL"),
            None => {}
        }
        if c.extra.contains("on update") {
            line.push_str(" ON UPDATE CURRENT_TIMESTAMP");
        }
        if c.extra == "auto_increment" {
            line.push_str(" AUTO_INCREMENT");
        }
        lines.push(line);
    }
    for (index, kind) in indexes {
        let cols: Vec<String> = index
            .columns
            .iter()
            .map(|(c, n)| match n {
                Some(n) => format!("`{c}`({n})"),
                None => format!("`{c}`"),
            })
            .collect();
        let cols = cols.join(",");
        lines.push(if index.name == "PRIMARY" {
            format!("  PRIMARY KEY ({cols})")
        } else if index.unique {
            format!("  UNIQUE KEY `{}` ({cols})", index.name)
        } else if kind == "FULLTEXT" || kind == "SPATIAL" {
            format!("  {kind} KEY `{}` ({cols})", index.name)
        } else {
            format!("  KEY `{}` ({cols})", index.name)
        });
    }
    let collation = collation.unwrap_or("utf8mb4_0900_ai_ci");
    format!(
        "CREATE TABLE `{table}` (\n{}\n) ENGINE=InnoDB DEFAULT CHARSET={} COLLATE={collation}",
        lines.join(",\n"),
        charset_of(collation)
    )
}

fn user_tables(
    introspector: &dyn Introspector,
    pattern: Option<&str>,
) -> Result<Vec<String>, DbError> {
    let mut tables: Vec<String> = introspector
        .tables(None)?
        .into_iter()
        .filter(|t| !is_catalogue_table(t))
        .filter(|t| pattern.is_none_or(|p| mysql_like(p, t, false)))
        .collect();
    tables.sort();
    Ok(tables)
}

/// The answer MySQL 8.4 gives to `op`, with its column names and order.
pub fn answer_emulated(
    op: &EmulatedOp,
    introspector: &dyn Introspector,
    session: &mut SessionVars,
    found_rows: u64,
) -> Result<Rows, DbError> {
    Ok(match op {
        EmulatedOp::ShowTables { like } => {
            let mut name = format!("Tables_in_{}", session.database());
            if let Some(p) = like {
                name.push_str(&format!(" ({p})"));
            }
            let tables = user_tables(introspector, like.as_deref())?;
            rows(
                &[&name],
                tables.into_iter().map(|n| vec![DbValue::Text(n)]).collect(),
            )
        }
        EmulatedOp::ShowColumns { table, full, like } => {
            let columns: Vec<ColumnDef> = introspector
                .columns(table)?
                .into_iter()
                .filter(|c| like.as_deref().is_none_or(|p| mysql_like(p, &c.name, true)))
                .collect();
            let null = |c: &ColumnDef| t(if c.nullable { "YES" } else { "NO" });
            if *full {
                rows(
                    &[
                        "Field",
                        "Type",
                        "Collation",
                        "Null",
                        "Key",
                        "Default",
                        "Extra",
                        "Privileges",
                        "Comment",
                    ],
                    columns
                        .iter()
                        .map(|c| {
                            vec![
                                t(&c.name),
                                t(&c.sql_type),
                                opt(c.collation.as_deref()),
                                null(c),
                                t(&c.key),
                                opt(c.default.as_deref()),
                                t(&c.extra),
                                t("select,insert,update,references"),
                                t(""),
                            ]
                        })
                        .collect(),
                )
            } else {
                rows(
                    &["Field", "Type", "Null", "Key", "Default", "Extra"],
                    columns
                        .iter()
                        .map(|c| {
                            vec![
                                t(&c.name),
                                t(&c.sql_type),
                                null(c),
                                t(&c.key),
                                opt(c.default.as_deref()),
                                t(&c.extra),
                            ]
                        })
                        .collect(),
                )
            }
        }
        EmulatedOp::ShowIndex { table } => {
            let columns = introspector.columns(table)?;
            let nullable = |name: &str| {
                columns
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(name) && c.nullable)
            };
            let mut out = Vec::new();
            for (index, kind) in typed_indexes(introspector, table)? {
                let typed = kind != "BTREE";
                for (seq, (column, sub)) in index.columns.iter().enumerate() {
                    out.push(vec![
                        t(table),
                        DbValue::Int(i64::from(!index.unique)),
                        t(&index.name),
                        DbValue::Int(seq as i64 + 1),
                        t(column),
                        // MySQL leaves a FULLTEXT or SPATIAL index's Collation empty.
                        if typed { DbValue::Null } else { t("A") },
                        DbValue::Int(0),
                        sub.map_or(DbValue::Null, |n| DbValue::Int(i64::from(n))),
                        DbValue::Null,
                        t(if nullable(column) { "YES" } else { "" }),
                        t(&kind),
                        t(""),
                        t(""),
                        t("YES"),
                        DbValue::Null,
                    ]);
                }
            }
            rows(
                &[
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
                    "Expression",
                ],
                out,
            )
        }
        EmulatedOp::ShowCreateTable { table } => {
            let text = render_create_table(
                table,
                &introspector.columns(table)?,
                &typed_indexes(introspector, table)?,
                introspector.table_collation(table)?.as_deref(),
            );
            rows(
                &["Table", "Create Table"],
                vec![vec![t(table), DbValue::Text(text)]],
            )
        }
        EmulatedOp::ShowVariables { like } => rows(
            &["Variable_name", "Value"],
            session
                .like(like.as_deref())
                .into_iter()
                .map(|(k, v)| vec![DbValue::Text(k), DbValue::Text(v)])
                .collect(),
        ),
        EmulatedOp::ShowTableStatus { like } => {
            let mut out = Vec::new();
            for table in user_tables(introspector, like.as_deref())? {
                let collation = introspector.table_collation(&table)?;
                let zero = || DbValue::Int(0);
                out.push(vec![
                    DbValue::Text(table),
                    t("InnoDB"),
                    DbValue::Int(10),
                    t("Dynamic"),
                    zero(),
                    zero(),
                    zero(),
                    zero(),
                    zero(),
                    zero(),
                    DbValue::Null,
                    DbValue::Null,
                    DbValue::Null,
                    DbValue::Null,
                    opt(collation.as_deref()),
                    DbValue::Null,
                    t(""),
                    t(""),
                ]);
            }
            rows(
                &[
                    "Name",
                    "Engine",
                    "Version",
                    "Row_format",
                    "Rows",
                    "Avg_row_length",
                    "Data_length",
                    "Max_data_length",
                    "Index_length",
                    "Data_free",
                    "Auto_increment",
                    "Create_time",
                    "Update_time",
                    "Check_time",
                    "Collation",
                    "Checksum",
                    "Create_options",
                    "Comment",
                ],
                out,
            )
        }
        EmulatedOp::SetSession { name, value } => {
            if name == "names" {
                let mut parts = value.split_whitespace();
                let charset = parts.next().unwrap_or("utf8mb4").to_string();
                session.set_names(&charset, parts.next());
            } else {
                session.set(name, value);
            }
            Rows::default()
        }
        EmulatedOp::Noop => Rows::default(),
        EmulatedOp::FoundRows => rows(
            &["FOUND_ROWS()"],
            vec![vec![DbValue::Int(
                i64::try_from(found_rows).unwrap_or(i64::MAX),
            )]],
        ),
    })
}

/// The family of a displayed MySQL type, when it parses.
pub fn family_of(sql_type: &str) -> Option<TypeFamily> {
    MySqlType::parse(sql_type).ok().map(|t| t.family())
}
