//! `PgDdl`: MySQL column definitions as PostgreSQL columns (types, identity, defaults, collations), the `ALTER TABLE`
//! forms PostgreSQL spells otherwise, and `TRUNCATE` restarting identity as MySQL's restarts `AUTO_INCREMENT`.

use std::rc::Rc;

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use speedyspec_data_translate::{CatalogueColumn, column_of, object_name, sql_ident};
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, ColumnDef, ColumnOption, ColumnOptionDef, DataType,
    Expr, GeneratedAs, Ident, ObjectName, Statement as Ast, TableConstraint,
    TruncateIdentityOption, Value,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::schema::SchemaCache;
use crate::types::{is_temporal, is_text, pg_type};

/// The collation PostgreSQL holds MySQL's case- and accent-insensitive comparisons in.
pub(crate) const CI_COLLATION: &str = "speedyspec_ci";

pub(crate) struct PgDdl {
    schema: Rc<SchemaCache>,
}

impl PgDdl {
    pub(crate) fn new(schema: Rc<SchemaCache>) -> PgDdl {
        PgDdl { schema }
    }
}

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Postgres,
    }
}

fn expr(sql: &str) -> Expr {
    Parser::new(&PostgreSqlDialect {})
        .try_with_sql(sql)
        .and_then(|mut p| p.parse_expr())
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn option(option: ColumnOption) -> ColumnOptionDef {
    ColumnOptionDef { name: None, option }
}

/// The PostgreSQL collation for a MySQL one: `_bin` is byte order, `_cs` ICU's case-sensitive root, any other the
/// case- and accent-insensitive `speedyspec_ci`.
pub(crate) fn pg_collation(mysql: Option<&str>) -> &'static str {
    match mysql {
        Some(c) if c.ends_with("_bin") || c == "binary" => "C",
        Some(c) if c.ends_with("_cs") => "und-x-icu",
        _ => CI_COLLATION,
    }
}

fn collation(name: &str) -> ColumnOption {
    ColumnOption::Collation(ObjectName::from(vec![Ident::new(name)]))
}

fn default_expr(column: &CatalogueColumn, value: &str) -> Expr {
    if value == "CURRENT_TIMESTAMP" {
        return expr("LOCALTIMESTAMP(0)");
    }
    // MySQL's zero date sorts before every date; -infinity does too.
    if is_temporal(&column.column_type) && value.starts_with("0000-00-00") {
        return Expr::value(Value::SingleQuotedString("-infinity".into()));
    }
    Expr::value(Value::SingleQuotedString(value.to_string()))
}

/// The PostgreSQL column for a catalogue column; `identity` for the table's `AUTO_INCREMENT` key.
/// The default MySQL applies: the column's own, else, for a NOT NULL column outside the primary key, the value
/// MySQL's non-strict mode (WordPress's) stores when an insert omits it.
fn effective_default(column: &CatalogueColumn, keyed: bool) -> Option<String> {
    match &column.default {
        Some(d) => Some(d.clone()),
        None if !column.nullable && !keyed && column.extra != "auto_increment" => {
            Some(column.column_type.implicit_default().to_string())
        }
        None => None,
    }
}

fn pg_column(column: &CatalogueColumn, identity: bool, keyed: bool) -> Result<ColumnDef, DbError> {
    if column.extra.contains("on update") {
        return Err(unsupported("ON UPDATE CURRENT_TIMESTAMP"));
    }
    let ty = &column.column_type;
    let mut options = Vec::new();
    if is_text(ty) {
        options.push(option(collation(pg_collation(column.collation.as_deref()))));
    }
    if !column.nullable || identity {
        options.push(option(ColumnOption::NotNull));
    }
    if identity {
        options.push(option(ColumnOption::Generated {
            generated_as: GeneratedAs::ByDefault,
            sequence_options: Some(Vec::new()),
            generation_expr: None,
            generation_expr_mode: None,
            generated_keyword: true,
        }));
    }
    if let Some(d) = effective_default(column, keyed || identity) {
        options.push(option(ColumnOption::Default(default_expr(column, &d))));
    }
    if ty.base == "enum" {
        let values: Vec<String> = ty.args.iter().map(|a| a.to_string()).collect();
        let check = expr(&format!(
            "{} IN ({})",
            sql_ident(&column.name),
            values.join(", ")
        ));
        options.push(option(ColumnOption::Check(
            sqlparser::ast::CheckConstraint {
                name: None,
                expr: Box::new(check),
                no_inherit: false,
                enforced: None,
            },
        )));
    }
    Ok(ColumnDef {
        name: Ident::new(&column.name),
        data_type: pg_type(ty)?,
        options,
    })
}

fn table_collation(schema: &SchemaCache, table: &str) -> Option<String> {
    schema
        .introspector()
        .and_then(|i| i.table_collation(table).ok().flatten())
}

impl PgDdl {
    fn create_table(
        columns: &mut [ColumnDef],
        constraints: &[TableConstraint],
    ) -> Result<(), DbError> {
        let keyed: Vec<String> = constraints
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
                    .to_ascii_lowercase()
            })
            .collect();
        for def in columns.iter_mut() {
            let (column, primary, _) = column_of(def, None)?;
            let identity = column.extra == "auto_increment";
            let key = primary || keyed.contains(&column.name.to_ascii_lowercase());
            *def = pg_column(&column, identity, key)?;
        }
        Ok(())
    }

    /// `CHANGE`/`MODIFY`: the type (with its collation), nullability and default set as the new definition says.
    fn change(&self, table: &str, old: &str, def: &ColumnDef) -> Result<Vec<Statement>, DbError> {
        let (column, _, _) = column_of(def, table_collation(&self.schema, table).as_deref())?;
        // Declaring AUTO_INCREMENT again on a column that has it (dbDelta does) changes nothing; adding it is not
        // something PostgreSQL's identity can do in place.
        let already = self.schema.columns(table).is_some_and(|c| {
            c.iter()
                .any(|c| c.name.eq_ignore_ascii_case(old) && c.extra == "auto_increment")
        });
        if column.extra == "auto_increment" && !already {
            return Err(unsupported("AUTO_INCREMENT added to an existing column"));
        }
        if column.extra.contains("on update") {
            return Err(unsupported("ON UPDATE CURRENT_TIMESTAMP"));
        }
        let mut out = Vec::new();
        let stored = self
            .schema
            .resolve(table, old)
            .unwrap_or_else(|| old.to_string());
        if stored != column.name {
            out.push(parse_one(&format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {}",
                sql_ident(table),
                sql_ident(&stored),
                sql_ident(&column.name)
            ))?);
        }
        let pg = pg_column(&column, false, false)?;
        let ty = pg.data_type.to_string().to_lowercase();
        // sqlparser has no COLLATE in ALTER COLUMN ... TYPE: the type is written out with it.
        let data_type = if is_text(&column.column_type) {
            DataType::Custom(
                ObjectName::from(vec![Ident::new(format!(
                    "{ty} COLLATE \"{}\"",
                    pg_collation(column.collation.as_deref())
                ))]),
                Vec::new(),
            )
        } else {
            pg.data_type.clone()
        };
        let name = Ident::new(&column.name);
        let mut ops = vec![AlterTableOperation::AlterColumn {
            column_name: name.clone(),
            op: AlterColumnOperation::SetDataType {
                data_type,
                using: None,
                had_set: false,
            },
        }];
        ops.push(AlterTableOperation::AlterColumn {
            column_name: name.clone(),
            op: if column.nullable {
                AlterColumnOperation::DropNotNull
            } else {
                AlterColumnOperation::SetNotNull
            },
        });
        if column.extra == "auto_increment" {
            // An identity column keeps its generated default.
            let (mut tree, _) =
                parse_one(&format!("ALTER TABLE {} DROP COLUMN x", sql_ident(table)))?.into_parts();
            if let Ast::AlterTable(alter) = &mut tree {
                alter.operations = ops;
            }
            out.push(Statement::new(tree, Vec::new()));
            return Ok(out);
        }
        ops.push(AlterTableOperation::AlterColumn {
            column_name: name,
            op: match effective_default(&column, false) {
                Some(d) => AlterColumnOperation::SetDefault {
                    value: default_expr(&column, &d),
                },
                None => AlterColumnOperation::DropDefault,
            },
        });
        let (mut tree, _) =
            parse_one(&format!("ALTER TABLE {} DROP COLUMN x", sql_ident(table)))?.into_parts();
        if let Ast::AlterTable(alter) = &mut tree {
            alter.operations = ops;
        }
        out.push(Statement::new(tree, Vec::new()));
        Ok(out)
    }
}

fn column_def(name: &Ident, data_type: &DataType, options: &[ColumnOption]) -> ColumnDef {
    ColumnDef {
        name: name.clone(),
        data_type: data_type.clone(),
        options: options.iter().map(|o| option(o.clone())).collect(),
    }
}

impl RewritePass for PgDdl {
    fn name(&self) -> &'static str {
        "pg_ddl"
    }

    fn rewrite(&self, stmt: &mut Statement, _cx: &PassContext) -> Result<Rewrite, DbError> {
        match stmt.tree_mut() {
            Ast::CreateTable(create) => {
                Self::create_table(&mut create.columns, &create.constraints)?;
                // The neutral DDL pass keeps only the primary key in CREATE TABLE.
                create
                    .constraints
                    .retain(|c| matches!(c, TableConstraint::PrimaryKey(_)));
            }
            Ast::CreateIndex(create) => {
                if let Some(name) = &create.name
                    && object_name(name).len() > 63
                {
                    return Err(unsupported("an index name longer than 63 bytes"));
                }
            }
            Ast::Truncate(truncate) => truncate.identity = Some(TruncateIdentityOption::Restart),
            // PostgreSQL finds a temporary table first by name; it has no DROP TEMPORARY.
            Ast::Drop { temporary, .. } => *temporary = false,
            Ast::AlterTable(alter) => {
                let table = object_name(&alter.name);
                let [op] = alter.operations.as_slice() else {
                    return Ok(Rewrite::Continue);
                };
                match op.clone() {
                    AlterTableOperation::AddColumn {
                        column_def: def, ..
                    } => {
                        let (column, primary, _) =
                            column_of(&def, table_collation(&self.schema, &table).as_deref())?;
                        if column.extra == "auto_increment" || primary {
                            return Err(unsupported("AUTO_INCREMENT added to an existing table"));
                        }
                        let pg = pg_column(&column, false, false)?;
                        if let AlterTableOperation::AddColumn { column_def, .. } =
                            &mut alter.operations[0]
                        {
                            *column_def = pg;
                        }
                    }
                    AlterTableOperation::ChangeColumn {
                        old_name,
                        new_name,
                        data_type,
                        options,
                        ..
                    } => {
                        let def = column_def(&new_name, &data_type, &options);
                        return Ok(Rewrite::Split(self.change(
                            &table,
                            &old_name.value,
                            &def,
                        )?));
                    }
                    AlterTableOperation::ModifyColumn {
                        col_name,
                        data_type,
                        options,
                        ..
                    } => {
                        let def = column_def(&col_name, &data_type, &options);
                        return Ok(Rewrite::Split(self.change(
                            &table,
                            &col_name.value,
                            &def,
                        )?));
                    }
                    AlterTableOperation::DropPrimaryKey { .. } => {
                        // PostgreSQL names a table's primary key <table>_pkey.
                        return Ok(Rewrite::Split(vec![parse_one(&format!(
                            "ALTER TABLE {} DROP CONSTRAINT {}",
                            sql_ident(&table),
                            sql_ident(&format!("{table}_pkey"))
                        ))?]));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(Rewrite::Continue)
    }
}
