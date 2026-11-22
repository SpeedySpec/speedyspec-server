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
    /// Defaults from the matched route's arguments: the lowest-priority source.
    pub default_params: IndexMap<String, Value>,
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
            default_params: IndexMap::new(),
            headers: IndexMap::new(),
            body: Vec::new(),
            route: None,
            operation: None,
            json_body: false,
        }
    }

    /// Whether the request carries body parameters WordPress reads (`POST`, `PUT`, `PATCH`, `DELETE`).
    fn reads_body(&self) -> bool {
        self.method != RestMethod::Get
    }

    /// The parameter sources in priority order (`WP_REST_Request::get_parameter_order`): the JSON body when the body is
    /// JSON, the form body for methods with a body, then the query, then URL parameters, then defaults. The first source holding a
    /// key wins, so the query beats URL parameters, as in WordPress.
    fn order(&self) -> Vec<&IndexMap<String, Value>> {
        let mut order = Vec::new();
        if self.json_body || !self.json_params.is_empty() {
            order.push(&self.json_params);
        }
        if self.reads_body() {
            order.push(&self.body_params);
        }
        order.push(&self.query_params);
        order.push(&self.url_params);
        order.push(&self.default_params);
        order
    }

    /// Every parameter, each from the first source holding it (`WP_REST_Request::get_params`).
    pub fn params(&self) -> IndexMap<String, Value> {
        let mut merged = IndexMap::new();
        for source in self.order().into_iter().rev() {
            for (k, v) in source {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    }

    /// Writes `key` as `WP_REST_Request::set_param` does: every source that holds it is updated; when none does, the
    /// first source in priority order gets it (the JSON body, the form body for methods with a body, else the query).
    pub fn set_param(&mut self, key: &str, value: Value) {
        let json = self.json_body || !self.json_params.is_empty();
        let reads_body = self.reads_body();
        let mut found = false;
        let sources: [(&mut IndexMap<String, Value>, bool); 4] = [
            (&mut self.json_params, json),
            (&mut self.body_params, reads_body),
            (&mut self.query_params, true),
            (&mut self.url_params, true),
        ];
        let mut first: Option<&mut IndexMap<String, Value>> = None;
        for (source, used) in sources {
            if !used {
                continue;
            }
            if let Some(slot) = source.get_mut(key) {
                *slot = value.clone();
                found = true;
            } else if first.is_none() {
                first = Some(source);
            }
        }
        // Defaults are never written, only updated when they hold the key, as trunk's set_param does.
        if let Some(slot) = self.default_params.get_mut(key) {
            *slot = value.clone();
            found = true;
        }
        if !found && let Some(source) = first {
            source.insert(key.to_owned(), value);
        }
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
