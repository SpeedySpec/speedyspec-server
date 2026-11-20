//! Connection methods (filled by the connection task).

use speedyspec_data::{DbError, SqlStore};
use speedyspec_data_query::parse_one;

use crate::{WpDb, WpDbError};

impl WpDb {
    /// `wpdb::check_connection`.
    pub fn check_connection(&self, _allow_bail: bool) -> Result<bool, WpDbError> {
        match self.store.query(&parse_one("SELECT 1")?) {
            Ok(_) => Ok(true),
            Err(DbError::Connection { .. }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}
