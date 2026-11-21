use std::io::Read;

use speedyspec_http::{
    Body, HttpError, Method, RequestFactory, ResponseFactory, ServerRequestFactory, StatusCode,
    StreamFactory, UploadedFileFactory, UriFactory,
};

use crate::{
    BytesBody, HttpRequest, HttpResponse, HttpServerRequest, HttpUploadedFile, HttpUri, LazyBody,
};

/// One family of PSR-17 factories for this crate's messages.
#[derive(Debug, Clone, Copy, Default)]
pub struct MessageFactory;

impl RequestFactory<HttpRequest> for MessageFactory {
    fn create_request(&self, method: Method, uri: &str) -> Result<HttpRequest, HttpError> {
        Ok(HttpRequest::new(method, uri.parse()?))
    }
}

impl ServerRequestFactory<HttpServerRequest> for MessageFactory {
    fn create_server_request(
        &self,
        method: Method,
        uri: &str,
        server_params: Vec<(String, String)>,
    ) -> Result<HttpServerRequest, HttpError> {
        Ok(HttpServerRequest::new(
            HttpRequest::new(method, uri.parse()?),
            server_params,
        ))
    }
}

impl ResponseFactory<HttpResponse> for MessageFactory {
    fn create_response(&self, status: StatusCode, reason: &str) -> HttpResponse {
        use speedyspec_http::Response;
        HttpResponse::new(status).with_status(status, reason)
    }
}

impl StreamFactory for MessageFactory {
    fn create_stream(&self, content: &[u8]) -> Box<dyn Body> {
        Box::new(BytesBody::new(content.to_vec()))
    }

    fn create_stream_from_file(&self, path: &std::path::Path) -> Result<Box<dyn Body>, HttpError> {
        let bytes = std::fs::read(path).map_err(|err| HttpError::Io(err.to_string()))?;
        Ok(Box::new(BytesBody::new(bytes)))
    }

    fn create_stream_from_reader(&self, reader: Box<dyn Read>, limit: u64) -> Box<dyn Body> {
        Box::new(LazyBody::new(reader, limit))
    }
}

impl UploadedFileFactory<HttpUploadedFile> for MessageFactory {
    fn create_uploaded_file(
        &self,
        stream: Box<dyn Body>,
        size: Option<u64>,
        error: i32,
        client_filename: Option<&str>,
        client_media_type: Option<&str>,
    ) -> HttpUploadedFile {
        HttpUploadedFile::new(stream, size, error, client_filename, client_media_type)
    }
}

impl UriFactory<HttpUri> for MessageFactory {
    fn create_uri(&self, uri: &str) -> Result<HttpUri, HttpError> {
        uri.parse()
    }
}
