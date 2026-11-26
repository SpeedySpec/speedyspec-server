//! `SqlMetaRepository` (Repository per meta type) and `SqlMetaRepositories` (Abstract Factory, `_get_meta_table()`):
//! the meta tables through the `wpdb` port, one `$wpdb` call per call `meta.php` makes.
//!
//! Reads go through `wpdb::prepare` and `wpdb::query` with `meta.php`'s own templates, so the `query` filter sees
//! WordPress's text; the id lists `meta.php` joins without `prepare` (`update_meta_cache()`, `delete_metadata()`) are
//! joined here the same way. Writes go through `$wpdb->insert`, `update` and `delete`, so charset stripping and the
//! changed-rows count are `wpdb`'s. Rows become entities through [`MetaMapper`] (Data Mapper). The table comes from
//! `wpdb` on every call, so `switch_to_blog` is followed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_db_contracts::{QueryOutcome, WpDbError, WpRow, WpValue, Wpdb};
use speedyspec_wp_meta_contracts::{
    Meta, MetaError, MetaId, MetaKey, MetaRepositories, MetaRepository, MetaType, ObjectId,
    SerializedValue,
};

use crate::MetaMapper;

/// One meta table over `wpdb`.
pub struct SqlMetaRepository {
    wpdb: Rc<dyn Wpdb>,
    meta_type: MetaType,
    mapper: MetaMapper,
    engine: String,
}

/// A hook's failure (a PHP exception from a `query` callback) is the one failure `$wpdb` does not hide. Its payload
/// stays in `wpdb`'s side channel (`take_hook_error()`) for the PHP bridge to rethrow.
fn surfaced(e: WpDbError) -> Result<(), MetaError> {
    match e {
        WpDbError::Hook(e) => Err(MetaError::Db(e.message)),
        _ => Ok(()),
    }
}

/// `%d` as PHP's `(int)` gives it: an id past `PHP_INT_MAX` is `PHP_INT_MAX`.
fn int(id: u64) -> WpValue {
    WpValue::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

/// An id or count column's bytes as a number; anything else is 0, as `(int)` reads it.
fn number(bytes: Option<&Option<Vec<u8>>>) -> u64 {
    bytes
        .and_then(Option::as_ref)
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn first_column(row: &WpRow) -> Option<&Option<Vec<u8>>> {
    row.fields.first().map(|(_, v)| v)
}

/// `wpdb`'s row (text bytes or `NULL`) as the mapper's row.
fn to_row(row: &WpRow) -> Row {
    let columns: Rc<[String]> = row.fields.iter().map(|(n, _)| n.clone()).collect();
    let values = row
        .fields
        .iter()
        .map(|(_, v)| v.clone().map_or(DbValue::Null, DbValue::Bytes))
        .collect();
    Row::new(columns, values)
}

/// `implode( ',', $ids )`, the ids as integers.
fn id_list(ids: impl Iterator<Item = u64>) -> String {
    ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

impl SqlMetaRepository {
    /// The repository for `meta_type`'s table. `engine` names the engine behind `wpdb`, as `engine()` reports it.
    pub fn new(wpdb: Rc<dyn Wpdb>, meta_type: MetaType, engine: &str) -> SqlMetaRepository {
        SqlMetaRepository {
            mapper: MetaMapper::new(meta_type.clone()),
            wpdb,
            meta_type,
            engine: engine.to_string(),
        }
    }

    /// `$wpdb->{type}meta`, read now.
    fn table(&self) -> String {
        self.wpdb.table_name(&self.meta_type.table_suffix())
    }

    fn object_column(&self) -> String {
        self.meta_type.object_column()
    }

    fn id_column(&self) -> &'static str {
        self.meta_type.id_column()
    }

    /// `$wpdb->get_results( $wpdb->prepare( $template, ...$args ) )`; no rows where `wpdb` would answer null.
    fn select(&self, template: &str, args: &[WpValue]) -> Result<Rc<Vec<WpRow>>, MetaError> {
        let Some(sql) = self.wpdb.prepare(Some(template.as_bytes()), args) else {
            return Ok(Rc::new(Vec::new()));
        };
        self.select_text(&sql)
    }

    /// `$wpdb->get_results( $sql )` for text built without `prepare`.
    fn select_text(&self, sql: &[u8]) -> Result<Rc<Vec<WpRow>>, MetaError> {
        match self.wpdb.query_bytes(sql) {
            Ok(QueryOutcome::Rows(rows)) => Ok(rows),
            Ok(_) => Ok(Rc::new(Vec::new())),
            Err(e) => surfaced(e).map(|()| Rc::new(Vec::new())),
        }
    }

    /// `$wpdb->get_col()`: the first column of every row, as ids.
    fn ids(&self, template: &str, args: &[WpValue]) -> Result<Vec<u64>, MetaError> {
        Ok(self
            .select(template, args)?
            .iter()
            .map(|row| number(first_column(row)))
            .collect())
    }

    /// A write helper's count; 0 where `wpdb` returns false.
    fn written(&self, result: Result<u64, WpDbError>) -> Result<u64, MetaError> {
        match result {
            Ok(n) => Ok(n),
            Err(e) => surfaced(e).map(|()| 0),
        }
    }

    fn key(key: &MetaKey) -> WpValue {
        MetaMapper::php_string(key.as_bytes())
    }

    fn value(value: Option<&SerializedValue>) -> WpValue {
        value.map_or(WpValue::Null, |v| MetaMapper::php_string(v.as_bytes()))
    }
}

impl MetaRepository for SqlMetaRepository {
    fn engine(&self) -> String {
        self.engine.clone()
    }

    fn meta_type(&self) -> &MetaType {
        &self.meta_type
    }

    fn count_for(&self, object: ObjectId, key: &MetaKey) -> Result<u64, MetaError> {
        let template = format!(
            "SELECT COUNT(*) FROM {} WHERE meta_key = %s AND {} = %d",
            self.table(),
            self.object_column()
        );
        let rows = self.select(&template, &[Self::key(key), int(object.0)])?;
        Ok(rows.first().map_or(0, |row| number(first_column(row))))
    }

    /// `$wpdb->insert( $table, array( $column => $object_id, 'meta_key' => .., 'meta_value' => .. ) )` with no
    /// formats, as `meta.php` sends it (so `$wpdb->field_types` decides: `post_id` is `%d`, `comment_id` and a custom
    /// `{type}_id` go in as strings), then `$wpdb->insert_id`.
    ///
    /// A `query` callback's failure answers `Ok(None)`: `$wpdb->insert`'s charset lookup hides it, as WordPress's
    /// does. The payload stays in `take_hook_error()`, which callers must check to rethrow it.
    fn add(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Option<MetaId>, MetaError> {
        let fields = self.mapper.insert_fields(object, key, value);
        let inserted = self.written(self.wpdb.insert(&self.table(), &fields, None))?;
        if inserted == 0 {
            return Ok(None);
        }
        Ok(Some(MetaId(self.wpdb.insert_id())))
    }

    /// WordPress sends no `ORDER BY`; the ids are sorted here, which is the order MySQL gives.
    fn ids_for(&self, object: ObjectId, key: &MetaKey) -> Result<Vec<MetaId>, MetaError> {
        let template = format!(
            "SELECT {} FROM {} WHERE meta_key = %s AND {} = %d",
            self.id_column(),
            self.table(),
            self.object_column()
        );
        let mut ids = self.ids(&template, &[Self::key(key), int(object.0)])?;
        ids.sort_unstable();
        Ok(ids.into_iter().map(MetaId).collect())
    }

    /// `$wpdb->update( $table, compact( 'meta_value' ), $where )`, no formats. The count is the changed rows on
    /// every engine: MySQL reports them, and the SQLite and PostgreSQL engines add the inequality themselves.
    fn update_by_key(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
        previous: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        let data = vec![("meta_value".to_string(), Self::value(value))];
        let mut conditions = vec![
            (self.object_column(), int(object.0)),
            ("meta_key".to_string(), Self::key(key)),
        ];
        if let Some(previous) = previous {
            conditions.push(("meta_value".to_string(), Self::value(Some(previous))));
        }
        self.written(
            self.wpdb
                .update(&self.table(), &data, &conditions, None, None),
        )
    }

    /// One `prepare` of the whole template: the same text as `meta.php`'s three concatenated `prepare` calls.
    fn ids_matching(
        &self,
        key: &MetaKey,
        object: Option<ObjectId>,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<MetaId>, MetaError> {
        let mut template = format!(
            "SELECT {} FROM {} WHERE meta_key = %s",
            self.id_column(),
            self.table()
        );
        let mut args = vec![Self::key(key)];
        if let Some(object) = object {
            template.push_str(&format!(" AND {} = %d", self.object_column()));
            args.push(int(object.0));
        }
        if let Some(value) = value {
            template.push_str(" AND meta_value = %s");
            args.push(Self::value(Some(value)));
        }
        Ok(self
            .ids(&template, &args)?
            .into_iter()
            .map(MetaId)
            .collect())
    }

    fn objects_with(
        &self,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<ObjectId>, MetaError> {
        let mut template = format!(
            "SELECT {} FROM {} WHERE meta_key = %s",
            self.object_column(),
            self.table()
        );
        let mut args = vec![Self::key(key)];
        if let Some(value) = value {
            template.push_str(" AND meta_value = %s");
            args.push(Self::value(Some(value)));
        }
        Ok(self
            .ids(&template, &args)?
            .into_iter()
            .map(ObjectId)
            .collect())
    }

    /// `"DELETE FROM $table WHERE $id_column IN( " . implode( ',', $meta_ids ) . ' )'`, sent by `$wpdb->query`.
    fn delete_ids(&self, ids: &[MetaId]) -> Result<u64, MetaError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let sql = format!(
            "DELETE FROM {} WHERE {} IN( {} )",
            self.table(),
            self.id_column(),
            id_list(ids.iter().map(|id| id.0))
        );
        match self.wpdb.query_bytes(sql.as_bytes()) {
            Ok(QueryOutcome::Affected { rows, .. }) => Ok(rows),
            Ok(_) => Ok(0),
            Err(e) => surfaced(e).map(|()| 0),
        }
    }

    fn by_mid(&self, mid: MetaId) -> Result<Option<Meta>, MetaError> {
        let template = format!(
            "SELECT * FROM {} WHERE {} = %d",
            self.table(),
            self.id_column()
        );
        let rows = self.select(&template, &[int(mid.0)])?;
        rows.first()
            .map(|row| self.mapper.to_entity(&to_row(row)))
            .transpose()
    }

    /// `$wpdb->update( $table, array( 'meta_key' => .., 'meta_value' => .. ), array( $id_column => .. ), '%s', '%d' )`.
    fn update_by_mid(
        &self,
        mid: MetaId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        let data = vec![
            ("meta_key".to_string(), Self::key(key)),
            ("meta_value".to_string(), Self::value(value)),
        ];
        let conditions = vec![(self.id_column().to_string(), int(mid.0))];
        let (format, where_format) = (vec!["%s".to_string()], vec!["%d".to_string()]);
        self.written(self.wpdb.update(
            &self.table(),
            &data,
            &conditions,
            Some(&format),
            Some(&where_format),
        ))
    }

    /// `$wpdb->delete( $table, array( $id_column => $meta_id ) )`, no format.
    fn delete_by_mid(&self, mid: MetaId) -> Result<u64, MetaError> {
        let conditions = vec![(self.id_column().to_string(), int(mid.0))];
        self.written(self.wpdb.delete(&self.table(), &conditions, None))
    }

    /// `update_meta_cache()`'s query with one known difference: the id column is added, because [`Meta`] carries
    /// `mid`. The `query` filter sees `SELECT meta_id, post_id, meta_key, meta_value FROM wp_postmeta WHERE post_id
    /// IN (..) ORDER BY meta_id ASC` where WordPress sends `SELECT post_id, meta_key, meta_value ...`.
    fn for_objects(&self, objects: &[ObjectId]) -> Result<Vec<Meta>, MetaError> {
        if objects.is_empty() {
            return Ok(Vec::new());
        }
        let column = self.object_column();
        let sql = format!(
            "SELECT {id}, {column}, meta_key, meta_value FROM {table} WHERE {column} IN ({list}) ORDER BY {id} ASC",
            id = self.id_column(),
            table = self.table(),
            list = id_list(objects.iter().map(|o| o.0)),
        );
        self.select_text(sql.as_bytes())?
            .iter()
            .map(|row| self.mapper.to_entity(&to_row(row)))
            .collect()
    }
}

/// `_get_meta_table()` over `wpdb`: a type has a table while `$wpdb->{type}meta` is set (a non-empty
/// `Wpdb::table_name`). Each type's repository is made once and shared; the table is still read on every call.
pub struct SqlMetaRepositories {
    wpdb: Rc<dyn Wpdb>,
    engine: String,
    made: RefCell<HashMap<MetaType, Rc<dyn MetaRepository>>>,
}

impl SqlMetaRepositories {
    pub fn new(wpdb: Rc<dyn Wpdb>, engine: &str) -> SqlMetaRepositories {
        SqlMetaRepositories {
            wpdb,
            engine: engine.to_string(),
            made: RefCell::new(HashMap::new()),
        }
    }
}

impl MetaRepositories for SqlMetaRepositories {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        if self.wpdb.table_name(&meta_type.table_suffix()).is_empty() {
            return None;
        }
        let mut made = self.made.borrow_mut();
        let repo = made.entry(meta_type.clone()).or_insert_with(|| {
            Rc::new(SqlMetaRepository::new(
                self.wpdb.clone(),
                meta_type.clone(),
                &self.engine,
            ))
        });
        Some(repo.clone())
    }
}
