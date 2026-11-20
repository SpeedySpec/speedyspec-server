//! `PostgresIntrospector`: the schema in MySQL's terms, from the MySQL catalogue the DDL wrote, falling back to
//! `pg_catalog` for tables the catalogue does not record.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_data::{ColumnDef, DbError, IndexDef, Introspector, Rows, Statement};
use speedyspec_data_translate::{
    CatalogueTable, catalogue_tables, is_catalogue_table, mysql_like, read_catalogue,
};

use crate::connection::Connection;
use crate::dialect::PostgresDialect;
use crate::exec;
use crate::plan::PgPlan;

pub struct PostgresIntrospector {
    conn: Rc<RefCell<Connection>>,
    dialect: PostgresDialect,
    /// This connection's temporary tables, as MySQL would describe them; never in the shared catalogue.
    temporary: RefCell<std::collections::HashMap<String, CatalogueTable>>,
}

/// MySQL's name for a PostgreSQL type, for tables the catalogue does not record.
fn mysql_type(pg: &str) -> String {
    match pg {
        "bigint" => "bigint".into(),
        "integer" => "int".into(),
        "smallint" => "smallint".into(),
        "double precision" => "double".into(),
        "real" => "float".into(),
        "text" => "text".into(),
        "bytea" => "longblob".into(),
        "boolean" => "tinyint(1)".into(),
        "date" => "date".into(),
        t if t.starts_with("timestamp") => "datetime".into(),
        t if t.starts_with("time") => "time".into(),
        t if t.starts_with("numeric") => t.replacen("numeric", "decimal", 1),
        t if t.starts_with("character varying") => t.replacen("character varying", "varchar", 1),
        t if t.starts_with("character") => t.replacen("character", "char", 1),
        other => other.into(),
    }
}

impl PostgresIntrospector {
    pub(crate) fn new(conn: Rc<RefCell<Connection>>) -> PostgresIntrospector {
        PostgresIntrospector {
            conn,
            dialect: PostgresDialect::new(None),
            temporary: RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Runs one of the catalogue's own statements (MySQL text), planned without the schema it reads.
    fn run(&self, stmt: &Statement) -> Result<Rows, DbError> {
        let PgPlan::Sql(emitted) = self.dialect.plan(stmt)? else {
            return Ok(Rows::default());
        };
        let mut conn = self.conn.borrow_mut();
        let engine = conn.engine().to_string();
        let mut rows = Rows::default();
        for e in &emitted {
            rows = exec::query(&mut conn, &engine, &e.sql)?;
        }
        Ok(rows)
    }

    pub(crate) fn catalogue(&self, table: &str) -> Result<Option<CatalogueTable>, DbError> {
        if let Some(temp) = self.temporary.borrow().get(table) {
            // A temporary table hides the permanent one of its name, as in MySQL.
            return Ok(Some(temp.clone()));
        }
        read_catalogue(&|s| self.run(s), table)
    }

    /// Records or forgets a temporary table this connection made or dropped.
    pub(crate) fn track_temporary(&self, stmt: &Statement) -> Result<(), DbError> {
        match stmt.tree() {
            sqlparser::ast::Statement::CreateTable(create) if create.temporary => {
                let table = speedyspec_data_translate::catalogue_table(stmt.tree())?;
                self.temporary
                    .borrow_mut()
                    .insert(table.name.clone(), table);
            }
            sqlparser::ast::Statement::Drop { names, .. } => {
                for name in names {
                    self.temporary
                        .borrow_mut()
                        .remove(&speedyspec_data_translate::object_name(name));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A connection reset ends its temporary tables.
    pub(crate) fn forget_temporary(&self) {
        self.temporary.borrow_mut().clear();
    }

    fn fixed(
        &self,
        sql: &str,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> Result<Vec<postgres::Row>, DbError> {
        let mut conn = self.conn.borrow_mut();
        let engine = conn.engine().to_string();
        conn.client()?
            .query(sql, params)
            .map_err(|e| crate::error::map_error(&engine, e))
    }
}

impl Introspector for PostgresIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        let rows = self.fixed("SELECT current_setting('server_version')", &[])?;
        let version: String = rows.first().map(|r| r.get(0)).unwrap_or_default();
        Ok(format!("8.4.0-speedyspec-postgres-{version}"))
    }

    fn tables(&self, like: Option<&str>) -> Result<Vec<String>, DbError> {
        let mut tables = catalogue_tables(&|s| self.run(s))?;
        for row in self.fixed(
            "SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = current_schema() AND c.relkind IN ('r', 'p') ORDER BY c.relname",
            &[],
        )? {
            let name: String = row.get(0);
            if !tables.contains(&name) {
                tables.push(name);
            }
        }
        tables.retain(|t| !is_catalogue_table(t) && like.is_none_or(|p| mysql_like(p, t, false)));
        tables.sort();
        Ok(tables)
    }

    fn columns(&self, table: &str) -> Result<Vec<ColumnDef>, DbError> {
        if !is_catalogue_table(table)
            && let Some(t) = self.catalogue(table)?
        {
            return Ok(t.column_defs());
        }
        let rows = self.fixed(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
             pg_get_expr(d.adbin, d.adrelid), a.attidentity <> '', \
             EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY(i.indkey)) \
             FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE n.nspname = current_schema() AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            &[&table],
        )?;
        Ok(rows
            .iter()
            .map(|r| {
                let default: Option<String> = r.get(3);
                ColumnDef {
                    name: r.get(0),
                    sql_type: mysql_type(&r.get::<_, String>(1)),
                    nullable: r.get(2),
                    key: if r.get(5) {
                        "PRI".into()
                    } else {
                        String::new()
                    },
                    default: default.map(|d| {
                        d.split("::")
                            .next()
                            .unwrap_or(&d)
                            .trim_matches('\'')
                            .to_string()
                    }),
                    extra: if r.get(4) {
                        "auto_increment".into()
                    } else {
                        String::new()
                    },
                    collation: None,
                }
            })
            .collect())
    }

    fn indexes(&self, table: &str) -> Result<Vec<IndexDef>, DbError> {
        if !is_catalogue_table(table)
            && let Some(t) = self.catalogue(table)?
        {
            return Ok(t.index_defs());
        }
        let rows = self.fixed(
            "SELECT i.relname, x.indisunique, x.indisprimary, a.attname FROM pg_index x \
             JOIN pg_class t ON t.oid = x.indrelid JOIN pg_class i ON i.oid = x.indexrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             JOIN LATERAL unnest(x.indkey) WITH ORDINALITY k(attnum, seq) ON true \
             JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum \
             WHERE n.nspname = current_schema() AND t.relname = $1 ORDER BY x.indisprimary DESC, i.relname, k.seq",
            &[&table],
        )?;
        let mut out: Vec<IndexDef> = Vec::new();
        for r in &rows {
            let primary: bool = r.get(2);
            let pg_name: String = r.get(0);
            let name = if primary {
                "PRIMARY".to_string()
            } else {
                pg_name
                    .split_once("__")
                    .map_or(pg_name.clone(), |(_, n)| n.to_string())
            };
            let column: String = r.get(3);
            match out.iter_mut().find(|i| i.name == name) {
                Some(index) => index.columns.push((column, None)),
                None => out.push(IndexDef {
                    name,
                    unique: r.get(1),
                    columns: vec![(column, None)],
                }),
            }
        }
        Ok(out)
    }

    fn table_collation(&self, table: &str) -> Result<Option<String>, DbError> {
        Ok(self.catalogue(table)?.and_then(|t| t.collation))
    }

    fn index_type(&self, table: &str, index: &str) -> Result<Option<String>, DbError> {
        Ok(self.catalogue(table)?.and_then(|t| t.index_type(index)))
    }
}
