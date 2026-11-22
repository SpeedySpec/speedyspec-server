//! The database the worker's native WordPress areas use, read from the worker's environment.

use speedyspec_wp_db::WpDbConfig;

/// The worker's database, from `DB_NAME`, `DB_USER`, `DB_PASSWORD`, `DB_HOST`, `DB_CHARSET`, `DB_COLLATE` and
/// `TABLE_PREFIX` (WordPress's `wp-config.php` names). The areas are fixed when PHP boots, so the configuration must
/// exist before any PHP runs; the server already passes each app's credentials through the worker's environment.
/// `None` when `DB_NAME` is unset or empty: the worker then serves without the database areas.
pub fn database_config(env: impl Fn(&str) -> Option<String>) -> Result<Option<WpDbConfig>, String> {
    let name = env("DB_NAME").unwrap_or_default();
    if name.is_empty() {
        return Ok(None);
    }
    let optional = |key: &str| env(key).filter(|value| !value.is_empty());
    let table_prefix = env("TABLE_PREFIX").unwrap_or_else(|| "wp_".to_string());
    // wp-settings.php refuses any other character in $table_prefix.
    if !table_prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "TABLE_PREFIX may contain only letters, digits and underscores: {table_prefix}"
        ));
    }
    Ok(Some(WpDbConfig {
        user: env("DB_USER").unwrap_or_default(),
        password: env("DB_PASSWORD").unwrap_or_default(),
        name,
        host: env("DB_HOST").unwrap_or_else(|| "localhost".to_string()),
        charset: optional("DB_CHARSET"),
        collate: optional("DB_COLLATE"),
        table_prefix,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn no_db_name_means_no_database() {
        assert_eq!(database_config(env(&[])), Ok(None));
        assert_eq!(database_config(env(&[("DB_NAME", "")])), Ok(None));
    }

    #[test]
    fn defaults_match_wordpress() {
        let config = database_config(env(&[("DB_NAME", "wp")])).unwrap().unwrap();
        assert_eq!(
            config,
            WpDbConfig {
                user: String::new(),
                password: String::new(),
                name: "wp".into(),
                host: "localhost".into(),
                charset: None,
                collate: None,
                table_prefix: "wp_".into(),
            }
        );
    }

    #[test]
    fn host_passes_through_unparsed() {
        let config = database_config(env(&[
            ("DB_NAME", "wp"),
            ("DB_HOST", "localhost:/tmp/mysql.sock"),
            ("DB_CHARSET", "utf8mb4"),
            ("DB_COLLATE", ""),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(config.host, "localhost:/tmp/mysql.sock");
        assert_eq!(config.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(config.collate, None);
    }

    #[test]
    fn bad_prefix_is_refused() {
        let err = database_config(env(&[("DB_NAME", "wp"), ("TABLE_PREFIX", "wp-")])).unwrap_err();
        assert!(err.contains("wp-"), "{err}");
    }
}
