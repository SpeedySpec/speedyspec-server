//! Contracts for HTTP messages, shaped after the PHP-FIG standards.
//!
//! The runtime needs one way to represent a request and a response, and one way to pass a request through steps
//! that may answer it. These traits mirror PSR-7 (messages, URIs, streams, server requests), PSR-17 (factories),
//! PSR-15 (handlers and middleware) and PSR-18 (clients) with Rust idioms: `with_*` methods take `self` and return
//! the changed value, so immutability costs no copy when the old value is not kept.
//!
//! This crate holds no implementation, so an implementation can be replaced partly or wholly.

mod attributes;
mod client;
mod error;
mod factory;
mod headers;
mod message;
mod method;
mod server;
mod status;

pub use attributes::Attributes;
pub use client::{Client, ClientError};
pub use error::HttpError;
pub use factory::{
    RequestFactory, ResponseFactory, ServerRequestFactory, StreamFactory, UploadedFileFactory,
    UriFactory,
};
pub use headers::Headers;
pub use message::{Body, Message, Request, Response, ServerRequest, SharedBody, UploadedFile, Uri};
pub use method::Method;
pub use server::{Handler, Middleware};
pub use status::StatusCode;
