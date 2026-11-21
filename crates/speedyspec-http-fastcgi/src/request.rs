use std::io::Read;

use speedyspec_http::{HttpError, Message, Method, ServerRequest};
use speedyspec_http_message::{HttpRequest, HttpServerRequest, HttpUri, LazyBody};

use crate::decode::url_decode;

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Builds the server request the pipeline works on.
///
/// Query params are flat, URL-decoded pairs; PHP's nesting of `a[b]` names is not reproduced here, because only
/// rewrite-rule queries need it and the router decodes those itself. The body stays unread until a step asks, and is
/// capped at `body_limit` bytes.
pub fn server_request_from_params(
    params: &[(String, String)],
    stdin: Box<dyn Read>,
    body_limit: u64,
) -> Result<HttpServerRequest, HttpError> {
    let method = Method::parse(param(params, "REQUEST_METHOD").unwrap_or("GET"));
    let https =
        param(params, "HTTPS").is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("off"));
    let scheme = if https { "https" } else { "http" };
    let authority = match param(params, "HTTP_HOST") {
        Some(host) if !host.is_empty() => host.to_string(),
        _ => {
            let name = param(params, "SERVER_NAME").unwrap_or("localhost");
            match param(params, "SERVER_PORT") {
                Some(port) if !port.is_empty() => format!("{name}:{port}"),
                _ => name.to_string(),
            }
        }
    };
    let target = param(params, "REQUEST_URI").unwrap_or("/");
    let uri: HttpUri = format!("{scheme}://{authority}{target}").parse()?;

    let mut request = HttpRequest::new(method, uri);
    for (name, value) in params {
        let header = if let Some(rest) = name.strip_prefix("HTTP_") {
            header_name(rest)
        } else if name == "CONTENT_TYPE" || name == "CONTENT_LENGTH" {
            header_name(name)
        } else {
            continue;
        };
        if value.is_empty() && name == "CONTENT_LENGTH" {
            continue;
        }
        request = request.with_header(&header, vec![value.clone()])?;
    }
    request = request.with_body(Box::new(LazyBody::new(stdin, body_limit)));

    let cookies = param(params, "HTTP_COOKIE")
        .map(split_cookies)
        .unwrap_or_default();
    let query = param(params, "QUERY_STRING")
        .map(split_query)
        .unwrap_or_default();
    Ok(HttpServerRequest::new(request, params.to_vec())
        .with_cookie_params(cookies)
        .with_query_params(query))
}

/// `X_FORWARDED_FOR` → `X-Forwarded-For`.
fn header_name(param: &str) -> String {
    param
        .split('_')
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// PHP's cookie parsing: pairs separated by `;`, leading spaces trimmed, values URL-decoded.
fn split_cookies(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .map(str::trim_start)
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (url_decode(name), url_decode(value)),
            None => (url_decode(pair), String::new()),
        })
        .collect()
}

fn split_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (url_decode(name), url_decode(value)),
            None => (url_decode(pair), String::new()),
        })
        .collect()
}
