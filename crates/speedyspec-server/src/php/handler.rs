//! `PhpRequestHandler`: one live PHP release's answer to a request.
//!
//! A path names an existing `.php` file under the document root (`PATH_INFO` is what follows it), a directory with
//! an `index.php`, or a static file (from `site_dir` first, then the release); everything else goes to the entry
//! (the front controller). PHP requests go through a leased worker over FastCGI: the request body is sent as it
//! arrives and the response is passed on as it is produced, each held back by the other side, and the whole request
//! is bounded by `request_timeout`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use salvo::http::{HeaderName, HeaderValue, StatusCode};
use salvo::{Request, Response};
use speedyspec_fastcgi::{Client, Event};
use speedyspec_server_presentation::{PhpCall, PhpEndpoint};

use super::REPORT_PREFIX;
use super::pool::{EmbeddedPhpPool, Fate};

/// See the module docs.
#[derive(Debug)]
pub struct PhpRequestHandler {
    pool: EmbeddedPhpPool,
    /// The release's document root, canonical.
    document_root: PathBuf,
    /// The front controller, under the document root.
    entry: PathBuf,
    site_dir: Option<PathBuf>,
}

/// Where a request goes.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    Script {
        script: PathBuf,
        script_name: String,
        path_info: String,
    },
    Static(PathBuf),
}

impl PhpRequestHandler {
    pub fn new(
        pool: EmbeddedPhpPool,
        document_root: &Path,
        entry: &str,
        site_dir: Option<PathBuf>,
    ) -> io::Result<Self> {
        let document_root = std::fs::canonicalize(document_root)?;
        let entry = document_root.join(entry);
        Ok(PhpRequestHandler {
            pool,
            document_root,
            entry,
            site_dir,
        })
    }

    pub fn pool(&self) -> &EmbeddedPhpPool {
        &self.pool
    }

    /// Is `path` an existing file inside `root` (symlinks resolved)?
    fn file_inside(root: &Path, path: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(path).ok()?;
        let root = std::fs::canonicalize(root).ok()?;
        (real.starts_with(&root) && real.is_file()).then_some(real)
    }

    fn target(&self, mount: &str, rel: &str) -> Target {
        let rel = rel.trim_start_matches('/');
        let is_php = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("php"));
        if !rel.is_empty() {
            for root in self
                .site_dir
                .iter()
                .chain(std::iter::once(&self.document_root))
            {
                if let Some(file) = Self::file_inside(root, &root.join(rel)) {
                    if !is_php(&file) {
                        return Target::Static(file);
                    }
                }
            }
        }
        let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        let mut path = self.document_root.clone();
        for (i, segment) in segments.iter().enumerate() {
            path.push(segment);
            if is_php(&path) {
                if let Some(script) = Self::file_inside(&self.document_root, &path) {
                    let rest = segments[i + 1..].join("/");
                    let path_info = if rest.is_empty() {
                        String::new()
                    } else {
                        format!("/{rest}")
                    };
                    return Target::Script {
                        script,
                        script_name: format!("{mount}{}", segments[..=i].join("/")),
                        path_info,
                    };
                }
            }
        }
        if path.is_dir() {
            if let Some(index) = Self::file_inside(&self.document_root, &path.join("index.php")) {
                let dir = segments.join("/");
                let script_name = if dir.is_empty() {
                    format!("{mount}index.php")
                } else {
                    format!("{mount}{dir}/index.php")
                };
                return Target::Script {
                    script: index,
                    script_name,
                    path_info: String::new(),
                };
            }
        }
        let entry_name = self
            .entry
            .strip_prefix(&self.document_root)
            .unwrap_or(&self.entry)
            .display()
            .to_string();
        Target::Script {
            script: self.entry.clone(),
            script_name: format!("{mount}{entry_name}"),
            path_info: String::new(),
        }
    }

    fn params(
        &self,
        call: &PhpCall,
        req: &Request,
        script: &Path,
        script_name: &str,
        path_info: &str,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut params: Vec<(String, String)> = vec![
            ("GATEWAY_INTERFACE".into(), "CGI/1.1".into()),
            (
                "SERVER_SOFTWARE".into(),
                format!("speedyspec-server/{}", env!("CARGO_PKG_VERSION")),
            ),
            ("SERVER_PROTOCOL".into(), format!("{:?}", req.version())),
            ("REQUEST_METHOD".into(), req.method().as_str().to_string()),
            (
                "REQUEST_URI".into(),
                req.uri()
                    .path_and_query()
                    .map_or_else(|| req.uri().path().to_string(), |p| p.as_str().to_string()),
            ),
            (
                "QUERY_STRING".into(),
                req.uri().query().unwrap_or("").to_string(),
            ),
            ("SCRIPT_FILENAME".into(), script.display().to_string()),
            ("SCRIPT_NAME".into(), script_name.to_string()),
            ("PHP_SELF".into(), format!("{script_name}{path_info}")),
            (
                "DOCUMENT_ROOT".into(),
                self.document_root.display().to_string(),
            ),
            ("DOCUMENT_URI".into(), req.uri().path().to_string()),
            ("SERVER_NAME".into(), call.origin.host.clone()),
            ("REMOTE_ADDR".into(), call.origin.client.to_string()),
            ("SPEEDYSPEC_MOUNT".into(), call.mount_prefix.clone()),
        ];
        if !path_info.is_empty() {
            params.push(("PATH_INFO".into(), path_info.to_string()));
            params.push((
                "PATH_TRANSLATED".into(),
                self.document_root
                    .join(path_info.trim_start_matches('/'))
                    .display()
                    .to_string(),
            ));
        }
        if let Some(port) = req.local_addr().clone().into_std().map(|a| a.port()) {
            params.push(("SERVER_PORT".into(), port.to_string()));
        }
        if call.origin.https {
            params.push(("HTTPS".into(), "on".into()));
            params.push(("REQUEST_SCHEME".into(), "https".into()));
        } else {
            params.push(("REQUEST_SCHEME".into(), "http".into()));
        }
        for (name, value) in req.headers() {
            let Ok(value) = value.to_str() else {
                continue;
            };
            let key = match name.as_str() {
                "content-type" => "CONTENT_TYPE".to_string(),
                "content-length" => "CONTENT_LENGTH".to_string(),
                other => format!("HTTP_{}", other.to_ascii_uppercase().replace('-', "_")),
            };
            match params.iter_mut().find(|(k, _)| *k == key) {
                // Repeated headers join with ", " (cookies with "; ").
                Some((_, existing)) => {
                    let sep = if key == "HTTP_COOKIE" { "; " } else { ", " };
                    existing.push_str(sep);
                    existing.push_str(value);
                }
                None => params.push((key, value.to_string())),
            }
        }
        params
            .into_iter()
            .map(|(k, v)| (k.into_bytes(), v.into_bytes()))
            .collect()
    }

    async fn serve_static(&self, file: &Path, res: &mut Response) {
        match tokio::fs::File::open(file).await {
            Ok(handle) => {
                res.status_code(StatusCode::OK);
                res.headers_mut().insert(
                    salvo::http::header::CONTENT_TYPE,
                    HeaderValue::from_static(content_type(file)),
                );
                if let Ok(meta) = handle.metadata().await {
                    if let Ok(len) = HeaderValue::from_str(&meta.len().to_string()) {
                        res.headers_mut()
                            .insert(salvo::http::header::CONTENT_LENGTH, len);
                    }
                }
                res.stream(tokio_util::io::ReaderStream::new(handle));
            }
            Err(_) => plain(res, StatusCode::NOT_FOUND, "not found"),
        }
    }
}

fn plain(res: &mut Response, status: StatusCode, text: &'static str) {
    res.status_code(status);
    res.headers_mut().insert(
        salvo::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    res.body(text);
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("xml") => "application/xml",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// The CGI response head: the status and headers before the first blank line.
fn parse_head(head: &[u8]) -> (StatusCode, Vec<(HeaderName, HeaderValue)>) {
    let text = String::from_utf8_lossy(head);
    let mut status = None;
    let mut headers = Vec::new();
    for line in text
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty())
    {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            status = value
                .split_whitespace()
                .next()
                .and_then(|code| code.parse::<u16>().ok());
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.trim().as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.push((name, value));
        }
    }
    let has_location = headers
        .iter()
        .any(|(n, _)| n == salvo::http::header::LOCATION);
    let code = status.unwrap_or(if has_location { 302 } else { 200 });
    (
        StatusCode::from_u16(code).unwrap_or(StatusCode::OK),
        headers,
    )
}

/// The end of the CGI head in `buf`: the index after the blank line.
fn head_end(buf: &[u8]) -> Option<usize> {
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Logs a worker's stderr, or reads its after-request report.
fn stderr(bytes: &[u8], report: &mut Option<(u64, u64, bool)>) {
    if let Some(rest) = bytes.strip_prefix(REPORT_PREFIX) {
        let text = String::from_utf8_lossy(rest);
        let field = |name: &str| {
            text.split_whitespace()
                .find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
                .and_then(|v| v.parse::<u64>().ok())
        };
        *report = Some((
            field("memory").unwrap_or(0),
            field("requests").unwrap_or(0),
            field("poisoned") == Some(1),
        ));
        return;
    }
    tracing::warn!(event = "php_stderr", message = %String::from_utf8_lossy(bytes).trim_end());
}

#[salvo::async_trait]
impl PhpEndpoint for PhpRequestHandler {
    async fn serve(&self, call: PhpCall, req: &mut Request, res: &mut Response) {
        let (script, script_name, path_info) = match self.target(&call.mount_prefix, &call.rel_path)
        {
            Target::Static(file) => return self.serve_static(&file, res).await,
            Target::Script {
                script,
                script_name,
                path_info,
            } => (script, script_name, path_info),
        };
        let params = self.params(&call, req, &script, &script_name, &path_info);
        let deadline = tokio::time::Instant::now() + self.pool.request_timeout();
        let lease = match tokio::time::timeout_at(deadline, self.pool.acquire()).await {
            Ok(Ok(lease)) => lease,
            Ok(Err(why)) => {
                tracing::warn!(event = "php_unavailable", reason = %why);
                return plain(res, StatusCode::SERVICE_UNAVAILABLE, "service unavailable");
            }
            Err(_) => return plain(res, StatusCode::GATEWAY_TIMEOUT, "gateway timeout"),
        };
        let body = req.take_body();
        let body = futures_util::stream::unfold(body, |mut body| async move {
            loop {
                match body.frame().await {
                    None => return None,
                    Some(Err(err)) => return Some((Err(io::Error::other(err.to_string())), body)),
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data() {
                            return Some((Ok::<Bytes, io::Error>(data), body));
                        }
                    }
                }
            }
        });
        let socket = lease.socket().to_path_buf();
        let mut response = match Client::request(&socket, params, body).await {
            Ok(response) => response,
            Err(err) => {
                self.pool.release(lease, Fate::Crashed(err.to_string()));
                return plain(res, StatusCode::BAD_GATEWAY, "bad gateway");
            }
        };

        // The head: everything before the first blank line of the worker's output.
        let mut buffered = Vec::new();
        let mut report = None;
        let split = loop {
            match tokio::time::timeout_at(deadline, response.next()).await {
                Err(_) => {
                    drop(response);
                    self.pool.release(lease, Fate::TimedOut);
                    return plain(res, StatusCode::GATEWAY_TIMEOUT, "gateway timeout");
                }
                Ok(None) | Ok(Some(Err(_))) => {
                    self.pool.release(
                        lease,
                        Fate::Crashed("the worker closed the connection mid-request".into()),
                    );
                    return plain(res, StatusCode::BAD_GATEWAY, "bad gateway");
                }
                Ok(Some(Ok(Event::Stderr(bytes)))) => stderr(&bytes, &mut report),
                Ok(Some(Ok(Event::Stdout(bytes)))) => {
                    buffered.extend_from_slice(&bytes);
                    if let Some(end) = head_end(&buffered) {
                        break Some(end);
                    }
                }
                Ok(Some(Ok(Event::End(_)))) => break None,
            }
        };
        let (status, headers, rest, ended) = match split {
            Some(end) => {
                let (status, headers) = parse_head(&buffered[..end]);
                (status, headers, buffered.split_off(end), false)
            }
            // Output with no head at all: PHP always sends one, so this is a broken worker.
            None => {
                let (memory, requests, poisoned) = report.unwrap_or((0, 0, false));
                self.pool.release(
                    lease,
                    Fate::Answered {
                        memory,
                        requests,
                        poisoned,
                    },
                );
                return plain(res, StatusCode::BAD_GATEWAY, "bad gateway");
            }
        };
        res.status_code(status);
        for (name, value) in headers {
            res.headers_mut().append(name, value);
        }

        // The body, passed on as it comes; a slow client holds the worker back (bounded by the deadline).
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, io::Error>>(4);
        let pool = self.pool.clone();
        tokio::spawn(async move {
            if !rest.is_empty() && tx.send(Ok(Bytes::from(rest))).await.is_err() {
                pool.release(lease, Fate::Crashed("the client went away".into()));
                return;
            }
            let _ = ended;
            loop {
                let next = tokio::time::timeout_at(deadline, response.next()).await;
                match next {
                    Err(_) => {
                        drop(response);
                        let _ = tx
                            .send(Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "request_timeout",
                            )))
                            .await;
                        pool.release(lease, Fate::TimedOut);
                        return;
                    }
                    Ok(None) => {
                        let (memory, requests, poisoned) = report.unwrap_or((0, 0, false));
                        pool.release(
                            lease,
                            Fate::Answered {
                                memory,
                                requests,
                                poisoned,
                            },
                        );
                        return;
                    }
                    Ok(Some(Err(err))) => {
                        let _ = tx.send(Err(err)).await;
                        pool.release(
                            lease,
                            Fate::Crashed("the worker closed the connection mid-response".into()),
                        );
                        return;
                    }
                    Ok(Some(Ok(Event::Stderr(bytes)))) => stderr(&bytes, &mut report),
                    Ok(Some(Ok(Event::End(_)))) => {}
                    Ok(Some(Ok(Event::Stdout(bytes)))) => {
                        let sent = tokio::time::timeout_at(deadline, tx.send(Ok(bytes))).await;
                        match sent {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) => {
                                drop(response);
                                pool.release(lease, Fate::Crashed("the client went away".into()));
                                return;
                            }
                            Err(_) => {
                                drop(response);
                                pool.release(lease, Fate::TimedOut);
                                return;
                            }
                        }
                    }
                }
            }
        });
        res.stream(tokio_stream_from(rx));
    }
}

fn tokio_stream_from(
    rx: tokio::sync::mpsc::Receiver<Result<Bytes, io::Error>>,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
    .boxed()
}

pub(crate) fn arc(handler: PhpRequestHandler) -> Arc<dyn PhpEndpoint> {
    Arc::new(handler)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgi_head_is_parsed() {
        let (status, headers) = parse_head(b"Status: 404 Not Found\r\nContent-Type: text/html\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers.iter().filter(|(n, _)| n == "set-cookie").count(), 2);
        let (status, _) = parse_head(b"Location: /x\r\n");
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(head_end(b"A: b\r\n\r\nbody"), Some(8));
        assert_eq!(head_end(b"A: b\n\nbody"), Some(6));
    }
}
