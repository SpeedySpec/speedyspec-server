//! Transport names.

use speedyspec_operation_contracts::INTERNAL_TRANSPORT;

/// A transport's name: lowercase letters, digits and dashes. `internal` is reserved for calls from inside the
/// application.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransportName(String);

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error(
    "{name} is not a transport name (lowercase letters, digits and dashes; internal is reserved)"
)]
pub struct TransportNameError {
    pub name: String,
}

impl TransportName {
    pub fn parse(name: &str) -> Result<TransportName, TransportNameError> {
        let valid = !name.is_empty()
            && name != INTERNAL_TRANSPORT
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid {
            Ok(TransportName(name.to_owned()))
        } else {
            Err(TransportNameError {
                name: name.to_owned(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
