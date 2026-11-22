//! The Rust requests and responses PHP proxies stand for, by handle, for one request.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_rest_contracts::{RestResponse, SharedRequest};

/// A response shared between Rust and its PHP proxy.
pub type SharedResponse = Rc<RefCell<RestResponse>>;

thread_local! {
    static NEXT: Cell<u64> = const { Cell::new(1) };
    static REQUESTS: RefCell<HashMap<u64, SharedRequest>> = RefCell::new(HashMap::new());
    static RESPONSES: RefCell<HashMap<u64, SharedResponse>> = RefCell::new(HashMap::new());
}

fn next() -> u64 {
    NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

pub fn add_request(request: SharedRequest) -> u64 {
    let id = next();
    REQUESTS.with(|r| r.borrow_mut().insert(id, request));
    id
}

pub fn request(handle: u64) -> Option<SharedRequest> {
    REQUESTS.with(|r| r.borrow().get(&handle).cloned())
}

pub fn add_response(response: SharedResponse) -> u64 {
    let id = next();
    RESPONSES.with(|r| r.borrow_mut().insert(id, response));
    id
}

pub fn response(handle: u64) -> Option<SharedResponse> {
    RESPONSES.with(|r| r.borrow().get(&handle).cloned())
}

/// Forgets every handle (the request ended): a proxy used after this throws.
pub fn clear() {
    REQUESTS.with(|r| r.borrow_mut().clear());
    RESPONSES.with(|r| r.borrow_mut().clear());
}
