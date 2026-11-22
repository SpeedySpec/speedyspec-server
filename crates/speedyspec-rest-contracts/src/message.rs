//! The REST request and response, as hooks and handlers see them.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_operation_contracts::Value;

use crate::RestMethod;

/// A REST request (WordPress's `WP_REST_Request`): parameters by source, headers, body, and what it matched.
#[derive(Clone, Debug, PartialEq)]
pub struct RestRequest {
    pub method: RestMethod,
    /// A `HEAD` request, served as `GET` without a body.
    pub head: bool,
    /// The path after `/wp-json`, such as `/acme/v1/reports/3`.
    pub path: String,
    pub url_params: IndexMap<String, Value>,
    pub query_params: IndexMap<String, Value>,
    pub body_params: IndexMap<String, Value>,
    /// A JSON object body. Its presence decides where `set_param` writes.
    pub json_params: IndexMap<String, Value>,
    /// Lowercase names, `-` written as `_`, as WordPress stores them.
    pub headers: IndexMap<String, Vec<String>>,
    pub body: Vec<u8>,
    /// The matched route pattern.
    pub route: Option<String>,
    /// The matched operation.
    pub operation: Option<String>,
    /// Whether the body was JSON (an object or not).
    pub json_body: bool,
}

/// A request shared between the server and the hooks, which may change it.
pub type SharedRequest = Rc<RefCell<RestRequest>>;

impl RestRequest {
    pub fn new(method: RestMethod, path: &str) -> RestRequest {
        RestRequest {
            method,
            head: false,
            path: path.to_owned(),
            url_params: IndexMap::new(),
            query_params: IndexMap::new(),
            body_params: IndexMap::new(),
            json_params: IndexMap::new(),
            headers: IndexMap::new(),
            body: Vec::new(),
            route: None,
            operation: None,
            json_body: false,
        }
    }

    /// Every parameter, later sources winning: query, body, JSON, URL (`WP_REST_Request::get_parameter_order`).
    pub fn params(&self) -> IndexMap<String, Value> {
        let mut merged = IndexMap::new();
        for source in [
            &self.query_params,
            &self.body_params,
            &self.json_params,
            &self.url_params,
        ] {
            for (k, v) in source {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    }

    /// Writes `key` where it wins (`WP_REST_Request::set_param`): URL parameters when it is one; otherwise the JSON
    /// body when the request has one; otherwise the form body for methods with a body; otherwise the query.
    pub fn set_param(&mut self, key: &str, value: Value) {
        let target = if self.url_params.contains_key(key) {
            &mut self.url_params
        } else if self.json_body || !self.json_params.is_empty() {
            &mut self.json_params
        } else if self.method != RestMethod::Get {
            &mut self.body_params
        } else {
            &mut self.query_params
        };
        target.insert(key.to_owned(), value);
    }

    /// The first value of header `name` (any case, `-` or `_`).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&header_key(name))
            .and_then(|values| values.first())
            .map(String::as_str)
    }
}

/// A header name as WordPress stores it: lowercase, `-` written as `_`.
pub fn header_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// A link of a response (`WP_REST_Response::add_link`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestLink {
    pub rel: String,
    pub href: String,
    pub embeddable: bool,
}

/// A REST response: data, status, headers and links.
#[derive(Clone, Debug, PartialEq)]
pub struct RestResponse {
    pub data: Value,
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub links: Vec<RestLink>,
}

impl RestResponse {
    /// `data` with status 200.
    pub fn ok(data: Value) -> RestResponse {
        RestResponse {
            data,
            status: 200,
            headers: IndexMap::new(),
            links: Vec::new(),
        }
    }
}
