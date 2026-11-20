//! MySQL's session variables, as an engine that is not MySQL keeps and reports them.

use std::collections::BTreeMap;

use crate::like::mysql_like;

/// A connection's session variables: MySQL 8.4's defaults until a `SET` changes them.
#[derive(Debug, Clone)]
pub struct SessionVars {
    database: String,
    values: BTreeMap<String, String>,
    defaults: BTreeMap<String, String>,
}

fn default_collation(charset: &str) -> &'static str {
    match charset.to_ascii_lowercase().as_str() {
        "utf8" | "utf8mb3" => "utf8mb3_general_ci",
        "latin1" => "latin1_swedish_ci",
        "ascii" => "ascii_general_ci",
        "binary" => "binary",
        _ => "utf8mb4_0900_ai_ci",
    }
}

impl SessionVars {
    pub fn mysql_defaults(database: &str, version: &str) -> SessionVars {
        let defaults: BTreeMap<String, String> = [
            ("version", version),
            ("version_comment", "SpeedySpec"),
            (
                "sql_mode",
                "ONLY_FULL_GROUP_BY,STRICT_TRANS_TABLES,NO_ZERO_IN_DATE,NO_ZERO_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION",
            ),
            ("character_set_client", "utf8mb4"),
            ("character_set_connection", "utf8mb4"),
            ("character_set_results", "utf8mb4"),
            ("collation_connection", "utf8mb4_0900_ai_ci"),
            ("time_zone", "+00:00"),
            ("max_allowed_packet", "67108864"),
            ("lower_case_table_names", "0"),
            ("autocommit", "ON"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        SessionVars {
            database: database.to_string(),
            values: defaults.clone(),
            defaults,
        }
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn set(&mut self, name: &str, value: &str) {
        self.values
            .insert(name.to_ascii_lowercase(), value.to_string());
    }

    /// `SET NAMES charset [COLLATE collation]`; MySQL 8.4 reports the `utf8` alias by its real name, `utf8mb3`.
    pub fn set_names(&mut self, charset: &str, collation: Option<&str>) {
        let real = |name: &str| match name.strip_prefix("utf8_") {
            Some(rest) => format!("utf8mb3_{rest}"),
            None if name.eq_ignore_ascii_case("utf8") => "utf8mb3".to_string(),
            None => name.to_string(),
        };
        let charset = real(charset);
        let collation = collation.map(real);
        let (charset, collation) = (charset.as_str(), collation.as_deref());
        for name in [
            "character_set_client",
            "character_set_connection",
            "character_set_results",
        ] {
            self.set(name, charset);
        }
        let collation =
            collation.map_or_else(|| default_collation(charset).to_string(), str::to_string);
        self.set("collation_connection", &collation);
    }

    /// The variables matching a MySQL `LIKE` pattern (case-insensitive), sorted by name.
    pub fn like(&self, pattern: Option<&str>) -> Vec<(String, String)> {
        self.values
            .iter()
            .filter(|(name, _)| pattern.is_none_or(|p| mysql_like(p, name, true)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Back to the defaults, as a connection reset does.
    pub fn reset(&mut self) {
        self.values = self.defaults.clone();
    }
}
