//! `DB_HOST` parsing and the connection configuration.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use speedyspec_data::EngineConfig;

use speedyspec_wp_db_contracts::WpDbError;

/// A parsed `DB_HOST` (Value Object), as `wpdb::parse_db_host` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbHost {
    pub host: String,
    pub port: Option<u16>,
    pub socket: Option<String>,
    pub is_ipv6: bool,
}

impl DbHost {
    /// `wpdb::parse_db_host`; `None` where it returns false.
    pub fn parse(db_host: &str) -> Option<DbHost> {
        static IPV6: OnceLock<Regex> = OnceLock::new();
        static IPV4: OnceLock<Regex> = OnceLock::new();
        let (mut host, mut socket) = (db_host, None);
        if let Some(pos) = host.find(":/") {
            socket = Some(host[pos + 1..].to_string());
            host = &host[..pos];
        }
        let is_ipv6 = host.matches(':').count() > 1;
        let pattern = if is_ipv6 {
            IPV6.get_or_init(|| {
                Regex::new(r"^(?:\[)?(?P<host>[0-9a-fA-F:]+)(?:\]:(?P<port>\d+))?").unwrap()
            })
        } else {
            IPV4.get_or_init(|| Regex::new(r"^(?P<host>[^:/]*)(?::(?P<port>\d+))?").unwrap())
        };
        let captures = pattern.captures(host)?;
        let host = captures.name("host").map_or("", |m| m.as_str());
        // PHP's `empty()`: "" and "0" are empty; then `absint`.
        let port = captures
            .name("port")
            .map(|m| m.as_str())
            .filter(|p| !p.is_empty() && *p != "0")
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p != 0);
        Some(DbHost {
            host: if host == "0" {
                String::new()
            } else {
                host.to_string()
            },
            port,
            socket,
            is_ipv6,
        })
    }
}

/// The `DB_*` constants and `$table_prefix`; `None` where a constant is undefined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WpDbConfig {
    pub user: String,
    pub password: String,
    pub name: String,
    pub host: String,
    pub charset: Option<String>,
    pub collate: Option<String>,
    pub table_prefix: String,
}

/// Characters kept as they are in a URL's user, password and path.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

impl WpDbConfig {
    /// The `mysql://` engine configuration for this site.
    pub fn engine_config(&self, engine: &str) -> Result<EngineConfig, WpDbError> {
        let parsed = DbHost::parse(&self.host).ok_or_else(|| {
            WpDbError::Config(format!("DB_HOST could not be parsed: {}", self.host))
        })?;
        let host = if parsed.is_ipv6 {
            format!("[{}]", parsed.host)
        } else {
            parsed.host.clone()
        };
        let port = parsed.port.map(|p| format!(":{p}")).unwrap_or_default();
        let url = format!(
            "mysql://{}:{}@{host}{port}/{}",
            utf8_percent_encode(&self.user, COMPONENT),
            utf8_percent_encode(&self.password, COMPONENT),
            utf8_percent_encode(&self.name, COMPONENT),
        );
        let mut options = BTreeMap::new();
        if let Some(socket) = parsed.socket {
            options.insert("socket".to_string(), socket);
        }
        if let Some(charset) = &self.charset {
            options.insert("charset".to_string(), charset.clone());
        }
        if let Some(collate) = &self.collate {
            options.insert("collate".to_string(), collate.clone());
        }
        Ok(EngineConfig {
            name: engine.to_string(),
            url,
            options,
        })
    }
}
