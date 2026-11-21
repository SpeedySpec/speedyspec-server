//! The route table store, supplied from PHP.
//!
//! The router cannot be replaced from PHP; its store can. The store is a container port under
//! [`ROUTE_TABLE_STORE`]: a PHP object implementing `SpeedySpec\Routing\RouteTableStore`, bound in
//! `app.<id>.php.bindings` or with the container's `instance`/`bind`, takes every lookup through
//! [`PhpRouteTableStore`] (Adapter). Route data crosses as an opaque versioned byte string, so a PHP store (APCu, files,
//! Redis, a table) needs to know nothing of its shape.
//!
//! PHP bindings last one request, so outside a PHP request, and before PHP starts in eager mode, lookups go to the
//! worker's memory store.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php_ext::{PhpObject, PortError, Value, php_port};
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{Fingerprint, RouteData, RouteTableStore, StoreError};

/// The container key of the route table store port.
pub const ROUTE_TABLE_STORE: &str = "SpeedySpec\\Routing\\RouteTableStore";

/// The store as PHP implements it. `data` is route data's byte form; `get` answers it, or null on a miss.
#[php_port(name = "SpeedySpec\\Routing\\RouteTableStore")]
pub trait RouteTableStorage {
    fn get(&self, fingerprint: String) -> Result<Value, PortError>;
    fn put(&self, fingerprint: String, data: Value) -> Result<(), PortError>;
    fn forget(&self, fingerprint: String) -> Result<(), PortError>;
}

/// [`RouteTableStore`] over a PHP object implementing the twin interface.
pub struct PhpRouteTableStore(PhpRouteTableStorage);

impl PhpRouteTableStore {
    pub fn new(object: PhpObject) -> PhpRouteTableStore {
        PhpRouteTableStore(PhpRouteTableStorage::new(object))
    }
}

fn store_error(err: PortError) -> StoreError {
    StoreError(err.0)
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => Value::Str(text),
        Err(err) => Value::Bytes(err.into_bytes()),
    }
}

impl RouteTableStore for PhpRouteTableStore {
    /// A value that is not a string, does not decode, or carries another version is a miss.
    fn get(&self, fingerprint: &Fingerprint) -> Result<Option<RouteData>, StoreError> {
        let value = self.0.get(fingerprint.to_hex()).map_err(store_error)?;
        Ok(match value {
            Value::Str(text) => RouteData::from_bytes(text.as_bytes()),
            Value::Bytes(bytes) => RouteData::from_bytes(&bytes),
            _ => None,
        })
    }

    fn put(&self, fingerprint: &Fingerprint, data: &RouteData) -> Result<(), StoreError> {
        self.0
            .put(fingerprint.to_hex(), bytes_value(data.to_bytes()))
            .map_err(store_error)
    }

    fn forget(&self, fingerprint: &Fingerprint) -> Result<(), StoreError> {
        self.0.forget(fingerprint.to_hex()).map_err(store_error)
    }
}

/// The store bound under [`ROUTE_TABLE_STORE`] at the moment of each call, else the worker's memory store.
pub struct BoundRouteTableStore {
    container: Rc<Container>,
    fallback: Rc<MemoryRouteTableStore>,
}

impl BoundRouteTableStore {
    pub fn new(
        container: Rc<Container>,
        fallback: Rc<MemoryRouteTableStore>,
    ) -> BoundRouteTableStore {
        BoundRouteTableStore {
            container,
            fallback,
        }
    }

    fn current(&self) -> Rc<dyn RouteTableStore> {
        self.container
            .make_port::<dyn RouteTableStore>(ROUTE_TABLE_STORE)
            .unwrap_or_else(|_| self.fallback.clone())
    }
}

impl RouteTableStore for BoundRouteTableStore {
    fn get(&self, fingerprint: &Fingerprint) -> Result<Option<RouteData>, StoreError> {
        self.current().get(fingerprint)
    }

    fn put(&self, fingerprint: &Fingerprint, data: &RouteData) -> Result<(), StoreError> {
        self.current().put(fingerprint, data)
    }

    fn forget(&self, fingerprint: &Fingerprint) -> Result<(), StoreError> {
        self.current().forget(fingerprint)
    }
}

/// Registers the store's port on the container area.
///
/// It is not marked stateful: the store is only a cache, so binding another one mid-request costs a miss, never a
/// wrong answer.
pub fn register_route_table_port(area: ContainerArea) -> ContainerArea {
    area.port::<dyn RouteTableStore>(ROUTE_TABLE_STORE, |object| {
        Rc::new(PhpRouteTableStore::new(object))
    })
}
