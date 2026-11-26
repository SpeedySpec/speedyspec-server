//! `SqlSitemetaRepository` (Repository) and `SitemetaMapper` (Data Mapper): `wp_sitemeta` through the `wpdb` port,
//! with the statements `option.php` sends. Writes go through `$wpdb->insert`, `update` and `delete`, so charset
//! stripping matches WordPress's.

use std::rc::Rc;

use speedyspec_data::{DbError, Statement};
use speedyspec_data_query::Query;
use speedyspec_wp_db_contracts::{QueryOutcome, WpDbError, WpRow, WpValue, Wpdb};
use speedyspec_wp_options::{NetworkMeta, SerializedValue, SitemetaRepository};

/// A `wp_sitemeta` row to a `NetworkMeta` and back; `meta_id` stays the table's.
pub struct SitemetaMapper;

/// The formats `option.php` gives: `site_id` `%d`, `meta_key` and `meta_value` `%s`.
const FORMATS: [&str; 3] = ["%d", "%s", "%s"];

fn field(row: &WpRow, name: &str) -> Option<Vec<u8>> {
    row.fields
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v.clone())
}

impl SitemetaMapper {
    pub fn to_entity(network_id: i64, key: &str, row: &WpRow) -> NetworkMeta {
        NetworkMeta {
            network_id,
            key: key.to_string(),
            value: SerializedValue::new(field(row, "meta_value").unwrap_or_default()),
        }
    }

    /// `site_id`, `meta_key` and `meta_value`, in `FORMATS`' order.
    pub fn to_fields(meta: &NetworkMeta) -> Vec<(String, WpValue)> {
        vec![
            ("site_id".into(), WpValue::Int(meta.network_id)),
            ("meta_key".into(), WpValue::Str(meta.key.clone())),
            ("meta_value".into(), meta.value.to_value()),
        ]
    }
}

pub struct SqlSitemetaRepository {
    wpdb: Rc<dyn Wpdb>,
}

/// A failure `wpdb` hides is the empty answer (as `$wpdb` returns null or false); a hook's failure is not.
fn hidden(e: WpDbError) -> Result<(), DbError> {
    match e {
        WpDbError::Hook(e) => Err(DbError::Engine {
            engine: "wpdb".into(),
            code: Some("hook".into()),
            message: e.to_string(),
        }),
        _ => Ok(()),
    }
}

fn formats(n: usize) -> Vec<String> {
    FORMATS[..n].iter().map(|f| f.to_string()).collect()
}

impl SqlSitemetaRepository {
    pub fn new(wpdb: Rc<dyn Wpdb>) -> SqlSitemetaRepository {
        SqlSitemetaRepository { wpdb }
    }

    fn table(&self) -> String {
        self.wpdb.table_name("sitemeta")
    }

    fn rows(&self, built: Result<Statement, DbError>) -> Result<Rc<Vec<WpRow>>, DbError> {
        match self.wpdb.query_statement(built?) {
            Ok(QueryOutcome::Rows(rows)) => Ok(rows),
            Ok(_) => Ok(Rc::new(Vec::new())),
            Err(e) => hidden(e).map(|()| Rc::new(Vec::new())),
        }
    }

    fn written(&self, result: Result<u64, WpDbError>) -> Result<u64, DbError> {
        match result {
            Ok(n) => Ok(n),
            Err(e) => hidden(e).map(|()| 0),
        }
    }

    fn condition(network_id: i64, key: &str) -> Vec<(String, WpValue)> {
        vec![
            ("meta_key".into(), WpValue::Str(key.to_string())),
            ("site_id".into(), WpValue::Int(network_id)),
        ]
    }
}

impl SitemetaRepository for SqlSitemetaRepository {
    fn find(&self, network_id: i64, key: &str) -> Result<Option<NetworkMeta>, DbError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["meta_value"])
                .where_eq("meta_key", key)
                .where_eq("site_id", network_id)
                .build(),
        )?;
        Ok(rows
            .first()
            .map(|r| SitemetaMapper::to_entity(network_id, key, r)))
    }

    fn find_many(&self, network_id: i64, keys: &[String]) -> Result<Vec<NetworkMeta>, DbError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        // wp_prime_network_option_caches(): prepare( "... IN (%s,%s) AND site_id = %d", ... ), as WordPress writes
        // the `IN` list (the builder would write `'a', 'b'`), for the `query` filter sees the text.
        let template = format!(
            "SELECT meta_key, meta_value FROM {} WHERE meta_key IN ({}) AND site_id = %d",
            self.table(),
            vec!["%s"; keys.len()].join(",")
        );
        let mut args: Vec<WpValue> = keys.iter().map(|k| WpValue::Str(k.clone())).collect();
        args.push(WpValue::Int(network_id));
        let Some(sql) = self.wpdb.prepare(Some(template.as_bytes()), &args) else {
            return Ok(Vec::new());
        };
        let rows = match self.wpdb.query_bytes(&sql) {
            Ok(QueryOutcome::Rows(rows)) => rows,
            Ok(_) => Rc::new(Vec::new()),
            Err(e) => hidden(e).map(|()| Rc::new(Vec::new()))?,
        };
        Ok(rows
            .iter()
            .filter_map(|r| {
                let key = String::from_utf8_lossy(&field(r, "meta_key")?).into_owned();
                Some(SitemetaMapper::to_entity(network_id, &key, r))
            })
            .collect())
    }

    fn exists(&self, network_id: i64, key: &str) -> Result<bool, DbError> {
        let rows = self.rows(
            Query::table(&self.table())
                .select(&["meta_id"])
                .where_eq("meta_key", key)
                .where_eq("site_id", network_id)
                .build(),
        )?;
        Ok(rows
            .first()
            .and_then(|r| field(r, "meta_id"))
            .is_some_and(|id| String::from_utf8_lossy(&id).parse::<u64>().unwrap_or(0) != 0))
    }

    fn add(&self, meta: NetworkMeta) -> Result<bool, DbError> {
        let fields = SitemetaMapper::to_fields(&meta);
        let formats = formats(3);
        Ok(self.written(self.wpdb.insert(&self.table(), &fields, Some(&formats)))? > 0)
    }

    fn update(&self, meta: NetworkMeta) -> Result<u64, DbError> {
        let set = vec![("meta_value".to_string(), meta.value.to_value())];
        let condition = Self::condition(meta.network_id, &meta.key);
        let (set_format, where_format) = (
            vec!["%s".to_string()],
            vec!["%s".to_string(), "%d".to_string()],
        );
        self.written(self.wpdb.update(
            &self.table(),
            &set,
            &condition,
            Some(&set_format),
            Some(&where_format),
        ))
    }

    fn remove(&self, network_id: i64, key: &str) -> Result<u64, DbError> {
        let condition = Self::condition(network_id, key);
        let where_format = vec!["%s".to_string(), "%d".to_string()];
        self.written(
            self.wpdb
                .delete(&self.table(), &condition, Some(&where_format)),
        )
    }
}
