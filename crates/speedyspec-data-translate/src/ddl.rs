//! `DdlPass`: MySQL DDL (as `dbDelta()` and plugins write it) split into statements every engine accepts, with the
//! catalogue writes that keep MySQL's view of the schema.

use speedyspec_data::{DbError, DialectKind, EmulatedOp, Statement};
use speedyspec_data_query::{PassContext, Rewrite, RewritePass, parse_one};
use sqlparser::ast::{
    AlterTableOperation, ColumnOption, CreateTableOptions, MySQLColumnPosition, ObjectType,
    RenameTableNameKind, SqlOption, Statement as Ast, TableConstraint,
};

use crate::catalogue::{
    CatalogueIndex, ColumnPosition, IndexKind, SchemaChange, catalogue_table, catalogue_writes,
    column_of, index_object_name, is_catalogue_table, parts, unnamed_index_name,
};
use crate::names::{object_name, sql_ident};

/// `CREATE TABLE` with its keys lifted out as `CREATE INDEX`; `DROP TABLE a, b` one per table; `ALTER TABLE` one
/// statement per action; `CREATE|DROP INDEX` with engine-wide index names; `RENAME TABLE` as `ALTER … RENAME TO`.
/// Each change also writes the catalogue.
pub struct DdlPass;

fn unsupported(construct: &str, target: DialectKind) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: target,
    }
}

fn stmt(sql: &str) -> Result<Statement, DbError> {
    parse_one(sql)
}

/// `CREATE [UNIQUE] INDEX <table>__<name> ON <table> (cols)`, prefix lengths dropped.
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

fn drop_index(table: &str, name: &str) -> Result<Statement, DbError> {
    stmt(&format!(
        "DROP INDEX {}",
        sql_ident(&index_object_name(table, name))
    ))
}

fn position(p: &Option<MySQLColumnPosition>) -> ColumnPosition {
    match p {
        Some(MySQLColumnPosition::First) => ColumnPosition::First,
        Some(MySQLColumnPosition::After(ident)) => ColumnPosition::After(ident.value.clone()),
        None => ColumnPosition::Last,
    }
}

fn has_auto_increment_option(options: &CreateTableOptions) -> bool {
    let list = match options {
        CreateTableOptions::Plain(list)
        | CreateTableOptions::With(list)
        | CreateTableOptions::Options(list) => list,
        _ => return false,
    };
    list.iter().any(|o| matches!(o, SqlOption::KeyValue { key, .. } if object_name(key).eq_ignore_ascii_case("AUTO_INCREMENT")))
}

struct Ddl<'a> {
    cx: &'a PassContext<'a>,
    out: Vec<Statement>,
    writes: Vec<SchemaChange>,
}

impl Ddl<'_> {
    fn index_names(&self, table: &str) -> Vec<String> {
        self.cx
            .introspector
            .and_then(|i| i.indexes(table).ok())
            .map(|idx| idx.into_iter().map(|i| i.name).collect())
            .unwrap_or_default()
    }

    fn add_index(
        &mut self,
        table: &str,
        kind: IndexKind,
        name: Option<String>,
        cols: Vec<(String, Option<u32>)>,
    ) -> Result<(), DbError> {
        let existing = self.index_names(table);
        let name = name.unwrap_or_else(|| unnamed_index_name(&cols, &existing));
        let index = CatalogueIndex {
            name,
            kind,
            parts: cols,
        };
        match kind {
            IndexKind::Fulltext | IndexKind::Spatial => {
                // Only the catalogue knows a FULLTEXT or SPATIAL index; adding one it already has changes nothing.
                if existing.iter().any(|n| n.eq_ignore_ascii_case(&index.name)) {
                    return Ok(());
                }
            }
            IndexKind::Primary => {}
            _ => self.out.push(create_index(table, &index)?),
        }
        self.writes.push(SchemaChange::AddIndex {
            table: table.to_string(),
            index,
        });
        Ok(())
    }

    fn alter(
        &mut self,
        table: &str,
        op: &AlterTableOperation,
        original: &Ast,
    ) -> Result<(), DbError> {
        let single = |op: AlterTableOperation| -> Result<Statement, DbError> {
            let mut tree = original.clone();
            if let Ast::AlterTable(alter) = &mut tree {
                alter.operations = vec![op];
            }
            Ok(Statement::new(tree, Vec::new()))
        };
        let table_collation = self
            .cx
            .introspector
            .and_then(|i| i.table_collation(table).ok().flatten());
        match op {
            AlterTableOperation::AddConstraint { constraint, .. } => match constraint {
                TableConstraint::Index(i) => self.add_index(
                    table,
                    IndexKind::Index,
                    i.name.as_ref().map(|n| n.value.clone()),
                    parts(&i.columns),
                )?,
                TableConstraint::Unique(u) => self.add_index(
                    table,
                    IndexKind::Unique,
                    u.index_name
                        .as_ref()
                        .or(u.name.as_ref())
                        .map(|n| n.value.clone()),
                    parts(&u.columns),
                )?,
                TableConstraint::FulltextOrSpatial(f) => self.add_index(
                    table,
                    if f.fulltext {
                        IndexKind::Fulltext
                    } else {
                        IndexKind::Spatial
                    },
                    f.opt_index_name.as_ref().map(|n| n.value.clone()),
                    parts(&f.columns),
                )?,
                TableConstraint::PrimaryKey(pk) => {
                    self.out.push(single(op.clone())?);
                    self.writes.push(SchemaChange::AddIndex {
                        table: table.to_string(),
                        index: CatalogueIndex {
                            name: "PRIMARY".into(),
                            kind: IndexKind::Primary,
                            parts: parts(&pk.columns),
                        },
                    });
                }
                _ => self.out.push(single(op.clone())?),
            },
            AlterTableOperation::DropIndex { name } => {
                self.out.push(drop_index(table, &name.value)?);
                self.writes.push(SchemaChange::DropIndex {
                    table: table.to_string(),
                    name: name.value.clone(),
                });
            }
            AlterTableOperation::DropPrimaryKey { .. } => {
                self.out.push(single(op.clone())?);
                self.writes.push(SchemaChange::DropIndex {
                    table: table.to_string(),
                    name: "PRIMARY".into(),
                });
            }
            AlterTableOperation::AddColumn {
                column_def,
                column_position,
                ..
            } => {
                let (column, _, _) = column_of(column_def, table_collation.as_deref())?;
                let mut plain = op.clone();
                if let AlterTableOperation::AddColumn {
                    column_position, ..
                } = &mut plain
                {
                    *column_position = None;
                }
                self.out.push(single(plain)?);
                self.writes.push(SchemaChange::AddColumn {
                    table: table.to_string(),
                    column,
                    position: position(column_position),
                });
            }
            AlterTableOperation::ChangeColumn {
                old_name,
                new_name,
                data_type,
                options,
                column_position,
            } => {
                let def = sqlparser::ast::ColumnDef {
                    name: new_name.clone(),
                    data_type: data_type.clone(),
                    options: options
                        .iter()
                        .map(|o| sqlparser::ast::ColumnOptionDef {
                            name: None,
                            option: o.clone(),
                        })
                        .collect(),
                };
                let (column, _, _) = column_of(&def, table_collation.as_deref())?;
                let mut plain = op.clone();
                if let AlterTableOperation::ChangeColumn {
                    column_position, ..
                } = &mut plain
                {
                    *column_position = None;
                }
                self.out.push(single(plain)?);
                self.writes.push(SchemaChange::ChangeColumn {
                    table: table.to_string(),
                    old_name: old_name.value.clone(),
                    column,
                    position: position(column_position),
                });
            }
            AlterTableOperation::ModifyColumn {
                col_name,
                data_type,
                options,
                column_position,
            } => {
                let def = sqlparser::ast::ColumnDef {
                    name: col_name.clone(),
                    data_type: data_type.clone(),
                    options: options
                        .iter()
                        .map(|o| sqlparser::ast::ColumnOptionDef {
                            name: None,
                            option: o.clone(),
                        })
                        .collect(),
                };
                let (column, _, _) = column_of(&def, table_collation.as_deref())?;
                let mut plain = op.clone();
                if let AlterTableOperation::ModifyColumn {
                    column_position, ..
                } = &mut plain
                {
                    *column_position = None;
                }
                self.out.push(single(plain)?);
                self.writes.push(SchemaChange::ChangeColumn {
                    table: table.to_string(),
                    old_name: col_name.value.clone(),
                    column,
                    position: position(column_position),
                });
            }
            AlterTableOperation::DropColumn { column_names, .. } => {
                self.out.push(single(op.clone())?);
                for column in column_names {
                    self.writes.push(SchemaChange::DropColumn {
                        table: table.to_string(),
                        column: column.value.clone(),
                    });
                }
            }
            AlterTableOperation::RenameTable { table_name } => {
                self.out.push(single(op.clone())?);
                let to = match table_name {
                    RenameTableNameKind::To(name) | RenameTableNameKind::As(name) => {
                        object_name(name)
                    }
                };
                self.writes.push(SchemaChange::RenameTable {
                    from: table.to_string(),
                    to,
                });
            }
            other => self.out.push(single(other.clone())?),
        }
        Ok(())
    }
}

impl RewritePass for DdlPass {
    fn name(&self) -> &'static str {
        "ddl"
    }

    fn rewrite(&self, stmt: &mut Statement, cx: &PassContext) -> Result<Rewrite, DbError> {
        let tree = stmt.tree().clone();
        let mut ddl = Ddl {
            cx,
            out: Vec::new(),
            writes: Vec::new(),
        };
        match &tree {
            Ast::CreateTable(create) => {
                let table = object_name(&create.name);
                if is_catalogue_table(&table) {
                    return Ok(Rewrite::Continue);
                }
                if create.like.is_some() {
                    return Err(unsupported("CREATE TABLE … LIKE", cx.target));
                }
                if create.query.is_some() {
                    return Err(unsupported("CREATE TABLE … SELECT", cx.target));
                }
                if has_auto_increment_option(&create.table_options) {
                    return Err(unsupported("the AUTO_INCREMENT=n table option", cx.target));
                }
                if create.if_not_exists
                    && let Some(introspector) = cx.introspector
                    && introspector
                        .tables(None)?
                        .iter()
                        .any(|t| t.eq_ignore_ascii_case(&table))
                {
                    return Ok(Rewrite::Emulate(EmulatedOp::Noop));
                }
                let catalogue = catalogue_table(&tree)?;
                let mut plain = tree.clone();
                if let Ast::CreateTable(c) = &mut plain {
                    c.table_options = CreateTableOptions::None;
                    c.constraints
                        .retain(|k| matches!(k, TableConstraint::PrimaryKey(_)));
                    for column in &mut c.columns {
                        column
                            .options
                            .retain(|o| !matches!(o.option, ColumnOption::Unique(_)));
                    }
                    // The table options go, so a binary table collation moves onto the text columns that inherit it.
                    if let Some(collation) = catalogue
                        .collation
                        .as_deref()
                        .filter(|c| c.ends_with("_bin"))
                    {
                        for (column, entry) in c.columns.iter_mut().zip(&catalogue.columns) {
                            let explicit = column
                                .options
                                .iter()
                                .any(|o| matches!(o.option, ColumnOption::Collation(_)));
                            if entry.collation.is_some() && !explicit {
                                column.options.push(sqlparser::ast::ColumnOptionDef {
                                    name: None,
                                    option: ColumnOption::Collation(
                                        sqlparser::ast::ObjectName::from(vec![
                                            sqlparser::ast::Ident::new(collation),
                                        ]),
                                    ),
                                });
                            }
                        }
                    }
                }
                ddl.out.push(Statement::new(plain, stmt.params().to_vec()));
                for index in &catalogue.indexes {
                    if matches!(index.kind, IndexKind::Unique | IndexKind::Index) {
                        ddl.out.push(create_index(&table, index)?);
                    }
                }
                // A temporary table belongs to one connection; the shared catalogue never records it.
                if !create.temporary {
                    ddl.writes.push(SchemaChange::CreateTable(catalogue));
                }
            }
            Ast::Drop {
                object_type: ObjectType::Table,
                names,
                if_exists,
                temporary,
                ..
            } => {
                for name in names {
                    let table = object_name(name);
                    let mut one = tree.clone();
                    if let Ast::Drop { names, .. } = &mut one {
                        *names = vec![name.clone()];
                    }
                    let _ = if_exists;
                    ddl.out.push(Statement::new(one, Vec::new()));
                    if !is_catalogue_table(&table) && !*temporary {
                        ddl.writes.push(SchemaChange::DropTable(table));
                    }
                }
            }
            Ast::Drop {
                object_type: ObjectType::Index,
                names,
                table: Some(table),
                ..
            } => {
                let table = object_name(table);
                for name in names {
                    let name = object_name(name);
                    ddl.out.push(drop_index(&table, &name)?);
                    ddl.writes.push(SchemaChange::DropIndex {
                        table: table.clone(),
                        name,
                    });
                }
            }
            Ast::CreateIndex(create) => {
                let table = object_name(&create.table_name);
                let name = create.name.as_ref().map(object_name);
                let kind = if create.unique {
                    IndexKind::Unique
                } else {
                    IndexKind::Index
                };
                ddl.add_index(&table, kind, name, parts(&create.columns))?;
            }
            Ast::RenameTable(renames) => {
                for rename in renames {
                    let (from, to) = (object_name(&rename.old_name), object_name(&rename.new_name));
                    ddl.out.push(parse_one(&format!(
                        "ALTER TABLE {} RENAME TO {}",
                        sql_ident(&from),
                        sql_ident(&to)
                    ))?);
                    ddl.writes.push(SchemaChange::RenameTable { from, to });
                }
            }
            Ast::AlterTable(alter) => {
                let table = object_name(&alter.name);
                if is_catalogue_table(&table) {
                    return Ok(Rewrite::Continue);
                }
                for op in &alter.operations {
                    ddl.alter(&table, op, &tree)?;
                }
            }
            _ => return Ok(Rewrite::Continue),
        }
        let mut parts = ddl.out;
        for change in &ddl.writes {
            parts.extend(catalogue_writes(change));
        }
        Ok(Rewrite::Split(parts))
    }
}
