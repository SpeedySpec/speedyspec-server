//! Application identity and kind.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::DomainError;

/// An application's id: `[a-z][a-z0-9-]*`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppId(String);

impl AppId
{
    /// Checks `value` against the id rule.
    pub fn parse(value: &str) -> Result<AppId, DomainError>
    {
        let mut chars = value.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if first_ok && rest_ok { Ok(AppId(value.to_string())) } else { Err(DomainError::BadAppId(value.to_string())) }
    }

    pub fn as_str(&self) -> &str
    {
        &self.0
    }
}

impl fmt::Display for AppId
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.write_str(&self.0)
    }
}

impl Serialize for AppId
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AppId
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
    {
        let value = String::deserialize(deserializer)?;
        AppId::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// What an app's releases contain, and so how one goes live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind
{
    /// The server binary itself; goes live by socket handoff.
    Server,
    /// A directory of files served from memory.
    Static,
    /// A PHP application run by the worker pool.
    Php,
}

impl AppKind
{
    pub fn as_str(self) -> &'static str
    {
        match self
        {
            AppKind::Server => "server",
            AppKind::Static => "static",
            AppKind::Php => "php",
        }
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn app_id_accepts_valid()
    {
        for id in ["admin", "frontend-api", "a1", "a"]
        {
            assert_eq!(AppId::parse(id).unwrap().as_str(), id);
        }
    }

    #[test]
    fn app_id_rejects_invalid()
    {
        for id in ["", "Admin", "1a", "-a", "a_b", "a b"]
        {
            assert!(AppId::parse(id).is_err(), "{id:?} must be rejected");
        }
    }

    #[test]
    fn app_kind_is_lowercase_in_json()
    {
        assert_eq!(serde_json::to_string(&AppKind::Static).unwrap(), "\"static\"");
        assert_eq!(serde_json::from_str::<AppKind>("\"php\"").unwrap(), AppKind::Php);
    }
}
