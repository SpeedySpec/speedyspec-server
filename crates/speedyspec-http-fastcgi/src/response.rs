use speedyspec_http::{HttpError, Message, Response};
use speedyspec_http_message::HttpResponse;

/// Writes a native response as CGI output: a `Status` line, the headers, a blank line, the body.
///
/// A response PHP already wrote is refused, since its bytes are already on their way to the client.
pub fn response_to_cgi(response: HttpResponse) -> Result<Vec<u8>, HttpError> {
    let body = response.body();
    if body.borrow().is_sent() {
        return Err(HttpError::BodyAlreadySent);
    }
    let mut out = format!(
        "Status: {} {}\r\n",
        response.status().as_u16(),
        response.reason_phrase()
    )
    .into_bytes();
    for (name, values) in response.headers().iter() {
        for value in values {
            out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&body.borrow_mut().read_all()?);
    Ok(out)
}
