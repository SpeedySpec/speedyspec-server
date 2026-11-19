//! Web requests: what [`crate::Php::serve`] needs from the server, and the SAPI callbacks that route PHP's output,
//! headers, request body, cookies and `$_SERVER` to it while a web request runs. Outside one, the callbacks keep
//! the CLI behaviour (output to stdout or stderr, no headers, an empty `$_SERVER` beyond PHP's own entries).

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_char, c_int};

use ext_php_rs::ffi::{php_register_variable_safe, sapi_header_struct, sapi_headers_struct, zval};
use ext_php_rs::types::Zval;

/// One HTTP request, as PHP sees it.
pub trait WebRequest {
    /// The CGI/1.1 parameters, which become `$_SERVER` (`SCRIPT_FILENAME`, `REQUEST_METHOD`, `HTTP_*`, …).
    fn params(&self) -> &[(String, String)];
    /// Reads the request body; 0 at its end (or when it cannot be read).
    fn read_body(&mut self, buf: &mut [u8]) -> usize;
    /// The response status and header lines (`Name: value`), once, before any body.
    fn send_headers(&mut self, status: u16, headers: &[String]) -> bool;
    /// Response body bytes; false when the client is gone.
    fn write(&mut self, data: &[u8]) -> bool;
}

/// What PHP tells `SG(sapi_headers)` the response is, read back after a request.
#[derive(Clone, Debug, Default)]
pub struct WebState {
    pub headers_sent: bool,
}

thread_local! {
    static CURRENT: Cell<Option<*mut dyn WebRequest>> = const { Cell::new(None) };
    static COOKIES: RefCell<Option<CString>> = const { RefCell::new(None) };
    pub(crate) static STATE: RefCell<WebState> = RefCell::new(WebState::default());
}

/// Runs `f` with the current web request, if one is being served.
fn with_current<R>(f: impl FnOnce(&mut dyn WebRequest) -> R) -> Option<R> {
    let current = CURRENT.with(Cell::get)?;
    // SAFETY: `serve` sets the pointer for exactly the duration of the request, from a `&mut` it holds.
    Some(f(unsafe { &mut *current }))
}

pub(crate) fn active() -> bool {
    CURRENT.with(Cell::get).is_some()
}

/// Makes `request` the current web request until [`end`]. The caller keeps `request` borrowed until then.
pub(crate) fn begin(request: &mut dyn WebRequest) {
    let cookie = request
        .params()
        .iter()
        .find(|(k, _)| k == "HTTP_COOKIE")
        .map(|(_, v)| v.clone());
    COOKIES.with(|c| *c.borrow_mut() = cookie.and_then(|v| CString::new(v).ok()));
    STATE.with(|s| *s.borrow_mut() = WebState::default());
    // SAFETY: erases the borrow's lifetime; `end` clears the pointer before `serve` returns.
    let ptr: *mut dyn WebRequest =
        unsafe { std::mem::transmute::<&mut dyn WebRequest, &'static mut dyn WebRequest>(request) };
    CURRENT.with(|c| c.set(Some(ptr)));
}

pub(crate) fn end() {
    CURRENT.with(|c| c.set(None));
    COOKIES.with(|c| c.borrow_mut().take());
}

/// `ub_write` while a web request runs: the response body.
pub(crate) fn write(bytes: &[u8]) -> Option<bool> {
    with_current(|request| request.write(bytes))
}

/// `SAPI_HEADER_SENT_SUCCESSFULLY`.
const HEADER_SENT: c_int = 1;

pub(crate) extern "C" fn send_headers(headers: *mut sapi_headers_struct) -> c_int {
    if headers.is_null() || !active() {
        return HEADER_SENT;
    }
    // SAFETY: PHP passes its live `SG(sapi_headers)`; each list element stores a `sapi_header_struct` inline.
    let (status, lines) = unsafe {
        let status = match (*headers).http_response_code {
            code if (100..=999).contains(&code) => code as u16,
            _ => 200,
        };
        let mut lines = Vec::new();
        let mut element = (*headers).headers.head;
        while !element.is_null() {
            let header = (&raw const (*element).data).cast::<sapi_header_struct>();
            let text = (*header).header;
            if !text.is_null() {
                let bytes = std::slice::from_raw_parts(text.cast::<u8>(), (*header).header_len);
                lines.push(String::from_utf8_lossy(bytes).into_owned());
            }
            element = (*element).next;
        }
        (status, lines)
    };
    STATE.with(|s| s.borrow_mut().headers_sent = true);
    with_current(|request| request.send_headers(status, &lines));
    HEADER_SENT
}

pub(crate) extern "C" fn read_post(buffer: *mut c_char, length: usize) -> usize {
    if buffer.is_null() || length == 0 {
        return 0;
    }
    // SAFETY: PHP passes a writable buffer of `length` bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), length) };
    with_current(|request| request.read_body(buf)).unwrap_or(0)
}

pub(crate) extern "C" fn read_cookies() -> *mut c_char {
    COOKIES.with(|c| {
        c.borrow()
            .as_ref()
            .map_or(std::ptr::null_mut(), |s| s.as_ptr().cast_mut())
    })
}

pub(crate) extern "C" fn register_server_variables(vars: *mut Zval) {
    let vars: *mut zval = vars;
    with_current(|request| {
        for (name, value) in request.params() {
            let Ok(name) = CString::new(name.as_str()) else {
                continue;
            };
            // SAFETY: PHP passes the live `$_SERVER` array being built; the name is NUL-terminated and the value
            // has its length.
            unsafe {
                php_register_variable_safe(name.as_ptr(), value.as_ptr().cast(), value.len(), vars)
            };
        }
    });
}
