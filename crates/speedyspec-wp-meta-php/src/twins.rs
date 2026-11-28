//! The PHP twins of the meta repositories (Abstract Factory and Repository per meta type), which PHP implements to
//! replace the SQL tables, and the adapter that makes a PHP implementation the Rust port.
//!
//! `#[php_port]` declares each interface at the start of every request. A PHP object implementing
//! `SpeedySpec\WpMeta\MetaRepositories` and bound at that key in the container reaches the use cases as
//! [`PhpMetaRepositories`]. Values cross as PHP holds them: keys and stored values as strings (`null` for SQL
//! `NULL`), ids as ints, a row as `array( 'meta_id', 'object_id', 'meta_key', 'meta_value' )`, and a row of
//! `for_objects()` (`update_meta_cache()`'s statement, which does not select the id) as `array( 'object_id',
//! 'meta_key', 'meta_value' )`.

use std::rc::Rc;

use speedyspec_php_ext::{Key, PhpObject, PortError, Value, php_port};
use speedyspec_wp_meta_contracts::{
    Meta, MetaError, MetaId, MetaKey, MetaRepositories, MetaRepository, MetaRow, MetaType,
    ObjectId, SerializedValue,
};

use crate::adapters::{int_list, park};
use crate::codec::{Thrown, string};

/// `_get_meta_table()`'s twin: `for_type()` returns an object implementing `SpeedySpec\WpMeta\MetaRepository`, or
/// `null` when the type has no table.
#[php_port(name = "SpeedySpec\\WpMeta\\MetaRepositories")]
pub trait MetaRepositoriesTwin {
    fn for_type(&self, meta_type: Value) -> Result<Value, PortError>;
}

/// One meta table's twin: the methods of the `MetaRepository` port, one per `$wpdb` call `meta.php` makes.
#[php_port(name = "SpeedySpec\\WpMeta\\MetaRepository")]
pub trait MetaRepositoryTwin {
    fn engine(&self) -> Result<Value, PortError>;
    fn count_for(&self, object_id: i64, meta_key: Value) -> Result<Value, PortError>;
    fn add(&self, object_id: i64, meta_key: Value, meta_value: Value) -> Result<Value, PortError>;
    fn ids_for(&self, object_id: i64, meta_key: Value) -> Result<Value, PortError>;
    fn update_by_key(
        &self,
        object_id: i64,
        meta_key: Value,
        meta_value: Value,
        previous: Value,
    ) -> Result<Value, PortError>;
    fn ids_matching(
        &self,
        meta_key: Value,
        object_id: Value,
        meta_value: Value,
    ) -> Result<Value, PortError>;
    fn objects_with(&self, meta_key: Value, meta_value: Value) -> Result<Value, PortError>;
    fn delete_ids(&self, meta_ids: Value) -> Result<Value, PortError>;
    fn by_mid(&self, meta_id: i64) -> Result<Value, PortError>;
    fn update_by_mid(
        &self,
        meta_id: i64,
        meta_key: Value,
        meta_value: Value,
    ) -> Result<Value, PortError>;
    fn delete_by_mid(&self, meta_id: i64) -> Result<Value, PortError>;
    fn for_objects(&self, object_ids: Value) -> Result<Value, PortError>;
    fn ids_of_object(&self, object_id: i64) -> Result<Value, PortError>;
}

/// Calls `$object->method(...$args)`; an exception is parked for the running native function to rethrow, and the
/// port answers `MetaError::Db`.
fn call(object: &PhpObject, method: &str, args: Vec<Value>) -> Result<Value, MetaError> {
    let callable = Value::Array(vec![
        (Key::Int(0), object.value().clone()),
        (Key::Int(1), Value::Str(method.to_owned())),
    ]);
    speedyspec_php::call_catching(&callable, &args).map_err(|failure| {
        let message = failure.error.to_string();
        park(Thrown(failure.exception));
        MetaError::Db(message)
    })
}

/// A PHP implementation of `SpeedySpec\WpMeta\MetaRepositories`.
pub struct PhpMetaRepositories(PhpObject);

impl PhpMetaRepositories {
    pub fn new(object: PhpObject) -> PhpMetaRepositories {
        PhpMetaRepositories(object)
    }
}

impl MetaRepositories for PhpMetaRepositories {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        let answer = call(
            &self.0,
            "for_type",
            vec![Value::Str(meta_type.name().to_owned())],
        )
        .ok()?;
        let object = PhpObject::new(answer)?;
        Some(Rc::new(PhpMetaRepository {
            object,
            meta_type: meta_type.clone(),
        }))
    }
}

/// A PHP implementation of `SpeedySpec\WpMeta\MetaRepository`.
pub struct PhpMetaRepository {
    object: PhpObject,
    meta_type: MetaType,
}

fn key(key: &MetaKey) -> Value {
    string(key.as_bytes())
}

fn stored(value: Option<&SerializedValue>) -> Value {
    value.map_or(Value::Null, |v| string(v.as_bytes()))
}

fn id(id: u64) -> i64 {
    i64::try_from(id).unwrap_or(i64::MAX)
}

/// A PHP int (or numeric string) as a count or an id; anything else is 0.
fn number(value: &Value) -> u64 {
    match &*value.read() {
        Value::Int(i) => u64::try_from(*i).unwrap_or(0),
        Value::Str(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(true) => 1,
        _ => 0,
    }
}

fn numbers(value: &Value) -> Vec<u64> {
    match &*value.read() {
        Value::Array(items) => items.iter().map(|(_, v)| number(v)).collect(),
        _ => Vec::new(),
    }
}

fn field<'a>(items: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    items
        .iter()
        .find(|(k, _)| matches!(k, Key::Str(s) if s == name))
        .map(|(_, v)| v)
}

fn bytes(value: &Value) -> Option<Vec<u8>> {
    match &*value.read() {
        Value::Null => None,
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        Value::Int(i) => Some(i.to_string().into_bytes()),
        _ => Some(Vec::new()),
    }
}

/// A row array as the entity (Data Mapper).
fn row(value: &Value) -> Option<Meta> {
    let read = value.read();
    let Value::Array(items) = &*read else {
        return None;
    };
    Some(Meta {
        mid: MetaId(field(items, "meta_id").map_or(0, number)),
        object_id: ObjectId(field(items, "object_id").map_or(0, number)),
        key: MetaKey::new(field(items, "meta_key").and_then(bytes).unwrap_or_default()),
        value: field(items, "meta_value")
            .and_then(bytes)
            .map(SerializedValue::new),
    })
}

/// A `for_objects()` row array as the row value (Data Mapper): no `meta_id`.
fn object_row(value: &Value) -> Option<MetaRow> {
    let read = value.read();
    let Value::Array(items) = &*read else {
        return None;
    };
    Some(MetaRow {
        object_id: ObjectId(field(items, "object_id").map_or(0, number)),
        key: MetaKey::new(field(items, "meta_key").and_then(bytes).unwrap_or_default()),
        value: field(items, "meta_value")
            .and_then(bytes)
            .map(SerializedValue::new),
    })
}

impl PhpMetaRepository {
    fn call(&self, method: &str, args: Vec<Value>) -> Result<Value, MetaError> {
        call(&self.object, method, args)
    }
}

impl MetaRepository for PhpMetaRepository {
    fn engine(&self) -> String {
        match self.call("engine", Vec::new()) {
            Ok(Value::Str(s)) => s,
            _ => "php".to_owned(),
        }
    }

    fn meta_type(&self) -> &MetaType {
        &self.meta_type
    }

    fn count_for(&self, object: ObjectId, meta_key: &MetaKey) -> Result<u64, MetaError> {
        Ok(number(&self.call(
            "count_for",
            vec![Value::Int(id(object.0)), key(meta_key)],
        )?))
    }

    fn add(
        &self,
        object: ObjectId,
        meta_key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Option<MetaId>, MetaError> {
        let mid = number(&self.call(
            "add",
            vec![Value::Int(id(object.0)), key(meta_key), stored(value)],
        )?);
        Ok((mid > 0).then_some(MetaId(mid)))
    }

    fn ids_for(&self, object: ObjectId, meta_key: &MetaKey) -> Result<Vec<MetaId>, MetaError> {
        let ids = self.call("ids_for", vec![Value::Int(id(object.0)), key(meta_key)])?;
        Ok(numbers(&ids).into_iter().map(MetaId).collect())
    }

    fn update_by_key(
        &self,
        object: ObjectId,
        meta_key: &MetaKey,
        value: Option<&SerializedValue>,
        previous: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        Ok(number(&self.call(
            "update_by_key",
            vec![
                Value::Int(id(object.0)),
                key(meta_key),
                stored(value),
                stored(previous),
            ],
        )?))
    }

    fn ids_matching(
        &self,
        meta_key: &MetaKey,
        object: Option<ObjectId>,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<MetaId>, MetaError> {
        let object = object.map_or(Value::Null, |o| Value::Int(id(o.0)));
        let ids = self.call("ids_matching", vec![key(meta_key), object, stored(value)])?;
        Ok(numbers(&ids).into_iter().map(MetaId).collect())
    }

    fn objects_with(
        &self,
        meta_key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<ObjectId>, MetaError> {
        let ids = self.call("objects_with", vec![key(meta_key), stored(value)])?;
        Ok(numbers(&ids).into_iter().map(ObjectId).collect())
    }

    fn delete_ids(&self, ids: &[MetaId]) -> Result<u64, MetaError> {
        Ok(number(&self.call(
            "delete_ids",
            vec![int_list(ids.iter().map(|m| m.0))],
        )?))
    }

    fn by_mid(&self, mid: MetaId) -> Result<Option<Meta>, MetaError> {
        Ok(row(&self.call("by_mid", vec![Value::Int(id(mid.0))])?))
    }

    fn update_by_mid(
        &self,
        mid: MetaId,
        meta_key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        Ok(number(&self.call(
            "update_by_mid",
            vec![Value::Int(id(mid.0)), key(meta_key), stored(value)],
        )?))
    }

    fn delete_by_mid(&self, mid: MetaId) -> Result<u64, MetaError> {
        Ok(number(
            &self.call("delete_by_mid", vec![Value::Int(id(mid.0))])?,
        ))
    }

    fn for_objects(&self, objects: &[ObjectId]) -> Result<Vec<MetaRow>, MetaError> {
        let rows = self.call("for_objects", vec![int_list(objects.iter().map(|o| o.0))])?;
        Ok(match &*rows.read() {
            Value::Array(items) => items.iter().filter_map(|(_, v)| object_row(v)).collect(),
            _ => Vec::new(),
        })
    }

    fn ids_of_object(&self, object: ObjectId) -> Result<Vec<MetaId>, MetaError> {
        let ids = self.call("ids_of_object", vec![Value::Int(id(object.0))])?;
        Ok(numbers(&ids).into_iter().map(MetaId).collect())
    }
}
