//! `sqlite_alter`: MySQL `ALTER TABLE` actions SQLite cannot run in place become a table rebuild; `TRUNCATE`
//! becomes a delete that also resets the table's counter.

use speedyspec_data::{DbError, DialectKind, Introspector, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use speedyspec_data_translate::{
    CatalogueColumn, CatalogueIndex, CatalogueTable, IndexKind, MySqlType, column_of,
    index_object_name, is_catalogue_table, object_name, sql_ident, sql_literal,
};
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOptionDef, RenameTableNameKind, Statement as Ast,
    TableConstraint,
};

use super::column_types::{ColumnRole, sqlite_column};

/// The suffix of the table a rebuild fills before it takes the original's name.
pub const REBUILD_SUFFIX: &str = "__speedyspec_rebuild";

pub struct SqliteAlter;

fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Sqlite,
    }
}

fn stmt(sql: &str) -> Result<Statement, DbError> {
    parse_one(sql)
}

/// The table as the catalogue describes it now. The introspector does not say which indexes are `FULLTEXT`, so
/// those read as plain indexes.
fn current(table: &str, introspector: &dyn Introspector) -> Result<CatalogueTable, DbError> {
    let columns = introspector
        .columns(table)?
        .into_iter()
        .map(|c| {
            Ok(CatalogueColumn {
                name: c.name,
                column_type: MySqlType::parse(&c.sql_type)?,
                nullable: c.nullable,
                default: c.default,
                extra: c.extra,
                collation: c.collation,
            })
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    let indexes = introspector
        .indexes(table)?
        .into_iter()
        .map(|i| CatalogueIndex {
            kind: if i.name == "PRIMARY" {
                IndexKind::Primary
            } else if i.unique {
                IndexKind::Unique
            } else {
                IndexKind::Index
            },
            name: i.name,
            parts: i.columns,
        })
        .collect();
    Ok(CatalogueTable {
        name: table.to_string(),
        collation: introspector.table_collation(table)?,
        columns,
        indexes,
    })
}

fn role(table: &CatalogueTable, column: &CatalogueColumn) -> ColumnRole {
    let in_pk = table.indexes.iter().any(|i| {
        i.kind == IndexKind::Primary
            && i.parts
                .iter()
                .any(|(c, _)| c.eq_ignore_ascii_case(&column.name))
    });
    match (in_pk, column.extra == "auto_increment") {
        (true, true) => ColumnRole::AutoIncrement,
        (true, false) => ColumnRole::PrimaryKey,
        _ => ColumnRole::Plain,
    }
}

/// The column as SQLite stores it, without its name: what decides whether a change needs a rebuild.
fn sqlite_shape(table: &CatalogueTable, column: &CatalogueColumn) -> String {
    let def = sqlite_column(column, role(table, column), false);
    let options: Vec<String> = def.options.iter().map(ToString::to_string).collect();
    format!("{} {}", def.data_type, options.join(" "))
}

fn find<'a>(table: &'a CatalogueTable, name: &str) -> Option<&'a CatalogueColumn> {
    table
        .columns
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
}

fn indexed(table: &CatalogueTable, column: &str) -> bool {
    table
        .indexes
        .iter()
        .any(|i| i.parts.iter().any(|(c, _)| c.eq_ignore_ascii_case(column)))
}

fn create_index(table: &str, index: &CatalogueIndex) -> Result<Statement, DbError> {
    let cols: Vec<String> = index.parts.iter().map(|(c, _)| sql_ident(c)).collect();
    stmt(&format!(
        "CREATE {}INDEX {} ON {} ({})",
        if index.kind == IndexKind::Unique {
            "UNIQUE "
        } else {
            ""
        },
        sql_ident(&index_object_name(table, &index.name)),
        sql_ident(table),
        cols.join(", ")
    ))
}

/// SQLite's way to change what `ALTER TABLE` cannot: a new table in the new shape, the rows copied, the old table
/// dropped, the new one renamed, and its indexes made again. `copies` pairs each new column with the old column
/// whose values it takes.
fn rebuild(new: &CatalogueTable, copies: &[(String, String)]) -> Result<Vec<Statement>, DbError> {
    let table = &new.name;
    let staging = format!("{table}{REBUILD_SUFFIX}");
    let mut shape = new.clone();
    shape.name = staging.clone();
    shape.indexes.retain(|i| i.kind == IndexKind::Primary);
    let mut out = vec![shape.to_create_table()];
    if !copies.is_empty() {
        let to: Vec<String> = copies.iter().map(|(n, _)| sql_ident(n)).collect();
        let from: Vec<String> = copies.iter().map(|(_, o)| sql_ident(o)).collect();
        out.push(stmt(&format!(
            "INSERT INTO {} ({}) SELECT {} FROM {}",
            sql_ident(&staging),
            to.join(", "),
            from.join(", "),
            sql_ident(table)
        ))?);
    }
    out.push(stmt(&format!("DROP TABLE {}", sql_ident(table)))?);
    out.push(stmt(&format!(
        "ALTER TABLE {} RENAME TO {}",
        sql_ident(&staging),
        sql_ident(table)
    ))?);
    for index in &new.indexes {
        if index.kind != IndexKind::Primary {
            out.push(create_index(table, index)?);
        }
    }
    Ok(out)
}

/// Every column copied under its own name, except those `skip` names.
fn same_columns(
    table: &CatalogueTable,
    old: &CatalogueTable,
    skip: &[&str],
) -> Vec<(String, String)> {
    table
        .columns
        .iter()
        .filter(|c| !skip.iter().any(|s| s.eq_ignore_ascii_case(&c.name)))
        .filter(|c| find(old, &c.name).is_some())
        .map(|c| (c.name.clone(), c.name.clone()))
        .collect()
}

fn column_def(
    name: &sqlparser::ast::Ident,
    data_type: &sqlparser::ast::DataType,
    options: &[sqlparser::ast::ColumnOption],
) -> ColumnDef {
    ColumnDef {
        name: name.clone(),
        data_type: data_type.clone(),
        options: options
            .iter()
            .map(|o| ColumnOptionDef {
                name: None,
                option: o.clone(),
            })
            .collect(),
    }
}

impl SqliteAlter {
    fn change(
        old: &CatalogueTable,
        old_name: &str,
        def: &ColumnDef,
        original: &Statement,
    ) -> Result<Rewrite, DbError> {
        let (column, primary, _) = column_of(def, old.collation.as_deref())?;
        let previous = find(old, old_name)
            .ok_or_else(|| unsupported("CHANGE of a column the table does not have"))?;
        let mut new = old.clone();
        for c in &mut new.columns {
            if c.name.eq_ignore_ascii_case(old_name) {
                *c = column.clone();
            }
        }
        for index in &mut new.indexes {
            for (c, _) in &mut index.parts {
                if c.eq_ignore_ascii_case(old_name) {
                    *c = column.name.clone();
                }
            }
        }
        let renamed = !previous.name.eq_ignore_ascii_case(&column.name);
        if !primary && sqlite_shape(old, previous) == sqlite_shape(&new, &column) {
            if renamed {
                return Ok(Rewrite::Split(vec![stmt(&format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    sql_ident(&old.name),
                    sql_ident(&previous.name),
                    sql_ident(&column.name)
                ))?]));
            }
            return Ok(Rewrite::Split(Vec::new()));
        }
        let _ = original;
        let mut copies = same_columns(&new, old, &[&column.name]);
        let position = new
            .columns
            .iter()
            .position(|c| c.name == column.name)
            .unwrap_or(0);
        copies.insert(
            position.min(copies.len()),
            (column.name.clone(), previous.name.clone()),
        );
        Ok(Rewrite::Split(rebuild(&new, &copies)?))
    }

    fn alter(
        &self,
        stmt_in: &Statement,
        op: &AlterTableOperation,
        table: &str,
        introspector: &dyn Introspector,
    ) -> Result<Rewrite, DbError> {
        let old = current(table, introspector)?;
        match op {
            AlterTableOperation::AddColumn { column_def, .. } => {
                let (column, primary, _) = column_of(column_def, old.collation.as_deref())?;
                let needs_rebuild = primary
                    || column.extra == "auto_increment"
                    || column.default.as_deref() == Some("CURRENT_TIMESTAMP");
                if !needs_rebuild {
                    return Ok(Rewrite::Continue);
                }
                let mut new = old.clone();
                new.columns.push(column);
                if primary {
                    new.indexes.retain(|i| i.kind != IndexKind::Primary);
                    new.indexes.insert(
                        0,
                        CatalogueIndex {
                            name: "PRIMARY".into(),
                            kind: IndexKind::Primary,
                            parts: vec![(column_def.name.value.clone(), None)],
                        },
                    );
                }
                let copies = same_columns(&new, &old, &[]);
                Ok(Rewrite::Split(rebuild(&new, &copies)?))
            }
            AlterTableOperation::ChangeColumn {
                old_name,
                new_name,
                data_type,
                options,
                ..
            } => Self::change(
                &old,
                &old_name.value,
                &column_def(new_name, data_type, options),
                stmt_in,
            ),
            AlterTableOperation::ModifyColumn {
                col_name,
                data_type,
                options,
                ..
            } => Self::change(
                &old,
                &col_name.value,
                &column_def(col_name, data_type, options),
                stmt_in,
            ),
            AlterTableOperation::DropColumn { column_names, .. } => {
                let names: Vec<&str> = column_names.iter().map(|c| c.value.as_str()).collect();
                let in_place = names.iter().all(|n| {
                    !indexed(&old, n)
                        && find(&old, n).is_none_or(|c| !c.extra.contains("on update"))
                });
                if in_place {
                    let drops = names
                        .iter()
                        .map(|n| {
                            stmt(&format!(
                                "ALTER TABLE {} DROP COLUMN {}",
                                sql_ident(table),
                                sql_ident(n)
                            ))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Rewrite::Split(drops));
                }
                let mut new = old.clone();
                new.columns
                    .retain(|c| !names.iter().any(|n| n.eq_ignore_ascii_case(&c.name)));
                for index in &mut new.indexes {
                    index
                        .parts
                        .retain(|(c, _)| !names.iter().any(|n| n.eq_ignore_ascii_case(c)));
                }
                new.indexes.retain(|i| !i.parts.is_empty());
                let copies = same_columns(&new, &old, &[]);
                Ok(Rewrite::Split(rebuild(&new, &copies)?))
            }
            AlterTableOperation::RenameColumn { .. } => Ok(Rewrite::Continue),
            AlterTableOperation::RenameTable { table_name } => {
                let to = match table_name {
                    RenameTableNameKind::To(name) | RenameTableNameKind::As(name) => {
                        object_name(name)
                    }
                };
                let mut out = vec![stmt(&format!(
                    "ALTER TABLE {} RENAME TO {}",
                    sql_ident(table),
                    sql_ident(&to)
                ))?];
                for index in old.indexes.iter().filter(|i| i.kind != IndexKind::Primary) {
                    // A FULLTEXT index lives only in the catalogue; IF EXISTS lets its drop pass.
                    out.push(stmt(&format!(
                        "DROP INDEX IF EXISTS {}",
                        sql_ident(&index_object_name(table, &index.name))
                    ))?);
                    out.push(create_index(&to, index)?);
                }
                Ok(Rewrite::Split(out))
            }
            AlterTableOperation::AddConstraint {
                constraint: TableConstraint::PrimaryKey(pk),
                ..
            } => {
                let mut new = old.clone();
                new.indexes.retain(|i| i.kind != IndexKind::Primary);
                let parts = pk
                    .columns
                    .iter()
                    .map(|c| {
                        (
                            c.column
                                .expr
                                .to_string()
                                .trim_matches(['`', '"'])
                                .to_string(),
                            None,
                        )
                    })
                    .collect();
                new.indexes.insert(
                    0,
                    CatalogueIndex {
                        name: "PRIMARY".into(),
                        kind: IndexKind::Primary,
                        parts,
                    },
                );
                for column in &mut new.columns {
                    if new.indexes[0]
                        .parts
                        .iter()
                        .any(|(c, _)| c.eq_ignore_ascii_case(&column.name))
                    {
                        column.nullable = false;
                    }
                }
                let copies = same_columns(&new, &old, &[]);
                Ok(Rewrite::Split(rebuild(&new, &copies)?))
            }
            AlterTableOperation::DropPrimaryKey { .. } => {
                let mut new = old.clone();
                new.indexes.retain(|i| i.kind != IndexKind::Primary);
                for column in &mut new.columns {
                    if column.extra == "auto_increment" {
                        return Err(unsupported("DROP PRIMARY KEY of an AUTO_INCREMENT column"));
                    }
                }
                let copies = same_columns(&new, &old, &[]);
                Ok(Rewrite::Split(rebuild(&new, &copies)?))
            }
            AlterTableOperation::AddConstraint { .. } => {
                Err(unsupported("ALTER TABLE … ADD CONSTRAINT"))
            }
            _ => Err(unsupported("this ALTER TABLE action")),
        }
    }
}

impl RewritePass for SqliteAlter {
    fn name(&self) -> &'static str {
        "sqlite_alter"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        match stmt.tree() {
            Ast::Truncate(truncate) => {
                let mut out = Vec::new();
                for target in &truncate.table_names {
                    let table = object_name(&target.name);
                    out.push(parse_one(&format!("DELETE FROM {}", sql_ident(&table)))?);
                    // MySQL's TRUNCATE also restarts AUTO_INCREMENT.
                    out.push(parse_one(&format!(
                        "DELETE FROM sqlite_sequence WHERE name = {}",
                        sql_literal(&table)
                    ))?);
                }
                Ok(Rewrite::Split(out))
            }
            Ast::AlterTable(alter) => {
                let table = object_name(&alter.name);
                if is_catalogue_table(&table) || table.ends_with(REBUILD_SUFFIX) {
                    return Ok(Rewrite::Continue);
                }
                let Some(introspector) = cx.introspector else {
                    return Err(unsupported("ALTER TABLE without schema information"));
                };
                let [op] = alter.operations.as_slice() else {
                    return Err(unsupported("ALTER TABLE with several actions"));
                };
                let op = op.clone();
                let original = stmt.clone();
                self.alter(&original, &op, &table, introspector)
            }
            _ => Ok(Rewrite::Continue),
        }
    }
}
