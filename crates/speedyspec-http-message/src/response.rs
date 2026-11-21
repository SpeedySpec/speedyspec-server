use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Body, Headers, HttpError, Message, Response, SharedBody, StatusCode};

use crate::{BytesBody, SentBody};

/// The status and headers PHP sent for a response it wrote itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentRecord {
    pub status: StatusCode,
    pub headers: Headers,
}

/// A response.
#[derive(Clone)]
pub struct HttpResponse {
    status: StatusCode,
    reason: String,
    version: String,
    headers: Headers,
    body: SharedBody,
}

impl HttpResponse {
    pub fn new(status: StatusCode) -> HttpResponse {
        HttpResponse {
            status,
            reason: status.reason().to_string(),
            version: "1.1".into(),
            headers: Headers::new(),
            body: Rc::new(RefCell::new(Box::new(BytesBody::default()))),
        }
    }

    /// A response PHP already wrote: its record is readable, its body is not.
    pub fn sent(record: SentRecord) -> HttpResponse {
        let mut response = HttpResponse::new(record.status);
        response.headers = record.headers;
        response.body = Rc::new(RefCell::new(Box::new(SentBody)));
        response
    }
}

impl Message for HttpResponse {
    fn protocol_version(&self) -> &str {
        &self.version
    }

    fn headers(&self) -> &Headers {
        &self.headers
    }

    fn body(&self) -> SharedBody {
        self.body.clone()
    }

    fn with_protocol_version(mut self, version: &str) -> HttpResponse {
        self.version = version.to_string();
        self
    }

    fn with_header(mut self, name: &str, values: Vec<String>) -> Result<HttpResponse, HttpError> {
        self.headers = self.headers.with(name, values)?;
        Ok(self)
    }

    fn with_added_header(
        mut self,
        name: &str,
        values: Vec<String>,
    ) -> Result<HttpResponse, HttpError> {
        self.headers = self.headers.with_added(name, values)?;
        Ok(self)
    }

    fn without_header(mut self, name: &str) -> HttpResponse {
        self.headers = self.headers.without(name);
        self
    }

    fn with_body(mut self, body: Box<dyn Body>) -> HttpResponse {
        self.body = Rc::new(RefCell::new(body));
        self
    }
}

impl Response for HttpResponse {
    fn status(&self) -> StatusCode {
        self.status
    }

    fn reason_phrase(&self) -> &str {
        &self.reason
    }

    fn with_status(mut self, status: StatusCode, reason: &str) -> HttpResponse {
        self.status = status;
        self.reason = if reason.is_empty() {
            status.reason().to_string()
        } else {
            reason.to_string()
        };
        self
    }
}
