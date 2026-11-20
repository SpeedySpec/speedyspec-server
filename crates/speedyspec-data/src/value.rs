//! Values sent to and read from an engine.

/// One value bound to a placeholder or read from a column.
#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Int(i64),
    UInt(u64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl DbValue {
    /// The value as mysqli's text protocol gives it to PHP: `None` for `NULL`, numbers in decimal.
    pub fn to_mysql_text(&self) -> Option<Vec<u8>> {
        match self {
            DbValue::Null => None,
            DbValue::Int(v) => Some(v.to_string().into_bytes()),
            DbValue::UInt(v) => Some(v.to_string().into_bytes()),
            DbValue::Float(v) => Some(format!("{v}").into_bytes()),
            DbValue::Text(v) => Some(v.as_bytes().to_vec()),
            DbValue::Bytes(v) => Some(v.clone()),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, DbValue::Null)
    }
}

impl From<i64> for DbValue {
    fn from(v: i64) -> Self {
        DbValue::Int(v)
    }
}

impl From<u64> for DbValue {
    fn from(v: u64) -> Self {
        DbValue::UInt(v)
    }
}

impl From<f64> for DbValue {
    fn from(v: f64) -> Self {
        DbValue::Float(v)
    }
}

impl From<&str> for DbValue {
    fn from(v: &str) -> Self {
        DbValue::Text(v.to_string())
    }
}

impl From<String> for DbValue {
    fn from(v: String) -> Self {
        DbValue::Text(v)
    }
}

impl From<Vec<u8>> for DbValue {
    fn from(v: Vec<u8>) -> Self {
        DbValue::Bytes(v)
    }
}

impl<T: Into<DbValue>> From<Option<T>> for DbValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(DbValue::Null, Into::into)
    }
}
