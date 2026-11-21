use speedyspec_http::{
    Attributes, Body, Headers, HttpError, Message, Method, Request, ServerRequest, SharedBody,
};

use crate::{HttpRequest, HttpUploadedFile, HttpUri};

/// A request as the runtime received it, with the steps' attributes.
#[derive(Clone)]
pub struct HttpServerRequest {
    request: HttpRequest,
    server_params: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
    query: Vec<(String, String)>,
    parsed_body: Option<Vec<(String, String)>>,
    files: Vec<(String, HttpUploadedFile)>,
    attributes: Attributes,
}

impl HttpServerRequest {
    pub fn new(request: HttpRequest, server_params: Vec<(String, String)>) -> HttpServerRequest {
        HttpServerRequest {
            request,
            server_params,
            cookies: Vec::new(),
            query: Vec::new(),
            parsed_body: None,
            files: Vec::new(),
            attributes: Attributes::new(),
        }
    }

    /// The server parameter named exactly `name`.
    pub fn server_param(&self, name: &str) -> Option<&str> {
        self.server_params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn with_uploaded_files(
        mut self,
        files: Vec<(String, HttpUploadedFile)>,
    ) -> HttpServerRequest {
        self.files = files;
        self
    }
}

impl Message for HttpServerRequest {
    fn protocol_version(&self) -> &str {
        self.request.protocol_version()
    }

    fn headers(&self) -> &Headers {
        self.request.headers()
    }

    fn body(&self) -> SharedBody {
        self.request.body()
    }

    fn with_protocol_version(mut self, version: &str) -> HttpServerRequest {
        self.request = self.request.with_protocol_version(version);
        self
    }

    fn with_header(
        mut self,
        name: &str,
        values: Vec<String>,
    ) -> Result<HttpServerRequest, HttpError> {
        self.request = self.request.with_header(name, values)?;
        Ok(self)
    }

    fn with_added_header(
        mut self,
        name: &str,
        values: Vec<String>,
    ) -> Result<HttpServerRequest, HttpError> {
        self.request = self.request.with_added_header(name, values)?;
        Ok(self)
    }

    fn without_header(mut self, name: &str) -> HttpServerRequest {
        self.request = self.request.without_header(name);
        self
    }

    fn with_body(mut self, body: Box<dyn Body>) -> HttpServerRequest {
        self.request = self.request.with_body(body);
        self
    }
}

impl Request for HttpServerRequest {
    type Uri = HttpUri;

    fn method(&self) -> &Method {
        self.request.method()
    }

    fn request_target(&self) -> String {
        self.request.request_target()
    }

    fn uri(&self) -> &HttpUri {
        self.request.uri()
    }

    fn with_method(mut self, method: Method) -> HttpServerRequest {
        self.request = self.request.with_method(method);
        self
    }

    fn with_request_target(mut self, target: &str) -> HttpServerRequest {
        self.request = self.request.with_request_target(target);
        self
    }

    fn with_uri(mut self, uri: HttpUri) -> HttpServerRequest {
        self.request = self.request.with_uri(uri);
        self
    }
}

impl ServerRequest for HttpServerRequest {
    type File = HttpUploadedFile;

    fn server_params(&self) -> &[(String, String)] {
        &self.server_params
    }

    fn cookie_params(&self) -> &[(String, String)] {
        &self.cookies
    }

    fn query_params(&self) -> &[(String, String)] {
        &self.query
    }

    fn parsed_body(&self) -> Option<&[(String, String)]> {
        self.parsed_body.as_deref()
    }

    fn uploaded_files(&self) -> &[(String, HttpUploadedFile)] {
        &self.files
    }

    fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    fn with_attribute<T: 'static>(mut self, value: T) -> HttpServerRequest {
        self.attributes = self.attributes.with(value);
        self
    }

    fn with_query_params(mut self, params: Vec<(String, String)>) -> HttpServerRequest {
        self.query = params;
        self
    }

    fn with_parsed_body(mut self, body: Option<Vec<(String, String)>>) -> HttpServerRequest {
        self.parsed_body = body;
        self
    }

    fn with_cookie_params(mut self, cookies: Vec<(String, String)>) -> HttpServerRequest {
        self.cookies = cookies;
        self
    }
}
