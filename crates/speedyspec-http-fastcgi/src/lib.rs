//! FastCGI on the edge of the request pipeline (Adapter).
//!
//! The runtime receives FastCGI params and stdin; the pipeline works on server requests. This crate is the only one
//! that knows FastCGI param names: it builds a server request from them, and writes a response as CGI output.

mod decode;
mod request;
mod response;

pub use request::server_request_from_params;
pub use response::response_to_cgi;
