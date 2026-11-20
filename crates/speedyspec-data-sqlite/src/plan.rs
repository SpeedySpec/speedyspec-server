//! What SQLite runs for one MySQL statement (the plan), and how a translated tree becomes its steps.

use speedyspec_data::{EmittedSql, EmulatedOp, Statement, StatementKind};
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOption, Expr, FromTable, OnConflict, OnConflictAction,
    OnInsert, RenameTableNameKind, SelectItem, SetExpr, Statement as Ast, TableFactor, TableObject,
};

use crate::passes::REBUILD_SUFFIX;
use speedyspec_data_translate::{is_catalogue_table, object_name};

/// How a step's affected-row count is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountRule {
    /// `sqlite3_changes()`.
    Changes,
    /// MySQL's upsert count: 1 for an insert, 2 for an update that changed the row.
    Upsert,
    /// The step's count is not reported.
    None,
}

/// How a step's insert id is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertIdRule {
    None,
    LastRowid,
    /// From the step's `RETURNING` row.
    Returning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqliteStep {
    pub sql: EmittedSql,
    pub count: CountRule,
    pub insert_id: InsertIdRule,
    pub returns_rows: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlitePlan {
    Run {
        steps: Vec<SqliteStep>,
        /// The `SELECT COUNT(*)` that answers a later `FOUND_ROWS()`.
        found_rows: Option<EmittedSql>,
        /// Whether a step reads `information_schema`, which the engine fills first.
        information_schema: bool,
    },
    Emulated(EmulatedOp),
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// MySQL's `ON UPDATE CURRENT_TIMESTAMP`, which SQLite has no column clause for: set the column when an update left
/// it as it was.
fn on_update_trigger(table: &str, column: &str) -> String {
    let (t, c) = (quoted(table), quoted(column));
    format!(
        "CREATE TRIGGER {} AFTER UPDATE ON {t} FOR EACH ROW WHEN NEW.{c} IS OLD.{c} BEGIN UPDATE {t} SET {c} = CURRENT_TIMESTAMP WHERE rowid = NEW.rowid; END",
        quoted(&format!("{table}__{column}__on_update"))
    )
}

/// Takes `ON UPDATE` off each column, naming the columns it was on.
fn take_on_update(columns: &mut [ColumnDef]) -> Vec<String> {
    let mut names = Vec::new();
    for column in columns {
        let before = column.options.len();
        column
            .options
            .retain(|o| !matches!(o.option, ColumnOption::OnUpdate(_)));
        if column.options.len() != before {
            names.push(column.name.value.clone());
        }
    }
    names
}

/// Whether a write to `name` is the engine's own bookkeeping, not a row MySQL would count.
fn bookkeeping(name: &sqlparser::ast::ObjectName) -> bool {
    let first = name
        .0
        .first()
        .map(|p| p.to_string().trim_matches('"').to_ascii_lowercase());
    let table = object_name(name);
    first.as_deref() == Some("temp")
        || table.eq_ignore_ascii_case("sqlite_sequence")
        || is_catalogue_table(&table)
}

fn returns_rowid(returning: &Option<Vec<SelectItem>>) -> bool {
    matches!(returning.as_deref(), Some([SelectItem::UnnamedExpr(Expr::Identifier(i))]) if i.value.eq_ignore_ascii_case("rowid"))
}

/// How a step's count and insert id are read; every step of a DDL statement reports none.
fn rules(kind: StatementKind, tree: &Ast) -> (CountRule, InsertIdRule, bool) {
    if kind == StatementKind::Ddl {
        return (CountRule::None, InsertIdRule::None, false);
    }
    match tree {
        Ast::Query(_) => (CountRule::None, InsertIdRule::None, true),
        Ast::Insert(insert) => {
            let TableObject::TableName(name) = &insert.table else {
                return (CountRule::None, InsertIdRule::None, false);
            };
            if bookkeeping(name) {
                return (CountRule::None, InsertIdRule::None, false);
            }
            let upsert = matches!(
                &insert.on,
                Some(OnInsert::OnConflict(OnConflict {
                    action: OnConflictAction::DoUpdate(_),
                    ..
                }))
            );
            let single = match insert.source.as_deref().map(|q| q.body.as_ref()) {
                Some(SetExpr::Values(values)) => values.rows.len() == 1,
                Some(_) => false,
                None => true,
            };
            let insert_id = if returns_rowid(&insert.returning) {
                InsertIdRule::Returning
            } else if single {
                InsertIdRule::LastRowid
            } else {
                InsertIdRule::None
            };
            (
                if upsert {
                    CountRule::Upsert
                } else {
                    CountRule::Changes
                },
                insert_id,
                false,
            )
        }
        Ast::Update(update) => match &update.table.relation {
            TableFactor::Table { name, .. } if !bookkeeping(name) => {
                (CountRule::Changes, InsertIdRule::None, false)
            }
            _ => (CountRule::None, InsertIdRule::None, false),
        },
        Ast::Delete(delete) => {
            let tables = match &delete.from {
                FromTable::WithFromKeyword(t) | FromTable::WithoutKeyword(t) => t,
            };
            let counted = tables.iter().any(
                |t| matches!(&t.relation, TableFactor::Table { name, .. } if !bookkeeping(name)),
            );
            (
                if counted {
                    CountRule::Changes
                } else {
                    CountRule::None
                },
                InsertIdRule::None,
                false,
            )
        }
        _ => (CountRule::None, InsertIdRule::None, false),
    }
}

/// The steps for the translated statements of one original statement of kind `kind`.
pub(crate) fn emit(kind: StatementKind, statements: Vec<Statement>) -> Vec<SqliteStep> {
    let mut steps: Vec<SqliteStep> = Vec::new();
    // Triggers for a rebuild's staging table wait until it has the table's own name.
    let mut waiting: Vec<(String, String)> = Vec::new();
    for mut stmt in statements {
        let mut triggers = Vec::new();
        match stmt.tree_mut() {
            Ast::CreateTable(create) => {
                let table = object_name(&create.name);
                for column in take_on_update(&mut create.columns) {
                    match table.strip_suffix(REBUILD_SUFFIX) {
                        Some(final_name) => waiting.push((final_name.to_string(), column)),
                        None => triggers.push(on_update_trigger(&table, &column)),
                    }
                }
            }
            Ast::AlterTable(alter) => {
                let table = object_name(&alter.name);
                for op in &mut alter.operations {
                    match op {
                        AlterTableOperation::AddColumn { column_def, .. } => {
                            for column in take_on_update(std::slice::from_mut(column_def)) {
                                triggers.push(on_update_trigger(&table, &column));
                            }
                        }
                        AlterTableOperation::RenameTable {
                            table_name: RenameTableNameKind::To(to) | RenameTableNameKind::As(to),
                        } if table.ends_with(REBUILD_SUFFIX) => {
                            let to = object_name(to);
                            waiting.retain(|(t, column)| {
                                let ready = t.eq_ignore_ascii_case(&to);
                                if ready {
                                    triggers.push(on_update_trigger(&to, column));
                                }
                                !ready
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let (tree, params) = stmt.into_parts();
        let (count, insert_id, returns_rows) = rules(kind, &tree);
        steps.push(SqliteStep {
            sql: EmittedSql {
                sql: tree.to_string(),
                params,
            },
            count,
            insert_id,
            returns_rows,
        });
        steps.extend(triggers.into_iter().map(|t| SqliteStep {
            sql: EmittedSql {
                sql: t,
                params: Vec::new(),
            },
            count: CountRule::None,
            insert_id: InsertIdRule::None,
            returns_rows: false,
        }));
    }
    steps
}
