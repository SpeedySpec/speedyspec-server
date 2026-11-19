//! `Php::serve`: web requests through the SAPI. Its own process: serving ends the open request, which the main
//! harness's cases share.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn eq(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected:?}, got {actual:?}"))
    }
}

/// A web request, as a server hands one to `Php::serve`.
#[derive(Default)]
struct FakeRequest {
    params: Vec<(String, String)>,
    body: Vec<u8>,
    read: usize,
    status: Option<u16>,
    headers: Vec<String>,
    output: Vec<u8>,
}

impl speedyspec_php::WebRequest for FakeRequest {
    fn params(&self) -> &[(String, String)] {
        &self.params
    }
    fn read_body(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.body.len() - self.read);
        buf[..n].copy_from_slice(&self.body[self.read..self.read + n]);
        self.read += n;
        n
    }
    fn send_headers(&mut self, status: u16, headers: &[String]) -> bool {
        self.status = Some(status);
        self.headers = headers.to_vec();
        true
    }
    fn write(&mut self, data: &[u8]) -> bool {
        self.output.extend_from_slice(data);
        true
    }
}

fn web_request(script: &Path) -> FakeRequest {
    let params = [
        ("REQUEST_METHOD", "PUT"),
        ("QUERY_STRING", "a=1"),
        ("REQUEST_URI", "/x?a=1"),
        ("CONTENT_TYPE", "text/plain"),
        ("CONTENT_LENGTH", "5"),
        ("HTTP_COOKIE", "c=cookie"),
        ("SCRIPT_FILENAME", script.to_str().unwrap()),
    ];
    FakeRequest {
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: b"hello".to_vec(),
        ..FakeRequest::default()
    }
}

/// `Php::serve`: `$_SERVER`, the query, the body, cookies, status and headers; an exception is a 500; each request
/// starts clean.
fn case_serve_web_request(php: &Php) -> Result<(), String> {
    let script = fixture("web.php");
    let mut first = web_request(&script);
    let served = php.serve(&mut first, &script);
    served.result.map_err(|e| e.to_string())?;
    eq(
        &String::from_utf8_lossy(&first.output),
        "PUT|1|hello|cookie|clean",
    )?;
    if first.status != Some(201) || !first.headers.iter().any(|h| h == "X-Test: one") {
        return Err(format!(
            "status {:?}, headers {:?}",
            first.status, first.headers
        ));
    }
    if !first.headers.iter().any(|h| {
        h.to_ascii_lowercase()
            .starts_with("content-type: text/html")
    }) {
        return Err(format!("no default content type in {:?}", first.headers));
    }
    let mut second = web_request(&script);
    php.serve(&mut second, &script)
        .result
        .map_err(|e| e.to_string())?;
    eq(
        &String::from_utf8_lossy(&second.output),
        "PUT|1|hello|cookie|clean",
    )?;
    let throws = fixture("web_throws.php");
    let mut failed = web_request(&throws);
    let served = php.serve(&mut failed, &throws);
    if served.result.is_ok() || failed.status != Some(500) {
        return Err(format!(
            "expected a 500, got {:?} and {:?}",
            served.result, failed.status
        ));
    }
    // Each request starts at 200: PHP itself never resets the status code.
    let plain = fixture("web_plain.php");
    let mut after = web_request(&plain);
    php.serve(&mut after, &plain)
        .result
        .map_err(|e| e.to_string())?;
    if after.status != Some(200) {
        return Err(format!(
            "the request after a 500 answered {:?}",
            after.status
        ));
    }
    // A request can be opened again afterwards.
    php.restart_request().map_err(|e| e.to_string())?;
    let value = php.eval("t", "return 1 + 1;").map_err(|e| e.to_string())?;
    eq(&format!("{value:?}"), "Int(2)")
}

fn main() -> ExitCode {
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .expect("PHP boots");
    match case_serve_web_request(&php) {
        Ok(()) => {
            println!("ok: serve_web_request");
            ExitCode::SUCCESS
        }
        Err(why) => {
            println!("FAILED: serve_web_request: {why}");
            ExitCode::FAILURE
        }
    }
}
