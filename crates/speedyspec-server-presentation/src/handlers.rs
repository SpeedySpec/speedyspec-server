//! What answers once a request has reached an app.

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};
use speedyspec_server_static::StaticSet;

/// How long clients are told to wait before retrying an app that cannot serve yet.
pub const RETRY_AFTER_SECONDS: &str = "30";

/// The live handler of one app.
#[derive(Clone, Debug)]
pub enum AppHandle {
    /// A static release, served from memory.
    Static(Arc<StaticSet>),
    /// An app with no release installed yet.
    NotInstalled,
    /// A PHP app, until the worker pool exists.
    PhpPlaceholder,
    /// The admin API, until it has endpoints.
    AdminApiPlaceholder,
}

impl AppHandle {
    pub fn respond(
        &self,
        method: &Method,
        mount_prefix: &str,
        rel_path: &str,
        query: Option<&str>,
        headers: &HeaderMap,
    ) -> Response<Bytes> {
        match self {
            AppHandle::Static(set) => set.respond(method, mount_prefix, rel_path, query, headers),
            AppHandle::NotInstalled | AppHandle::PhpPlaceholder => {
                let mut response = text(StatusCode::SERVICE_UNAVAILABLE, "service unavailable");
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    HeaderValue::from_static(RETRY_AFTER_SECONDS),
                );
                response
            }
            AppHandle::AdminApiPlaceholder => text(StatusCode::NOT_FOUND, "not found"),
        }
    }
}

/// A plain-text response.
pub fn text(status: StatusCode, body: &'static str) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from_static(body.as_bytes()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
