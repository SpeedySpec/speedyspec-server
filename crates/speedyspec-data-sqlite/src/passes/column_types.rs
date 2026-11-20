//! `sqlite_types`: MySQL column definitions as SQLite columns, with MySQL's implicit defaults and collations.

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass};
use speedyspec_data_translate::{CatalogueColumn, Temporal, TypeFamily, column_of, object_name};
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOption, ColumnOptionDef, DataType, Expr, Ident,
    ObjectName, PrimaryKeyConstraint, Statement as Ast, TableConstraint, Value,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Token;

/// The collation SQLite registers for MySQL's case- and accent-insensitive comparisons.
pub const CI_COLLATION: &str = "speedyspec_ci";

/// Converts `CREATE TABLE` columns and `ADD COLUMN` definitions.
pub struct SqliteTypes;

/// How a column is keyed, which decides its SQLite form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnRole {
    Plain,
    /// Part of the primary key: MySQL gives it no implicit default.
    PrimaryKey,
    /// The table's one primary-key column, with `AUTO_INCREMENT`: SQLite's `INTEGER PRIMARY KEY AUTOINCREMENT`.
    AutoIncrement,
}

/// SQLite's storage class for a MySQL type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Affinity {
    Integer,
    Real,
    Text,
    Blob,
}

fn affinity(column: &CatalogueColumn) -> Affinity {
    let ty = &column.column_type;
    if ty.base == "bit" {
        return Affinity::Integer;
    }
    match ty.family() {
        TypeFamily::Integer | TypeFamily::Temporal(Temporal::Year) => Affinity::Integer,
        TypeFamily::Decimal { .. } | TypeFamily::Float => Affinity::Real,
        TypeFamily::Binary => Affinity::Blob,
        TypeFamily::Text | TypeFamily::Enum | TypeFamily::Temporal(_) => Affinity::Text,
    }
}

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Sqlite,
    }
}

/// An expression from SQL text this crate writes itself.
pub(crate) fn expr(sql: &str) -> Expr {
    Parser::new(&GenericDialect {})
        .try_with_sql(sql)
        .and_then(|mut p| p.parse_expr())
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn option(option: ColumnOption) -> ColumnOptionDef {
    ColumnOptionDef { name: None, option }
}

fn default_expr(value: &str, affinity: Affinity) -> Expr {
    if value == "CURRENT_TIMESTAMP" {
        return expr("CURRENT_TIMESTAMP");
    }
    let numeric =
        matches!(affinity, Affinity::Integer | Affinity::Real) && value.parse::<f64>().is_ok();
    Expr::value(if numeric {
        Value::Number(value.to_string(), false)
    } else {
        Value::SingleQuotedString(value.to_string())
    })
}

/// The SQLite column for a catalogue column. `binary_charset` marks a `CHARACTER SET binary` text column.
pub fn sqlite_column(
    column: &CatalogueColumn,
    role: ColumnRole,
    binary_charset: bool,
) -> ColumnDef {
    let affinity = affinity(column);
    let data_type = match affinity {
        Affinity::Integer => DataType::Integer(None),
        Affinity::Real => DataType::Real,
        Affinity::Text => DataType::Text,
        Affinity::Blob => DataType::Blob(None),
    };
    let mut options = Vec::new();
    if role == ColumnRole::AutoIncrement {
        options.push(option(ColumnOption::PrimaryKey(PrimaryKeyConstraint {
            name: None,
            index_name: None,
            index_type: None,
            columns: Vec::new(),
            include: Vec::new(),
            index_options: Vec::new(),
            characteristics: None,
        })));
        options.push(option(ColumnOption::DialectSpecific(vec![
            Token::make_keyword("AUTOINCREMENT"),
        ])));
    }
    if !column.nullable {
        options.push(option(ColumnOption::NotNull));
    }
    let default = match (&column.default, role) {
        (Some(d), _) => Some(d.as_str()),
        // WordPress runs MySQL without strict mode, where a NOT NULL column given no value takes this.
        (None, ColumnRole::Plain) if !column.nullable => {
            Some(column.column_type.implicit_default())
        }
        _ => None,
    };
    if let Some(d) = default {
        options.push(option(ColumnOption::Default(default_expr(d, affinity))));
    }
    let text = matches!(
        column.column_type.family(),
        TypeFamily::Text | TypeFamily::Enum
    );
    if text {
        let binary = binary_charset
            || column
                .collation
                .as_deref()
                .is_some_and(|c| c.ends_with("_bin"));
        let name = if binary { "BINARY" } else { CI_COLLATION };
        options.push(option(ColumnOption::Collation(ObjectName::from(vec![
            Ident::new(name),
        ]))));
    }
    if column.extra.contains("on update") {
        options.push(option(ColumnOption::OnUpdate(expr("CURRENT_TIMESTAMP"))));
    }
    ColumnDef {
        name: Ident::new(&column.name),
        data_type,
        options,
    }
}

fn binary_charset(def: &ColumnDef) -> bool {
    def.options.iter().any(|o| {
        matches!(&o.option, ColumnOption::CharacterSet(name) if object_name(name).eq_ignore_ascii_case("binary"))
    })
}

fn pk_columns(constraints: &[TableConstraint]) -> Vec<String> {
    constraints
        .iter()
        .filter_map(|c| match c {
            TableConstraint::PrimaryKey(pk) => Some(&pk.columns),
            _ => None,
        })
        .flatten()
        .map(|c| {
            c.column
                .expr
                .to_string()
                .trim_matches(['`', '"'])
                .to_string()
        })
        .collect()
}

impl SqliteTypes {
    fn create_table(
        columns: &mut [ColumnDef],
        constraints: &mut Vec<TableConstraint>,
    ) -> Result<(), DbError> {
        let mut keyed = pk_columns(constraints);
        let mut parsed = Vec::new();
        for def in columns.iter() {
            let (column, primary, _) = column_of(def, None)?;
            if primary {
                keyed.push(column.name.clone());
            }
            parsed.push((column, primary, binary_charset(def)));
        }
        let auto: Vec<&str> = parsed
            .iter()
            .filter(|(c, _, _)| c.extra == "auto_increment")
            .map(|(c, _, _)| c.name.as_str())
            .collect();
        let auto_column = match auto.as_slice() {
            [] => None,
            [one] if keyed.len() == 1 && keyed[0].eq_ignore_ascii_case(one) => {
                Some(one.to_string())
            }
            _ => {
                return Err(unsupported(
                    "AUTO_INCREMENT outside a single-column primary key",
                ));
            }
        };
        if auto_column.is_some() {
            constraints.retain(|c| !matches!(c, TableConstraint::PrimaryKey(_)));
        }
        for (def, (column, primary, binary)) in columns.iter_mut().zip(&parsed) {
            let role = if auto_column
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(&column.name))
            {
                ColumnRole::AutoIncrement
            } else if keyed.iter().any(|k| k.eq_ignore_ascii_case(&column.name)) {
                ColumnRole::PrimaryKey
            } else {
                ColumnRole::Plain
            };
            let mut converted = sqlite_column(column, role, *binary);
            if *primary && role == ColumnRole::PrimaryKey {
                converted
                    .options
                    .push(option(ColumnOption::PrimaryKey(PrimaryKeyConstraint {
                        name: None,
                        index_name: None,
                        index_type: None,
                        columns: Vec::new(),
                        include: Vec::new(),
                        index_options: Vec::new(),
                        characteristics: None,
                    })));
            }
            *def = converted;
        }
        Ok(())
    }
}

impl RewritePass for SqliteTypes {
    fn name(&self) -> &'static str {
        "sqlite_types"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        match stmt.tree_mut() {
            Ast::CreateTable(create) => {
                SqliteTypes::create_table(&mut create.columns, &mut create.constraints)?;
            }
            Ast::AlterTable(alter) => {
                let table = object_name(&alter.name);
                let collation = match cx.introspector {
                    Some(i) => i.table_collation(&table)?,
                    None => None,
                };
                for op in &mut alter.operations {
                    if let AlterTableOperation::AddColumn { column_def, .. } = op {
                        let (column, primary, _) = column_of(column_def, collation.as_deref())?;
                        if primary || column.extra == "auto_increment" {
                            return Err(unsupported("ADD COLUMN with a key"));
                        }
                        *column_def =
                            sqlite_column(&column, ColumnRole::Plain, binary_charset(column_def));
                    }
                }
            }
            _ => {}
        }
        Ok(Rewrite::Continue)
    }
}
