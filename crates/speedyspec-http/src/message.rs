use crate::{Attributes, Headers, HttpError, Method, StatusCode};

/// A message body (PSR-7's `StreamInterface`), read once into memory when asked.
pub trait Body {
    /// The size in bytes, when known without reading.
    fn size(&self) -> Option<u64>;
    /// Reads the whole body. Reading again gives the same bytes.
    fn read_all(&mut self) -> Result<Vec<u8>, HttpError>;
    /// True when the body was already written to the client, as PHP's output is.
    fn is_sent(&self) -> bool;
}

/// A URI (RFC 3986) with PSR-7's components and normalization.
pub trait Uri {
    fn scheme(&self) -> &str;
    fn authority(&self) -> String;
    fn user_info(&self) -> &str;
    fn host(&self) -> &str;
    /// The port, unless it is the scheme's default.
    fn port(&self) -> Option<u16>;
    fn path(&self) -> &str;
    fn query(&self) -> &str;
    fn fragment(&self) -> &str;
    fn to_uri_string(&self) -> String;
    fn with_scheme(self, scheme: &str) -> Result<Self, HttpError>
    where
        Self: Sized;
    fn with_user_info(self, user: &str, password: Option<&str>) -> Self
    where
        Self: Sized;
    fn with_host(self, host: &str) -> Self
    where
        Self: Sized;
    fn with_port(self, port: Option<u16>) -> Self
    where
        Self: Sized;
    fn with_path(self, path: &str) -> Self
    where
        Self: Sized;
    fn with_query(self, query: &str) -> Self
    where
        Self: Sized;
    fn with_fragment(self, fragment: &str) -> Self
    where
        Self: Sized;
}

/// What requests and responses share (PSR-7's `MessageInterface`).
pub trait Message: Sized {
    fn protocol_version(&self) -> &str;
    fn headers(&self) -> &Headers;
    fn body_mut(&mut self) -> &mut dyn Body;
    fn with_protocol_version(self, version: &str) -> Self;
    fn with_header(self, name: &str, values: Vec<String>) -> Result<Self, HttpError>;
    fn with_added_header(self, name: &str, values: Vec<String>) -> Result<Self, HttpError>;
    fn without_header(self, name: &str) -> Self;
    fn with_body(self, body: Box<dyn Body>) -> Self;
}

/// An outgoing or incoming request (PSR-7's `RequestInterface`).
pub trait Request: Message {
    type Uri: Uri;
    fn method(&self) -> &Method;
    /// The request target as sent, else the URI's path and query.
    fn request_target(&self) -> String;
    fn uri(&self) -> &Self::Uri;
    fn with_method(self, method: Method) -> Self;
    fn with_request_target(self, target: &str) -> Self;
    fn with_uri(self, uri: Self::Uri) -> Self;
}

/// A request as the server received it (PSR-7's `ServerRequestInterface`).
pub trait ServerRequest: Request {
    type File: UploadedFile;
    /// The server's parameters (the FastCGI params), in the order they arrived.
    fn server_params(&self) -> &[(String, String)];
    fn cookie_params(&self) -> &[(String, String)];
    fn query_params(&self) -> &[(String, String)];
    fn parsed_body(&self) -> Option<&[(String, String)]>;
    fn uploaded_files(&self) -> &[(String, Self::File)];
    fn attributes(&self) -> &Attributes;
    fn with_attribute<T: 'static>(self, value: T) -> Self;
    fn with_query_params(self, params: Vec<(String, String)>) -> Self;
    fn with_parsed_body(self, body: Option<Vec<(String, String)>>) -> Self;
    fn with_cookie_params(self, cookies: Vec<(String, String)>) -> Self;
}

/// A response (PSR-7's `ResponseInterface`).
pub trait Response: Message {
    fn status(&self) -> StatusCode;
    fn reason_phrase(&self) -> &str;
    /// Sets the status; an empty `reason` uses the standard phrase.
    fn with_status(self, status: StatusCode, reason: &str) -> Self;
}

/// A file uploaded with a request (PSR-7's `UploadedFileInterface`).
pub trait UploadedFile {
    fn size(&self) -> Option<u64>;
    /// PHP's `UPLOAD_ERR_*` code; 0 is success.
    fn error(&self) -> i32;
    fn client_filename(&self) -> Option<&str>;
    fn client_media_type(&self) -> Option<&str>;
    fn stream(&mut self) -> Result<&mut dyn Body, HttpError>;
    fn move_to(&mut self, target: &std::path::Path) -> Result<(), HttpError>;
}
