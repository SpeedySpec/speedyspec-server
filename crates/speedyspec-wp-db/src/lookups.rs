//! Charset lookups (filled by the helpers task).

use crate::{WpDb, WpDbError};

impl WpDb {
    /// The query-level charset check: the query with text its table's charset cannot hold stripped.
    pub(crate) fn strip_invalid_text_from_query(&self, sql: &[u8]) -> Result<Vec<u8>, WpDbError> {
        Ok(sql.to_vec())
    }
}
