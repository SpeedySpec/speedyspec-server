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

use super::pool::{EmbeddedPhpPool, Fate};
use speedyspec_runtime_contract::WorkerReport;

/// The most a server's own call may answer with.
const INTERNAL_RESPONSE_LIMIT: usize = 1024 * 1024;

/// A request body, as it arrives.
type BodyStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, io::Error>> + Send>>;

/// Why a server's own call to a PHP app has no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InternalError {
    /// No answer within the call's timeout.
    Timeout,
    /// The app could not answer: retired, crashed, not a script, or an oversized answer.
    Unavailable(String),
}

impl std::fmt::Display for InternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalError::Timeout => f.write_str("no answer in time"),
            InternalError::Unavailable(why) => f.write_str(why),
        }
    }
}

/// What a request's CGI parameters are built from: a visitor's request, or a server's own call.
struct Facts<'a> {
    method: &'a str,
    path_and_query: &'a str,
    path: &'a str,
    query: &'a str,
    version: String,
    headers: &'a http::HeaderMap,
    port: Option<u16>,
    host: &'a str,
    /// The client's address; empty for a server's own call.
    client: String,
    https: bool,
    mount: &'a str,
}

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
    /// A directory named without its trailing slash: `301` to the same path with it.
    Redirect(String),
}

fn is_php(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("php"))
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

    /// One of the server's own calls (a health probe, a gate question, a migration) on the pool's internal worker,
    /// answered whole. `path` is the app's path with any query (`/v1/gate/session`); the request is built as a
    /// visitor's to `mount` on `host` would be, with no client address. Bounded by `timeout` in all.
    #[allow(clippy::too_many_arguments)]
    pub async fn internal(
        &self,
        mount: &str,
        host: &str,
        method: http::Method,
        path: &str,
        headers: http::HeaderMap,
        body: Bytes,
        timeout: std::time::Duration,
    ) -> Result<http::Response<Bytes>, InternalError> {
        let (path_only, query) = path
            .split_once('?')
            .map_or((path, None), |(p, q)| (p, Some(q)));
        let (script, script_name, path_info) =
            match self.target(mount, path_only.trim_start_matches('/'), query) {
                Target::Script {
                    script,
                    script_name,
                    path_info,
                } => (script, script_name, path_info),
                Target::Static(_) | Target::Redirect(_) => {
                    return Err(InternalError::Unavailable(format!(
                        "{path} is not a PHP script"
                    )));
                }
            };
        let request_path = format!("{}{}", mount.trim_end_matches('/'), path_only);
        let path_and_query = match query {
            Some(query) => format!("{request_path}?{query}"),
            None => request_path.clone(),
        };
        let facts = Facts {
            method: method.as_str(),
            path_and_query: &path_and_query,
            path: &request_path,
            query: query.unwrap_or(""),
            version: "HTTP/1.1".to_string(),
            headers: &headers,
            port: None,
            host,
            client: String::new(),
            https: false,
            mount,
        };
        let params = self.params_from(&facts, &script, &script_name, &path_info);
        let deadline = tokio::time::Instant::now() + timeout;
        let lease = match tokio::time::timeout_at(deadline, self.pool.acquire_internal()).await {
            Ok(Ok(lease)) => lease,
            Ok(Err(why)) => return Err(InternalError::Unavailable(why)),
            Err(_) => return Err(InternalError::Timeout),
        };
        let (head_tx, head_rx) = tokio::sync::oneshot::channel();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Bytes, io::Error>>(4);
        let body: BodyStream = Box::pin(futures_util::stream::once(async move { Ok(body) }));
        tokio::spawn(exchange(
            self.pool.clone(),
            lease,
            params,
            body,
            deadline,
            head_tx,
            tx,
        ));
        let (status, headers) = match head_rx.await {
            Ok(Ok((status, headers, _))) => (status, headers),
            Ok(Err(NoHead::Timeout)) => return Err(InternalError::Timeout),
            Ok(Err(NoHead::BadGateway)) | Err(_) => {
                return Err(InternalError::Unavailable(
                    "the worker gave no answer".into(),
                ));
            }
        };
        let mut answer = Vec::new();
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Err(_) => return Err(InternalError::Timeout),
                Ok(None) => break,
                Ok(Some(Err(err))) if err.kind() == io::ErrorKind::TimedOut => {
                    return Err(InternalError::Timeout);
                }
                Ok(Some(Err(err))) => return Err(InternalError::Unavailable(err.to_string())),
                Ok(Some(Ok(chunk))) => {
                    answer.extend_from_slice(&chunk);
                    if answer.len() > INTERNAL_RESPONSE_LIMIT {
                        return Err(InternalError::Unavailable(format!(
                            "{path} answered more than {INTERNAL_RESPONSE_LIMIT} bytes"
                        )));
                    }
                }
            }
        }
        let mut response = http::Response::new(Bytes::from(answer));
        *response.status_mut() = status;
        for (name, value) in headers {
            response.headers_mut().append(name, value);
        }
        Ok(response)
    }

    /// Is `path` an existing file inside `root` (symlinks resolved)?
    fn file_inside(root: &Path, path: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(path).ok()?;
        let root = std::fs::canonicalize(root).ok()?;
        (real.starts_with(&root) && real.is_file()).then_some(real)
    }

    /// The first `.php` file along `segments` under `root`: the script, and its `SCRIPT_NAME` and `PATH_INFO`.
    fn script_along(root: &Path, segments: &[&str], url_prefix: &str) -> Option<Target> {
        let mut path = root.to_path_buf();
        for (i, segment) in segments.iter().enumerate() {
            path.push(segment);
            if is_php(&path)
                && let Some(script) = Self::file_inside(root, &path)
            {
                let rest = segments[i + 1..].join("/");
                let path_info = if rest.is_empty() {
                    String::new()
                } else {
                    format!("/{rest}")
                };
                return Some(Target::Script {
                    script,
                    script_name: format!("{url_prefix}{}", segments[..=i].join("/")),
                    path_info,
                });
            }
        }
        None
    }

    /// Maps a path below the mount to its target. Only `site_dir/wp-content` is reachable in the site directory,
    /// which also holds the app's secrets (`.env`); a path with a dot-segment (`.env`, `.git/`) is never served or
    /// run, and goes to the front controller.
    fn target(&self, mount: &str, rel: &str, query: Option<&str>) -> Target {
        let trailing = rel.ends_with('/');
        let rel = rel.trim_start_matches('/');
        let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        let hidden = segments.iter().any(|s| s.starts_with('.'));
        let content = self.site_dir.as_ref().map(|site| site.join("wp-content"));
        let in_content = segments.first() == Some(&"wp-content");
        if !hidden && !segments.is_empty() {
            if let (true, Some(content)) = (in_content, &content) {
                let below = segments[1..].join("/");
                if let Some(file) = Self::file_inside(content, &content.join(&below))
                    && !is_php(&file)
                {
                    return Target::Static(file);
                }
            }
            if let Some(file) =
                Self::file_inside(&self.document_root, &self.document_root.join(rel))
                && !is_php(&file)
            {
                return Target::Static(file);
            }
        }
        if !hidden {
            if let (true, Some(content)) = (in_content, &content)
                && let Some(script) =
                    Self::script_along(content, &segments[1..], &format!("{mount}wp-content/"))
            {
                return script;
            }
            if let Some(script) = Self::script_along(&self.document_root, &segments, mount) {
                return script;
            }
            let dir = self.document_root.join(segments.join("/"));
            if dir.is_dir()
                && let Some(index) = Self::file_inside(&self.document_root, &dir.join("index.php"))
            {
                let joined = segments.join("/");
                if !joined.is_empty() && !trailing {
                    let query = query.map(|q| format!("?{q}")).unwrap_or_default();
                    return Target::Redirect(format!("{mount}{joined}/{query}"));
                }
                let script_name = if joined.is_empty() {
                    format!("{mount}index.php")
                } else {
                    format!("{mount}{joined}/index.php")
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
        let path_and_query = req
            .uri()
            .path_and_query()
            .map_or_else(|| req.uri().path().to_string(), |p| p.as_str().to_string());
        let facts = Facts {
            method: req.method().as_str(),
            path_and_query: &path_and_query,
            path: req.uri().path(),
            query: req.uri().query().unwrap_or(""),
            version: format!("{:?}", req.version()),
            headers: req.headers(),
            port: req.local_addr().clone().into_std().map(|a| a.port()),
            host: &call.origin.host,
            client: call.origin.client.to_string(),
            https: call.origin.https,
            mount: &call.mount_prefix,
        };
        self.params_from(&facts, script, script_name, path_info)
    }

    fn params_from(
        &self,
        facts: &Facts<'_>,
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
            ("SERVER_PROTOCOL".into(), facts.version.clone()),
            ("REQUEST_METHOD".into(), facts.method.to_string()),
            ("REQUEST_URI".into(), facts.path_and_query.to_string()),
            ("QUERY_STRING".into(), facts.query.to_string()),
            ("SCRIPT_FILENAME".into(), script.display().to_string()),
            ("SCRIPT_NAME".into(), script_name.to_string()),
            ("PHP_SELF".into(), format!("{script_name}{path_info}")),
            (
                "DOCUMENT_ROOT".into(),
                self.document_root.display().to_string(),
            ),
            ("DOCUMENT_URI".into(), facts.path.to_string()),
            ("SERVER_NAME".into(), facts.host.to_string()),
            ("REMOTE_ADDR".into(), facts.client.clone()),
            ("SPEEDYSPEC_MOUNT".into(), facts.mount.to_string()),
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
        if let Some(port) = facts.port {
            params.push(("SERVER_PORT".into(), port.to_string()));
        }
        if facts.https {
            params.push(("HTTPS".into(), "on".into()));
            params.push(("REQUEST_SCHEME".into(), "https".into()));
        } else {
            params.push(("REQUEST_SCHEME".into(), "http".into()));
        }
        for (name, value) in facts.headers {
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
                if let Ok(meta) = handle.metadata().await
                    && let Ok(len) = HeaderValue::from_str(&meta.len().to_string())
                {
                    res.headers_mut()
                        .insert(salvo::http::header::CONTENT_LENGTH, len);
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
    if let Some(decoded) = WorkerReport::decode(bytes) {
        *report = Some((decoded.memory, decoded.requests, decoded.poisoned));
        return;
    }
    tracing::warn!(event = "php_stderr", message = %String::from_utf8_lossy(bytes).trim_end());
}

/// Why no response head came from the worker.
enum NoHead {
    BadGateway,
    Timeout,
}

/// The response head, and what followed it in the same read.
type Head = (StatusCode, Vec<(HeaderName, HeaderValue)>, Vec<u8>);

/// Runs one FastCGI exchange to its end, on its own task: the head goes to `head`, the body to `body` while anyone
/// reads it. A client that goes away does not stop the script (WP-Cron relies on that): its output is drained and
/// dropped until the worker ends the request, all within `deadline`. The worker is always given back before the
/// client is told anything, so a client that stops reading never holds it.
async fn exchange(
    pool: EmbeddedPhpPool,
    lease: super::pool::Lease,
    params: Vec<(Vec<u8>, Vec<u8>)>,
    body: BodyStream,
    deadline: tokio::time::Instant,
    head: tokio::sync::oneshot::Sender<Result<Head, NoHead>>,
    tx: tokio::sync::mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let socket = lease.socket().to_path_buf();
    let mut response =
        match tokio::time::timeout_at(deadline, Client::request(&socket, params, body)).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                pool.release(lease, Fate::Crashed(err.to_string()));
                let _ = head.send(Err(NoHead::BadGateway));
                return;
            }
            Err(_) => {
                pool.release(lease, Fate::TimedOut);
                let _ = head.send(Err(NoHead::Timeout));
                return;
            }
        };
    let mut report = None;
    let mut buffered = Vec::new();
    let mut head = Some(head);
    let mut client_gone = false;
    loop {
        let next = tokio::time::timeout_at(deadline, response.next()).await;
        let event = match next {
            Err(_) => {
                drop(response);
                pool.release(lease, Fate::TimedOut);
                if let Some(head) = head.take() {
                    let _ = head.send(Err(NoHead::Timeout));
                } else {
                    let _ = tx.try_send(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "request_timeout",
                    )));
                }
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
                if let Some(head) = head.take() {
                    // Output with no head at all: PHP always sends one, so the worker is broken.
                    let _ = head.send(Err(NoHead::BadGateway));
                }
                return;
            }
            Ok(Some(Err(err))) => {
                pool.release(
                    lease,
                    Fate::Crashed("the worker closed the connection mid-request".into()),
                );
                if let Some(head) = head.take() {
                    let _ = head.send(Err(NoHead::BadGateway));
                } else {
                    let _ = tx.try_send(Err(err));
                }
                return;
            }
            Ok(Some(Ok(event))) => event,
        };
        let bytes = match event {
            Event::Stderr(bytes) => {
                stderr(&bytes, &mut report);
                continue;
            }
            Event::End(_) => continue,
            Event::Stdout(bytes) => bytes,
        };
        let chunk = if let Some(sender) = head.take() {
            buffered.extend_from_slice(&bytes);
            let Some(end) = head_end(&buffered) else {
                head = Some(sender);
                continue;
            };
            let rest = buffered.split_off(end);
            let (status, headers) = parse_head(&buffered);
            if sender.send(Ok((status, headers, Vec::new()))).is_err() {
                client_gone = true;
            }
            if rest.is_empty() {
                continue;
            }
            Bytes::from(rest)
        } else {
            bytes
        };
        if client_gone {
            continue;
        }
        match tokio::time::timeout_at(deadline, tx.send(Ok(chunk))).await {
            Ok(Ok(())) => {}
            // The client went away: keep draining, so the script runs to its end.
            Ok(Err(_)) => client_gone = true,
            // The client stopped reading: the deadline applies as to any request.
            Err(_) => {
                drop(response);
                pool.release(lease, Fate::TimedOut);
                return;
            }
        }
    }
}

#[salvo::async_trait]
impl PhpEndpoint for PhpRequestHandler {
    async fn serve(&self, call: PhpCall, req: &mut Request, res: &mut Response) {
        let query = req.uri().query().map(str::to_string);
        let (script, script_name, path_info) =
            match self.target(&call.mount_prefix, &call.rel_path, query.as_deref()) {
                Target::Static(file) => return self.serve_static(&file, res).await,
                Target::Redirect(location) => {
                    res.status_code(StatusCode::MOVED_PERMANENTLY);
                    if let Ok(value) = HeaderValue::from_str(&location) {
                        res.headers_mut()
                            .insert(salvo::http::header::LOCATION, value);
                    }
                    return;
                }
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
        let (head_tx, head_rx) = tokio::sync::oneshot::channel();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, io::Error>>(4);
        let body = futures_util::stream::unfold(req.take_body(), |mut body| async move {
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
        tokio::spawn(exchange(
            self.pool.clone(),
            lease,
            params,
            Box::pin(body),
            deadline,
            head_tx,
            tx,
        ));
        match head_rx.await {
            Ok(Ok((status, headers, _))) => {
                res.status_code(status);
                for (name, value) in headers {
                    res.headers_mut().append(name, value);
                }
                res.stream(tokio_stream_from(rx));
            }
            Ok(Err(NoHead::Timeout)) => plain(res, StatusCode::GATEWAY_TIMEOUT, "gateway timeout"),
            Ok(Err(NoHead::BadGateway)) | Err(_) => {
                plain(res, StatusCode::BAD_GATEWAY, "bad gateway")
            }
        }
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
