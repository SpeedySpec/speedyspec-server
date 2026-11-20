//! Running one emitted statement on a client: parameters bound as `PgParam`, rows decoded as `PgCell`.

use std::rc::Rc;

use postgres::types::ToSql;
use speedyspec_data::{DbError, EmittedSql, Row, Rows};

use crate::connection::Connection;
use crate::error::map_error;
use crate::value::{PgCell, PgParam, mysql_column_info};

/// The statement's rows.
pub(crate) fn query(
    conn: &mut Connection,
    engine: &str,
    sql: &EmittedSql,
) -> Result<Rows, DbError> {
    let params: Vec<PgParam> = sql.params.iter().map(PgParam).collect();
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
    let statement = conn.prepare(&sql.sql)?;
    let client = conn.client()?;
    // A statement names fewer parameters than the plan carries: bind only those it has.
    let refs = &refs[..statement.params().len().min(refs.len())];
    let rows = client
        .query(&statement, refs)
        .map_err(|e| map_error(engine, e))?;
    let names: Rc<[String]> = statement
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();
    let mut values: Vec<Vec<speedyspec_data::DbValue>> = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut out = Vec::with_capacity(names.len());
        for i in 0..names.len() {
            let cell: PgCell = row.try_get(i).map_err(|e| map_error(engine, e))?;
            out.push(cell.0);
        }
        values.push(out);
    }
    let columns = statement
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let column: Vec<speedyspec_data::DbValue> =
                values.iter().map(|r| r[i].clone()).collect();
            mysql_column_info(c, None, &column, i)
        })
        .collect();
    Ok(Rows {
        columns,
        rows: values
            .into_iter()
            .map(|v| Row::new(names.clone(), v))
            .collect(),
    })
}

/// The statement's affected-row count.
pub(crate) fn execute(
    conn: &mut Connection,
    engine: &str,
    sql: &EmittedSql,
) -> Result<u64, DbError> {
    let params: Vec<PgParam> = sql.params.iter().map(PgParam).collect();
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
    let statement = conn.prepare(&sql.sql)?;
    let client = conn.client()?;
    let refs = &refs[..statement.params().len().min(refs.len())];
    client
        .execute(&statement, refs)
        .map_err(|e| map_error(engine, e))
}
