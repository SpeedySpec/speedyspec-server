//! The collation and functions the dialect's output calls, installed once per schema.

use postgres::Client;
use speedyspec_data::DbError;

use crate::error::map_error;

/// The support objects, public so a CLI can install them ahead of time.
pub const SUPPORT_SQL: &str = include_str!("support.sql");

const MARKER: &str = "speedyspec-support 2";

/// Installs the support objects in the current schema unless the marker says they are there.
pub(crate) fn ensure(client: &mut Client, engine: &str) -> Result<(), DbError> {
    let installed: Option<String> = client
        .query_opt(
            "SELECT obj_description(p.oid, 'pg_proc') FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE p.proname = 'speedyspec_signed' AND n.nspname = current_schema()",
            &[],
        )
        .map_err(|e| map_error(engine, e))?
        .and_then(|r| r.get(0));
    if installed.as_deref() == Some(MARKER) {
        return Ok(());
    }
    let mut tx = client.transaction().map_err(|e| map_error(engine, e))?;
    tx.execute("SELECT pg_advisory_xact_lock(7316)", &[])
        .map_err(|e| map_error(engine, e))?;
    tx.batch_execute(SUPPORT_SQL)
        .map_err(|e| map_error(engine, e))?;
    tx.commit().map_err(|e| map_error(engine, e))
}
