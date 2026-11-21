use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Body, Headers, HttpError, Message, Method, Request, SharedBody, Uri};

use crate::{BytesBody, HttpUri};

/// A request.
#[derive(Clone)]
pub struct HttpRequest {
    pub(crate) method: Method,
    pub(crate) uri: HttpUri,
    pub(crate) target: Option<String>,
    pub(crate) version: String,
    pub(crate) headers: Headers,
    pub(crate) body: SharedBody,
}

impl HttpRequest {
    /// A request with an empty body, HTTP/1.1, and a `Host` header from the URI.
    pub fn new(method: Method, uri: HttpUri) -> HttpRequest {
        let headers = match uri.host() {
            "" => Headers::new(),
            _ => Headers::new()
                .with("Host", vec![uri.authority()])
                .unwrap_or_default(),
        };
        HttpRequest {
            method,
            uri,
            target: None,
            version: "1.1".into(),
            headers,
            body: Rc::new(RefCell::new(Box::new(BytesBody::default()))),
        }
    }
}

impl Message for HttpRequest {
    fn protocol_version(&self) -> &str {
        &self.version
    }

    fn headers(&self) -> &Headers {
        &self.headers
    }

    fn body(&self) -> SharedBody {
        self.body.clone()
    }

    fn with_protocol_version(mut self, version: &str) -> HttpRequest {
        self.version = version.to_string();
        self
    }

    fn with_header(mut self, name: &str, values: Vec<String>) -> Result<HttpRequest, HttpError> {
        self.headers = self.headers.with(name, values)?;
        Ok(self)
    }

    fn with_added_header(
        mut self,
        name: &str,
        values: Vec<String>,
    ) -> Result<HttpRequest, HttpError> {
        self.headers = self.headers.with_added(name, values)?;
        Ok(self)
    }

    fn without_header(mut self, name: &str) -> HttpRequest {
        self.headers = self.headers.without(name);
        self
    }

    fn with_body(mut self, body: Box<dyn Body>) -> HttpRequest {
        self.body = Rc::new(RefCell::new(body));
        self
    }
}

impl Request for HttpRequest {
    type Uri = HttpUri;

    fn method(&self) -> &Method {
        &self.method
    }

    fn request_target(&self) -> String {
        if let Some(target) = &self.target {
            return target.clone();
        }
        let path = if self.uri.path().is_empty() {
            "/"
        } else {
            self.uri.path()
        };
        match self.uri.query() {
            "" => path.to_string(),
            query => format!("{path}?{query}"),
        }
    }

    fn uri(&self) -> &HttpUri {
        &self.uri
    }

    fn with_method(mut self, method: Method) -> HttpRequest {
        self.method = method;
        self
    }

    fn with_request_target(mut self, target: &str) -> HttpRequest {
        self.target = Some(target.to_string());
        self
    }

    fn with_uri(mut self, uri: HttpUri) -> HttpRequest {
        if !uri.host().is_empty() {
            let headers = std::mem::take(&mut self.headers);
            self.headers = match headers.clone().with("Host", vec![uri.authority()]) {
                Ok(with_host) => with_host,
                Err(_) => headers,
            };
        }
        self.uri = uri;
        self
    }
}
