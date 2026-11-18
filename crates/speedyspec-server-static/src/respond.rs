//! Answering a request from a static set.

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};

use crate::encoding::{Encoding, choose};
use crate::range::{RangeOutcome, parse};
use crate::{StaticFile, StaticSet, encode_path};

impl StaticSet {
    /// Answers `method` for `rel_path`: the already-normalized, percent-decoded path after the mount prefix, without a
    /// leading slash (`""` is the mount root). `mount_prefix` and `query` are only used to build redirects.
    pub fn respond(
        &self,
        method: &Method,
        mount_prefix: &str,
        rel_path: &str,
        query: Option<&str>,
        headers: &HeaderMap,
    ) -> Response<Bytes> {
        if method != Method::GET && method != Method::HEAD {
            return plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
                .with_header(header::ALLOW, "GET, HEAD");
        }
        let head = method == Method::HEAD;

        let found = if rel_path.is_empty() || rel_path.ends_with('/') {
            self.files.get(&format!("{rel_path}index.html"))
        } else if let Some(file) = self.files.get(rel_path) {
            Some(file)
        } else if self.files.contains_key(&format!("{rel_path}/index.html")) {
            // A directory with an index: redirect to the slash form so its relative links resolve. A directory
            // without one falls through to `<rel>.html` below.
            let path = encode_path(&format!("{mount_prefix}{rel_path}/"));
            let location = match query {
                Some(query) => format!("{path}?{query}"),
                None => path,
            };
            return plain(StatusCode::PERMANENT_REDIRECT, "moved")
                .with_header(header::LOCATION, &location);
        } else {
            self.files.get(&format!("{rel_path}.html"))
        };

        match found {
            Some(file) => serve(file, headers, head),
            None => match self
                .not_found
                .as_ref()
                .and_then(|name| self.files.get(name))
            {
                Some(page) => {
                    let mut response = body(
                        StatusCode::NOT_FOUND,
                        &page.content_type,
                        page.identity.clone(),
                        head,
                    );
                    set(&mut response, header::CACHE_CONTROL, "no-cache");
                    response
                }
                None => plain(StatusCode::NOT_FOUND, "not found"),
            },
        }
    }
}

fn serve(file: &StaticFile, headers: &HeaderMap, head: bool) -> Response<Bytes> {
    let varies = file.br.is_some() || file.gzip.is_some();
    let len = file.identity.len() as u64;

    // A satisfiable or unsatisfiable range selects the identity body; anything else negotiates a coding.
    let range = headers
        .get(header::RANGE)
        .filter(|_| if_range_holds(headers, &file.etag))
        .map(|value| parse(value, len))
        .filter(|outcome| *outcome != RangeOutcome::Full);
    let encoding = match range {
        Some(_) => Encoding::Identity,
        None => choose(
            headers.get(header::ACCEPT_ENCODING),
            file.br.is_some(),
            file.gzip.is_some(),
        ),
    };
    let etag = etag_for(file, encoding);

    if let Some(value) = headers.get(header::IF_NONE_MATCH)
        && none_match(value, &etag)
    {
        let mut response = empty(StatusCode::NOT_MODIFIED);
        common(&mut response, file, &etag, varies);
        return response;
    }

    match range {
        Some(RangeOutcome::Partial(start, end)) => {
            let slice = file.identity.slice(start as usize..=end as usize);
            let mut response = body(StatusCode::PARTIAL_CONTENT, &file.content_type, slice, head);
            common(&mut response, file, &etag, varies);
            set(
                &mut response,
                header::CONTENT_RANGE,
                &format!("bytes {start}-{end}/{len}"),
            );
            return response;
        }
        Some(_) => {
            let mut response = plain(StatusCode::RANGE_NOT_SATISFIABLE, "range not satisfiable");
            set(
                &mut response,
                header::CONTENT_RANGE,
                &format!("bytes */{len}"),
            );
            return response;
        }
        None => {}
    }

    let (bytes, coding) = match encoding {
        Encoding::Br => (
            file.br.clone().expect("chosen only when present"),
            Some("br"),
        ),
        Encoding::Gzip => (
            file.gzip.clone().expect("chosen only when present"),
            Some("gzip"),
        ),
        Encoding::Identity => (file.identity.clone(), None),
    };
    let mut response = body(StatusCode::OK, &file.content_type, bytes, head);
    common(&mut response, file, &etag, varies);
    if let Some(coding) = coding {
        set(&mut response, header::CONTENT_ENCODING, coding);
    }

    response
}

/// Each coding is its own representation, so it gets its own strong ETag: the identity tag with `-br` or `-gz` added
/// inside the quotes. Ranges and `If-Range` only ever use the identity tag.
fn etag_for(file: &StaticFile, encoding: Encoding) -> String {
    let suffix = match encoding {
        Encoding::Identity => return file.etag.clone(),
        Encoding::Br => "-br",
        Encoding::Gzip => "-gz",
    };
    let inner = file.etag.trim_end_matches('"');
    format!("{inner}{suffix}\"")
}

/// `If-None-Match` uses the weak comparison: `W/` prefixes are ignored.
fn none_match(value: &HeaderValue, etag: &str) -> bool {
    let Ok(value) = value.to_str() else {
        return false;
    };
    value
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.trim_start_matches("W/") == etag)
}

/// A `Range` is honoured without `If-Range`, or when `If-Range` names this file's strong ETag exactly.
fn if_range_holds(headers: &HeaderMap, etag: &str) -> bool {
    match headers.get(header::IF_RANGE) {
        None => true,
        Some(value) => value.to_str().is_ok_and(|value| value.trim() == etag),
    }
}

fn common(response: &mut Response<Bytes>, file: &StaticFile, etag: &str, varies: bool) {
    set(response, header::ETAG, etag);
    set(response, header::CACHE_CONTROL, &file.cache_control);
    if varies {
        set(response, header::VARY, "Accept-Encoding");
    }
}

fn body(status: StatusCode, content_type: &str, bytes: Bytes, head: bool) -> Response<Bytes> {
    let length = bytes.len().to_string();
    let mut response = Response::new(if head { Bytes::new() } else { bytes });
    *response.status_mut() = status;
    set(&mut response, header::CONTENT_TYPE, content_type);
    set(&mut response, header::CONTENT_LENGTH, &length);
    set(&mut response, header::ACCEPT_RANGES, "bytes");
    response
}

fn empty(status: StatusCode) -> Response<Bytes> {
    let mut response = Response::new(Bytes::new());
    *response.status_mut() = status;
    response
}

fn plain(status: StatusCode, text: &'static str) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from_static(text.as_bytes()));
    *response.status_mut() = status;
    set(
        &mut response,
        header::CONTENT_TYPE,
        "text/plain; charset=utf-8",
    );
    set(
        &mut response,
        header::CONTENT_LENGTH,
        &text.len().to_string(),
    );
    response
}

fn set(response: &mut Response<Bytes>, name: header::HeaderName, value: &str) {
    response.headers_mut().insert(
        name,
        HeaderValue::from_str(value).expect("header values here are ASCII"),
    );
}

trait WithHeader {
    fn with_header(self, name: header::HeaderName, value: &str) -> Self;
}

impl WithHeader for Response<Bytes> {
    fn with_header(mut self, name: header::HeaderName, value: &str) -> Self {
        set(&mut self, name, value);
        self
    }
}
