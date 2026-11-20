//! The port's errors.

use std::fmt;

use speedyspec_data::DbError;
use speedyspec_wp_hooks::HookError;

/// Every failure of a `wpdb` method.
#[derive(Debug, Clone)]
pub enum WpDbError {
    /// `$this->ready` is false.
    NotReady,
    /// The `query` filter left nothing to run.
    EmptyQuery,
    /// The query contains text its table's charset cannot hold.
    InvalidData(String),
    /// A helper's field failed `process_fields`.
    ProcessFields(String),
    /// `set_prefix` with characters other than `[a-z0-9_]`.
    InvalidPrefix,
    Db(DbError),
    Hook(HookError),
    /// A `wp-content/db.php` drop-in is present.
    DropInRefused {
        path: String,
    },
    Config(String),
    /// `bail()` recorded an error instead of dying.
    Bailed {
        code: String,
        message: String,
    },
}

impl WpDbError {
    /// What `$wpdb->last_error` holds after this error.
    pub fn last_error(&self) -> String {
        match self {
            WpDbError::Db(DbError::Engine { message, .. })
            | WpDbError::Db(DbError::Constraint { message, .. })
            | WpDbError::Db(DbError::Deadlock { message, .. })
            | WpDbError::Db(DbError::Connection { message, .. }) => message.clone(),
            WpDbError::Db(DbError::Parse {
                message,
                line,
                column,
            }) => {
                format!("Could not parse the query: {message} at line {line}, column {column}")
            }
            WpDbError::Db(DbError::Unsupported { construct, dialect }) => {
                format!("{construct} is not supported on {dialect}")
            }
            WpDbError::Db(other) => other.to_string(),
            WpDbError::InvalidData(m) | WpDbError::ProcessFields(m) => m.clone(),
            WpDbError::Hook(e) => e.message.clone(),
            WpDbError::NotReady | WpDbError::EmptyQuery | WpDbError::InvalidPrefix => String::new(),
            WpDbError::DropInRefused { .. } | WpDbError::Config(_) => self.to_string(),
            WpDbError::Bailed { message, .. } => message.clone(),
        }
    }
}

impl fmt::Display for WpDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WpDbError::NotReady => f.write_str("the database connection is not ready"),
            WpDbError::EmptyQuery => f.write_str("the query is empty"),
            WpDbError::InvalidData(m) | WpDbError::ProcessFields(m) => f.write_str(m),
            WpDbError::InvalidPrefix => f.write_str("Invalid database prefix"),
            WpDbError::Db(e) => write!(f, "{e}"),
            WpDbError::Hook(e) => write!(f, "{e}"),
            WpDbError::DropInRefused { path } => write!(
                f,
                "the database drop-in {path} is not supported: SpeedySpec's native wpdb replaces it; remove the file"
            ),
            WpDbError::Config(m) => f.write_str(m),
            WpDbError::Bailed { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

impl std::error::Error for WpDbError {}

impl From<DbError> for WpDbError {
    fn from(e: DbError) -> Self {
        WpDbError::Db(e)
    }
}

impl From<HookError> for WpDbError {
    fn from(e: HookError) -> Self {
        WpDbError::Hook(e)
    }
}
