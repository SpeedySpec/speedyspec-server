//! What the database server can do (`has_cap`, `db_version`).

use std::cmp::Ordering;

/// The server's version and what it supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCaps {
    pub version: String,
    pub server_info: String,
}

fn version_prefix(info: &str) -> String {
    info.chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

/// PHP's `version_compare` for dotted numbers: part by part, a longer version greater when the rest is equal.
pub(crate) fn version_compare(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| {
        v.split('.')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    for (x, y) in a.iter().zip(&b) {
        match x.cmp(y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

impl ServerCaps {
    /// From `mysqli_get_server_info()`; MariaDB's `5.5.5-` prefix is stripped, as `has_cap` does.
    pub fn new(server_info: &str) -> ServerCaps {
        let info = server_info
            .strip_prefix("5.5.5-")
            .filter(|_| server_info.contains("MariaDB"))
            .unwrap_or(server_info);
        ServerCaps {
            version: version_prefix(info),
            server_info: server_info.to_string(),
        }
    }

    fn at_least(&self, version: &str) -> bool {
        version_compare(&self.version, version) != Ordering::Less
    }

    /// `wpdb::has_cap`.
    pub fn has_cap(&self, cap: &str) -> bool {
        match cap.to_ascii_lowercase().as_str() {
            "collation" | "group_concat" | "subqueries" => self.at_least("4.1"),
            "set_charset" => self.at_least("5.0.7"),
            "utf8mb4" | "identifier_placeholders" => true,
            "utf8mb4_520" => self.at_least("5.6"),
            _ => false,
        }
    }
}
