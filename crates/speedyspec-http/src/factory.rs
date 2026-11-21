use crate::{Body, HttpError, Method, StatusCode};

/// Creates requests (PSR-17's `RequestFactoryInterface`).
pub trait RequestFactory<R> {
    fn create_request(&self, method: Method, uri: &str) -> Result<R, HttpError>;
}

/// Creates server requests (PSR-17's `ServerRequestFactoryInterface`).
pub trait ServerRequestFactory<R> {
    fn create_server_request(
        &self,
        method: Method,
        uri: &str,
        server_params: Vec<(String, String)>,
    ) -> Result<R, HttpError>;
}

/// Creates responses (PSR-17's `ResponseFactoryInterface`).
pub trait ResponseFactory<R> {
    fn create_response(&self, status: StatusCode, reason: &str) -> R;
}

/// Creates bodies (PSR-17's `StreamFactoryInterface`).
pub trait StreamFactory {
    fn create_stream(&self, content: &[u8]) -> Box<dyn Body>;
    fn create_stream_from_file(&self, path: &std::path::Path) -> Result<Box<dyn Body>, HttpError>;
    fn create_stream_from_reader(
        &self,
        reader: Box<dyn std::io::Read>,
        limit: u64,
    ) -> Box<dyn Body>;
}

/// Creates uploaded files (PSR-17's `UploadedFileFactoryInterface`).
pub trait UploadedFileFactory<F> {
    fn create_uploaded_file(
        &self,
        stream: Box<dyn Body>,
        size: Option<u64>,
        error: i32,
        client_filename: Option<&str>,
        client_media_type: Option<&str>,
    ) -> F;
}

/// Creates URIs (PSR-17's `UriFactoryInterface`).
pub trait UriFactory<U> {
    fn create_uri(&self, uri: &str) -> Result<U, HttpError>;
}
