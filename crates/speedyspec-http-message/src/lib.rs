//! HTTP messages implementing the `speedyspec-http` contracts.
//!
//! Requests, server requests, responses, URIs, bodies and uploaded files, and [`MessageFactory`], one family of
//! PSR-17 factories for them (Abstract Factory), so tests and later ports can supply their own family instead.

mod body;
mod factory;
mod percent;
mod request;
mod response;
mod server_request;
mod uploaded_file;
mod uri;

pub use body::{BytesBody, LazyBody, Replay, SentBody};
pub use factory::MessageFactory;
pub use request::HttpRequest;
pub use response::{HttpResponse, SentRecord};
pub use server_request::HttpServerRequest;
pub use uploaded_file::HttpUploadedFile;
pub use uri::HttpUri;
