//! The MySQL schema catalogue (Metadata Mapping): what MySQL would report for each table, kept in three
//! `_speedyspec_*` tables beside the engine's own, so `SHOW COLUMNS`, `SHOW INDEX` and `DESCRIBE` answer as MySQL 8.4.

use speedyspec_data::{ColumnDef, DbError, DbValue, IndexDef, Row, Rows, Statement};
use speedyspec_data_query::parse_one;
use sqlparser::ast::{
    ColumnDef as AstColumn, ColumnOption, CreateTableOptions, Expr, IndexColumn, SqlOption,
    Statement as Ast, TableConstraint,
};

use crate::names::{literal_text, object_name, sql_ident, sql_literal, sql_opt};
use crate::{MySqlType, TypeFamily};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueColumn {
    pub name: String,
    pub column_type: MySqlType,
    pub nullable: bool,
    pub default: Option<String>,
    pub extra: String,
    pub collation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    Primary,
    Unique,
    Index,
    Fulltext,
}

impl IndexKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            IndexKind::Primary => "PRIMARY",
            IndexKind::Unique => "UNIQUE",
            IndexKind::Index => "INDEX",
            IndexKind::Fulltext => "FULLTEXT",
        }
    }

    pub fn parse(text: &str) -> IndexKind {
        match text {
            "PRIMARY" => IndexKind::Primary,
            "UNIQUE" => IndexKind::Unique,
            "FULLTEXT" => IndexKind::Fulltext,
            _ => IndexKind::Index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueIndex {
    pub name: String,
    pub kind: IndexKind,
    pub parts: Vec<(String, Option<u32>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueTable {
    pub name: String,
    pub collation: Option<String>,
    pub columns: Vec<CatalogueColumn>,
    pub indexes: Vec<CatalogueIndex>,
}

/// Whether `name` is one of the catalogue's own tables.
pub fn is_catalogue_table(name: &str) -> bool {
    name.starts_with("_speedyspec_")
}

/// The engine-wide name of a MySQL index (SQLite's and PostgreSQL's index names are schema-wide, MySQL's per table).
pub fn index_object_name(table: &str, index: &str) -> String {
    format!("{table}__{index}")
}

/// The catalogue's three tables, in MySQL form (translated like any DDL).
pub fn catalogue_ddl() -> Vec<Statement> {
    [
        "CREATE TABLE IF NOT EXISTS _speedyspec_tables (table_name varchar(191) NOT NULL, `collation` varchar(64) NULL, PRIMARY KEY (table_name))",
        "CREATE TABLE IF NOT EXISTS _speedyspec_columns (table_name varchar(191) NOT NULL, ordinal int NOT NULL, column_name varchar(64) NOT NULL, column_type text NOT NULL, nullable tinyint NOT NULL, column_default text NULL, extra varchar(64) NOT NULL, `collation` varchar(64) NULL, PRIMARY KEY (table_name, column_name))",
        "CREATE TABLE IF NOT EXISTS _speedyspec_indexes (table_name varchar(191) NOT NULL, index_name varchar(64) NOT NULL, kind varchar(16) NOT NULL, seq int NOT NULL, column_name varchar(64) NOT NULL, sub_part int NULL, PRIMARY KEY (table_name, index_name, seq))",
    ]
    .iter()
    .map(|sql| parse_one(sql).expect("the catalogue's own DDL parses"))
    .collect()
}

/// MySQL's `Key` column: `PRI` in the primary key, `UNI` the first column of a single-column unique index, `MUL` the
/// first column of any other index; the highest wins.
pub fn column_key(column: &str, indexes: &[CatalogueIndex]) -> &'static str {
    let mut key = "";
    for index in indexes {
        let first = index
            .parts
            .first()
            .is_some_and(|(c, _)| c.eq_ignore_ascii_case(column));
        let found = match index.kind {
            IndexKind::Primary
                if index
                    .parts
                    .iter()
                    .any(|(c, _)| c.eq_ignore_ascii_case(column)) =>
            {
                "PRI"
            }
            IndexKind::Unique if first && index.parts.len() == 1 => "UNI",
            _ if first => "MUL",
            _ => "",
        };
        let rank = |k: &str| match k {
            "PRI" => 3,
            "UNI" => 2,
            "MUL" => 1,
            _ => 0,
        };
        if rank(found) > rank(key) {
            key = found;
        }
    }
    key
}

fn default_collation(charset: &str) -> Option<String> {
    Some(
        match charset.to_ascii_lowercase().as_str() {
            "utf8mb4" => "utf8mb4_0900_ai_ci",
            "utf8" | "utf8mb3" => "utf8mb3_general_ci",
            "latin1" => "latin1_swedish_ci",
            "ascii" => "ascii_general_ci",
            "binary" => return None,
            _ => return None,
        }
        .to_string(),
    )
}

/// The table's `COLLATE`, or its `CHARSET`'s default collation.
fn table_collation(options: &CreateTableOptions) -> Option<String> {
    let list = match options {
        CreateTableOptions::Plain(list)
        | CreateTableOptions::With(list)
        | CreateTableOptions::Options(list) => list,
        _ => return None,
    };
    let mut collation = None;
    let mut charset = None;
    for option in list {
        if let SqlOption::KeyValue { key, value } = option {
            let key = object_name(key).to_ascii_uppercase();
            let value = literal_text(value);
            if key.ends_with("COLLATE") {
                collation = Some(value);
            } else if key.ends_with("CHARSET") || key.ends_with("CHARACTER SET") {
                charset = Some(value);
            }
        }
    }
    collation.or_else(|| charset.as_deref().and_then(default_collation))
}

fn is_current_timestamp(expr: &Expr) -> bool {
    let text = expr.to_string().to_ascii_uppercase();
    text.starts_with("CURRENT_TIMESTAMP")
        || text.starts_with("NOW(")
        || text.starts_with("LOCALTIMESTAMP")
}

/// A catalogue column from a column definition; `table_collation` is what text columns inherit.
pub(crate) fn column_of(
    def: &AstColumn,
    table_collation: Option<&str>,
) -> Result<(CatalogueColumn, bool, bool), DbError> {
    let column_type = MySqlType::from_ast(&def.data_type)?;
    let mut nullable = true;
    let mut default = None;
    let mut auto_increment = false;
    let mut generated = false;
    let mut on_update = false;
    let mut collation = None;
    let (mut primary, mut unique) = (false, false);
    for option in &def.options {
        match &option.option {
            ColumnOption::NotNull => nullable = false,
            ColumnOption::Null => nullable = true,
            ColumnOption::Default(expr) => {
                if matches!(expr, Expr::Value(v) if matches!(v.value, sqlparser::ast::Value::Null))
                {
                    default = None;
                } else if is_current_timestamp(expr) {
                    default = Some("CURRENT_TIMESTAMP".to_string());
                    generated = true;
                } else {
                    default = Some(literal_text(expr));
                }
            }
            ColumnOption::OnUpdate(_) => on_update = true,
            ColumnOption::Collation(name) => collation = Some(object_name(name)),
            ColumnOption::PrimaryKey(_) => primary = true,
            ColumnOption::Unique(_) => unique = true,
            ColumnOption::DialectSpecific(tokens)
                if tokens
                    .iter()
                    .any(|t| t.to_string().eq_ignore_ascii_case("AUTO_INCREMENT")) =>
            {
                auto_increment = true;
            }
            _ => {}
        }
    }
    if primary {
        nullable = false;
    }
    let extra = match (auto_increment, generated, on_update) {
        (true, _, _) => "auto_increment".to_string(),
        (false, true, true) => "DEFAULT_GENERATED on update CURRENT_TIMESTAMP".to_string(),
        (false, true, false) => "DEFAULT_GENERATED".to_string(),
        (false, false, true) => "on update CURRENT_TIMESTAMP".to_string(),
        (false, false, false) => String::new(),
    };
    let text = matches!(column_type.family(), TypeFamily::Text | TypeFamily::Enum);
    let collation = if text {
        collation.or_else(|| table_collation.map(str::to_string))
    } else {
        None
    };
    Ok((
        CatalogueColumn {
            name: def.name.value.clone(),
            column_type,
            nullable,
            default,
            extra,
            collation,
        },
        primary,
        unique,
    ))
}

/// An index column: a name, or `name(n)` for a prefix (which sqlparser reads as a call).
pub(crate) fn index_part(column: &IndexColumn) -> Option<(String, Option<u32>)> {
    match &column.column.expr {
        Expr::Identifier(ident) => Some((ident.value.clone(), None)),
        Expr::Function(f) => {
            let length = match &f.args {
                sqlparser::ast::FunctionArguments::List(list) => match list.args.as_slice() {
                    [
                        sqlparser::ast::FunctionArg::Unnamed(
                            sqlparser::ast::FunctionArgExpr::Expr(e),
                        ),
                    ] => literal_text(e).parse().ok(),
                    _ => None,
                },
                _ => None,
            };
            Some((object_name(&f.name), length))
        }
        _ => None,
    }
}

pub(crate) fn parts(columns: &[IndexColumn]) -> Vec<(String, Option<u32>)> {
    columns.iter().filter_map(index_part).collect()
}

/// MySQL's name for an unnamed index: its first column, then `_2`, `_3` on a clash.
pub(crate) fn unnamed_index_name(parts: &[(String, Option<u32>)], taken: &[String]) -> String {
    let base = parts
        .first()
        .map(|(c, _)| c.clone())
        .unwrap_or_else(|| "index".into());
    if !taken.iter().any(|t| t.eq_ignore_ascii_case(&base)) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|n| !taken.iter().any(|t| t.eq_ignore_ascii_case(n)))
        .expect("a free name")
}

/// What MySQL would record for a `CREATE TABLE`.
pub fn catalogue_table(create: &Ast) -> Result<CatalogueTable, DbError> {
    let Ast::CreateTable(create) = create else {
        return Err(DbError::Parse {
            message: "not a CREATE TABLE".into(),
            line: 1,
            column: 1,
        });
    };
    let collation = table_collation(&create.table_options);
    let mut columns = Vec::new();
    let mut primary: Vec<(String, Option<u32>)> = Vec::new();
    let mut indexes: Vec<CatalogueIndex> = Vec::new();
    let mut column_unique = Vec::new();
    for def in &create.columns {
        let (column, is_primary, is_unique) = column_of(def, collation.as_deref())?;
        if is_primary {
            primary.push((column.name.clone(), None));
        }
        if is_unique {
            column_unique.push(column.name.clone());
        }
        columns.push(column);
    }
    let taken =
        |indexes: &[CatalogueIndex]| indexes.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
    for constraint in &create.constraints {
        let (kind, name, cols) = match constraint {
            TableConstraint::PrimaryKey(pk) => {
                primary = parts(&pk.columns);
                continue;
            }
            TableConstraint::Unique(u) => (
                IndexKind::Unique,
                u.index_name.clone().or(u.name.clone()),
                parts(&u.columns),
            ),
            TableConstraint::Index(i) => (IndexKind::Index, i.name.clone(), parts(&i.columns)),
            TableConstraint::FulltextOrSpatial(f) if f.fulltext => (
                IndexKind::Fulltext,
                f.opt_index_name.clone(),
                parts(&f.columns),
            ),
            TableConstraint::FulltextOrSpatial(f) => (
                IndexKind::Index,
                f.opt_index_name.clone(),
                parts(&f.columns),
            ),
            _ => continue,
        };
        let name = match name {
            Some(n) => n.value,
            None => unnamed_index_name(&cols, &taken(&indexes)),
        };
        indexes.push(CatalogueIndex {
            name,
            kind,
            parts: cols,
        });
    }
    for column in column_unique {
        let name = unnamed_index_name(&[(column.clone(), None)], &taken(&indexes));
        indexes.push(CatalogueIndex {
            name,
            kind: IndexKind::Unique,
            parts: vec![(column, None)],
        });
    }
    for (name, _) in &primary {
        if let Some(c) = columns
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
        {
            c.nullable = false;
        }
    }
    if !primary.is_empty() {
        indexes.insert(
            0,
            CatalogueIndex {
                name: "PRIMARY".into(),
                kind: IndexKind::Primary,
                parts: primary,
            },
        );
    }
    Ok(CatalogueTable {
        name: object_name(&create.name),
        collation,
        columns,
        indexes,
    })
}

impl CatalogueTable {
    pub fn column_defs(&self) -> Vec<ColumnDef> {
        self.columns
            .iter()
            .map(|c| ColumnDef {
                name: c.name.clone(),
                sql_type: c.column_type.display(),
                nullable: c.nullable,
                key: column_key(&c.name, &self.indexes).to_string(),
                default: c.default.clone(),
                extra: c.extra.clone(),
                collation: c.collation.clone(),
            })
            .collect()
    }

    pub fn index_defs(&self) -> Vec<IndexDef> {
        self.indexes
            .iter()
            .map(|i| IndexDef {
                name: i.name.clone(),
                unique: matches!(i.kind, IndexKind::Primary | IndexKind::Unique),
                columns: i.parts.clone(),
            })
            .collect()
    }

    /// The table as a MySQL `CREATE TABLE` (for a table rebuild).
    pub fn to_create_table(&self) -> Statement {
        let mut items: Vec<String> = self.columns.iter().map(column_sql).collect();
        for index in &self.indexes {
            let cols = parts_sql(&index.parts);
            items.push(match index.kind {
                IndexKind::Primary => format!("PRIMARY KEY ({cols})"),
                IndexKind::Unique => format!("UNIQUE KEY {} ({cols})", sql_ident(&index.name)),
                IndexKind::Index => format!("KEY {} ({cols})", sql_ident(&index.name)),
                IndexKind::Fulltext => format!("FULLTEXT KEY {} ({cols})", sql_ident(&index.name)),
            });
        }
        let collate = self
            .collation
            .as_ref()
            .map(|c| format!(" COLLATE {c}"))
            .unwrap_or_default();
        parse_one(&format!(
            "CREATE TABLE {} ({}){collate}",
            sql_ident(&self.name),
            items.join(", ")
        ))
        .expect("a catalogue table prints as a CREATE TABLE that parses")
    }
}

pub(crate) fn parts_sql(parts: &[(String, Option<u32>)]) -> String {
    parts
        .iter()
        .map(|(c, n)| match n {
            Some(n) => format!("{}({n})", sql_ident(c)),
            None => sql_ident(c),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A catalogue column as a MySQL column definition.
pub(crate) fn column_sql(c: &CatalogueColumn) -> String {
    let mut sql = format!("{} {}", sql_ident(&c.name), c.column_type.display());
    if let Some(collation) = &c.collation {
        sql.push_str(&format!(" COLLATE {collation}"));
    }
    sql.push_str(if c.nullable { " NULL" } else { " NOT NULL" });
    match c.default.as_deref() {
        Some("CURRENT_TIMESTAMP") => sql.push_str(" DEFAULT CURRENT_TIMESTAMP"),
        Some(d) => sql.push_str(&format!(" DEFAULT {}", sql_literal(d))),
        None => {}
    }
    if c.extra.contains("on update") {
        sql.push_str(" ON UPDATE CURRENT_TIMESTAMP");
    }
    if c.extra == "auto_increment" {
        sql.push_str(" AUTO_INCREMENT");
    }
    sql
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnPosition {
    /// At the end (`ADD COLUMN`); for `CHANGE`/`MODIFY`, where the column already is.
    Last,
    First,
    After(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaChange {
    CreateTable(CatalogueTable),
    DropTable(String),
    RenameTable {
        from: String,
        to: String,
    },
    AddColumn {
        table: String,
        column: CatalogueColumn,
        position: ColumnPosition,
    },
    ChangeColumn {
        table: String,
        old_name: String,
        column: CatalogueColumn,
        position: ColumnPosition,
    },
    DropColumn {
        table: String,
        column: String,
    },
    AddIndex {
        table: String,
        index: CatalogueIndex,
    },
    DropIndex {
        table: String,
        name: String,
    },
    SetCollation {
        table: String,
        collation: String,
    },
}

fn stmt(sql: String) -> Statement {
    parse_one(&sql).unwrap_or_else(|e| panic!("a catalogue write parses: {sql}: {e}"))
}

const COLUMN_FIELDS: &str =
    "table_name, ordinal, column_name, column_type, nullable, column_default, extra, `collation`";

fn column_values(table: &str, ordinal: &str, c: &CatalogueColumn, collation: String) -> String {
    format!(
        "{}, {ordinal}, {}, {}, {}, {}, {}, {collation}",
        sql_literal(table),
        sql_literal(&c.name),
        sql_literal(&c.column_type.display()),
        i32::from(c.nullable),
        sql_opt(c.default.as_deref()),
        sql_literal(&c.extra),
    )
}

/// A text column's collation, or the table's when it names none.
fn collation_sql(table: &str, c: &CatalogueColumn) -> String {
    match (&c.collation, c.column_type.family()) {
        (Some(collation), _) => sql_literal(collation),
        (None, TypeFamily::Text | TypeFamily::Enum) => format!(
            "(SELECT `collation` FROM _speedyspec_tables WHERE table_name = {})",
            sql_literal(table)
        ),
        (None, _) => "NULL".into(),
    }
}

fn index_rows(table: &str, index: &CatalogueIndex) -> Option<Statement> {
    if index.parts.is_empty() {
        return None;
    }
    let rows: Vec<String> = index
        .parts
        .iter()
        .enumerate()
        .map(|(i, (column, sub))| {
            format!(
                "({}, {}, {}, {}, {}, {})",
                sql_literal(table),
                sql_literal(&index.name),
                sql_literal(index.kind.as_str()),
                i + 1,
                sql_literal(column),
                sub.map_or_else(|| "NULL".to_string(), |n| n.to_string())
            )
        })
        .collect();
    Some(stmt(format!(
        "INSERT INTO _speedyspec_indexes (table_name, index_name, kind, seq, column_name, sub_part) VALUES {}",
        rows.join(", ")
    )))
}

/// The ordinal of `column` in `table`, as a subquery.
fn ordinal_of(table: &str, column: &str) -> String {
    format!(
        "(SELECT o FROM (SELECT ordinal AS o FROM _speedyspec_columns WHERE table_name = {} AND column_name = {}) AS speedyspec_o)",
        sql_literal(table),
        sql_literal(column)
    )
}

/// Inserts `column` at `position`, shifting the columns after it.
fn insert_column(
    table: &str,
    column: &CatalogueColumn,
    position: &ColumnPosition,
) -> Vec<Statement> {
    let t = sql_literal(table);
    let mut out = Vec::new();
    let ordinal = match position {
        ColumnPosition::Last => format!(
            "(SELECT o FROM (SELECT COALESCE(MAX(ordinal), 0) + 1 AS o FROM _speedyspec_columns WHERE table_name = {t}) AS speedyspec_o)"
        ),
        ColumnPosition::First => {
            out.push(stmt(format!(
                "UPDATE _speedyspec_columns SET ordinal = ordinal + 1 WHERE table_name = {t}"
            )));
            "1".to_string()
        }
        ColumnPosition::After(after) => {
            let at = ordinal_of(table, after);
            out.push(stmt(format!(
                "UPDATE _speedyspec_columns SET ordinal = ordinal + 1 WHERE table_name = {t} AND ordinal > {at}"
            )));
            format!("{at} + 1")
        }
    };
    out.push(stmt(format!(
        "INSERT INTO _speedyspec_columns ({COLUMN_FIELDS}) VALUES ({})",
        column_values(table, &ordinal, column, collation_sql(table, column))
    )));
    out
}

/// Removes `column`, closing the gap in the ordinals.
fn remove_column(table: &str, column: &str) -> Vec<Statement> {
    let (t, c) = (sql_literal(table), sql_literal(column));
    vec![
        stmt(format!(
            "UPDATE _speedyspec_columns SET ordinal = ordinal - 1 WHERE table_name = {t} AND ordinal > {}",
            ordinal_of(table, column)
        )),
        stmt(format!(
            "DELETE FROM _speedyspec_columns WHERE table_name = {t} AND column_name = {c}"
        )),
    ]
}

/// The catalogue rows a schema change writes: literals only, no params.
pub fn catalogue_writes(change: &SchemaChange) -> Vec<Statement> {
    match change {
        SchemaChange::CreateTable(table) => {
            let t = sql_literal(&table.name);
            let mut out: Vec<Statement> = [
                "_speedyspec_columns",
                "_speedyspec_indexes",
                "_speedyspec_tables",
            ]
            .iter()
            .map(|name| stmt(format!("DELETE FROM {name} WHERE table_name = {t}")))
            .collect();
            out.push(stmt(format!(
                "INSERT INTO _speedyspec_tables (table_name, `collation`) VALUES ({t}, {})",
                sql_opt(table.collation.as_deref())
            )));
            if !table.columns.is_empty() {
                let rows: Vec<String> = table
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        format!(
                            "({})",
                            column_values(
                                &table.name,
                                &(i + 1).to_string(),
                                c,
                                sql_opt(c.collation.as_deref())
                            )
                        )
                    })
                    .collect();
                out.push(stmt(format!(
                    "INSERT INTO _speedyspec_columns ({COLUMN_FIELDS}) VALUES {}",
                    rows.join(", ")
                )));
            }
            out.extend(
                table
                    .indexes
                    .iter()
                    .filter_map(|i| index_rows(&table.name, i)),
            );
            out
        }
        SchemaChange::DropTable(table) => [
            "_speedyspec_columns",
            "_speedyspec_indexes",
            "_speedyspec_tables",
        ]
        .iter()
        .map(|name| {
            stmt(format!(
                "DELETE FROM {name} WHERE table_name = {}",
                sql_literal(table)
            ))
        })
        .collect(),
        SchemaChange::RenameTable { from, to } => [
            "_speedyspec_columns",
            "_speedyspec_indexes",
            "_speedyspec_tables",
        ]
        .iter()
        .map(|name| {
            stmt(format!(
                "UPDATE {name} SET table_name = {} WHERE table_name = {}",
                sql_literal(to),
                sql_literal(from)
            ))
        })
        .collect(),
        SchemaChange::AddColumn {
            table,
            column,
            position,
        } => insert_column(table, column, position),
        SchemaChange::ChangeColumn {
            table,
            old_name,
            column,
            position,
        } => {
            let mut out = Vec::new();
            let (t, old) = (sql_literal(table), sql_literal(old_name));
            match position {
                ColumnPosition::Last => out.push(stmt(format!(
                    "UPDATE _speedyspec_columns SET column_name = {}, column_type = {}, nullable = {}, column_default = {}, extra = {}, `collation` = {} WHERE table_name = {t} AND column_name = {old}",
                    sql_literal(&column.name),
                    sql_literal(&column.column_type.display()),
                    i32::from(column.nullable),
                    sql_opt(column.default.as_deref()),
                    sql_literal(&column.extra),
                    collation_sql(table, column),
                ))),
                moved => {
                    out.extend(remove_column(table, old_name));
                    out.extend(insert_column(table, column, moved));
                }
            }
            if !old_name.eq_ignore_ascii_case(&column.name) {
                out.push(stmt(format!(
                    "UPDATE _speedyspec_indexes SET column_name = {} WHERE table_name = {t} AND column_name = {old}",
                    sql_literal(&column.name)
                )));
            }
            out
        }
        SchemaChange::DropColumn { table, column } => {
            let mut out = remove_column(table, column);
            out.push(stmt(format!(
                "DELETE FROM _speedyspec_indexes WHERE table_name = {} AND column_name = {}",
                sql_literal(table),
                sql_literal(column)
            )));
            out
        }
        SchemaChange::AddIndex { table, index } => index_rows(table, index).into_iter().collect(),
        SchemaChange::DropIndex { table, name } => vec![stmt(format!(
            "DELETE FROM _speedyspec_indexes WHERE table_name = {} AND index_name = {}",
            sql_literal(table),
            sql_literal(name)
        ))],
        SchemaChange::SetCollation { table, collation } => vec![stmt(format!(
            "UPDATE _speedyspec_tables SET `collation` = {} WHERE table_name = {}",
            sql_literal(collation),
            sql_literal(table)
        ))],
    }
}

fn text(row: &Row, column: &str) -> Option<String> {
    match row.get(column)? {
        DbValue::Null => None,
        DbValue::Text(s) => Some(s.clone()),
        DbValue::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        other => other
            .to_mysql_text()
            .map(|b| String::from_utf8_lossy(&b).into_owned()),
    }
}

/// The catalogue's record of `table`; `None` when it has none.
pub fn read_catalogue(
    run: &dyn Fn(&Statement) -> Result<Rows, DbError>,
    table: &str,
) -> Result<Option<CatalogueTable>, DbError> {
    let t = sql_literal(table);
    let tables = run(&stmt(format!(
        "SELECT table_name, `collation` FROM _speedyspec_tables WHERE table_name = {t}"
    )))?;
    let Some(row) = tables.rows.first() else {
        return Ok(None);
    };
    let name = text(row, "table_name").unwrap_or_else(|| table.to_string());
    let collation = text(row, "collation");
    let columns = run(&stmt(format!(
        "SELECT column_name, column_type, nullable, column_default, extra, `collation` FROM _speedyspec_columns WHERE table_name = {t} ORDER BY ordinal"
    )))?
    .rows
    .iter()
    .map(|r| {
        Ok(CatalogueColumn {
            name: text(r, "column_name").unwrap_or_default(),
            column_type: MySqlType::parse(&text(r, "column_type").unwrap_or_default())?,
            nullable: text(r, "nullable").as_deref() == Some("1"),
            default: text(r, "column_default"),
            extra: text(r, "extra").unwrap_or_default(),
            collation: text(r, "collation"),
        })
    })
    .collect::<Result<Vec<_>, DbError>>()?;
    let mut indexes: Vec<CatalogueIndex> = Vec::new();
    for r in run(&stmt(format!(
        "SELECT index_name, kind, seq, column_name, sub_part FROM _speedyspec_indexes WHERE table_name = {t} ORDER BY CASE WHEN kind = 'PRIMARY' THEN 0 ELSE 1 END, index_name, seq"
    )))?
    .rows
    .iter()
    {
        let name = text(r, "index_name").unwrap_or_default();
        let part = (text(r, "column_name").unwrap_or_default(), text(r, "sub_part").and_then(|s| s.parse().ok()));
        match indexes.iter_mut().find(|i| i.name == name) {
            Some(index) => index.parts.push(part),
            None => indexes.push(CatalogueIndex {
                kind: IndexKind::parse(&text(r, "kind").unwrap_or_default()),
                name,
                parts: vec![part],
            }),
        }
    }
    Ok(Some(CatalogueTable {
        name,
        collation,
        columns,
        indexes,
    }))
}

/// Every table the catalogue records, by name.
pub fn catalogue_tables(
    run: &dyn Fn(&Statement) -> Result<Rows, DbError>,
) -> Result<Vec<String>, DbError> {
    Ok(run(&stmt(
        "SELECT table_name FROM _speedyspec_tables ORDER BY table_name".into(),
    ))?
    .rows
    .iter()
    .filter_map(|r| text(r, "table_name"))
    .collect())
}
