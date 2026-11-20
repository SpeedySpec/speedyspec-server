//! The `SAVEQUERIES` query log (filled by the query log task).

use speedyspec_data::Statement;

use crate::WpDb;

impl WpDb {
    /// Runs after each executed statement.
    pub(crate) fn after_run(&self, _stmt: &Statement) {}
}
