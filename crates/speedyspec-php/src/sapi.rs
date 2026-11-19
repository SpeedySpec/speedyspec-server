//! The SAPI's fixed INI entries and its output and log callbacks.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// INI settings the CLI needs regardless of any php.ini, which is never read.
/// These follow what PHP's own CLI SAPI forces, with two exceptions:
///
/// - PHP's diagnostics (errors, warnings, notices, deprecations) must never
///   reach stdout, which carries only command output. `display_errors=stderr`
///   is honoured only by SAPIs named cli, cgi, phpdbg or micro, so instead
///   nothing is displayed and everything is logged: with `error_log` unset,
///   PHP hands each message ("PHP Warning:  ... in f on line n") to the
///   SAPI's `log_message`, which writes it to stderr whatever
///   `redirect_output` says.
/// - `register_argc_argv=0`: outside the CLI SAPI PHP 8.5 would derive
///   `$argv` from the query string (deprecated), so `run_file` sets `$argv`
///   and `$argc` itself.
pub(crate) const INI_ENTRIES: &CStr = c"html_errors=0
display_errors=0
display_startup_errors=0
log_errors=1
register_argc_argv=0
implicit_flush=1
output_buffering=0
max_execution_time=0
max_input_time=-1
";

/// Where PHP's output goes: false = stdout, true = stderr. Read by the SAPI's
/// output callback, which has no other way to reach the `Php`.
pub(crate) static OUTPUT_TO_STDERR: AtomicBool = AtomicBool::new(false);

/// Sends PHP's output to stdout or stderr.
pub(crate) extern "C" fn ub_write(data: *const c_char, len: usize) -> usize {
    if len == 0 || data.is_null() {
        return 0;
    }
    // SAFETY: PHP passes a non-null buffer of `len` bytes that outlives the call.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
    if let Some(written) = crate::web::write(bytes) {
        return if written { len } else { 0 };
    }
    let written = if OUTPUT_TO_STDERR.load(Ordering::SeqCst) {
        std::io::stderr().lock().write_all(bytes)
    } else {
        std::io::stdout().lock().write_all(bytes)
    };
    match written {
        Ok(()) => len,
        Err(_) => 0,
    }
}

pub(crate) extern "C" fn flush(_server_context: *mut c_void) {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// Sends PHP's log messages (startup errors, `error_log()`) to stderr.
pub(crate) extern "C" fn log_message(message: *const c_char, _syslog_type: c_int) {
    if message.is_null() {
        return;
    }
    // SAFETY: PHP passes a NUL-terminated string that outlives the call.
    let message = unsafe { CStr::from_ptr(message) };
    // Never panic here (an extern "C" function): a failed write, such as
    // EPIPE on a closed stderr, is ignored.
    let mut stderr = std::io::stderr().lock();
    let _ = stderr
        .write_all(message.to_bytes())
        .and_then(|()| stderr.write_all(b"\n"))
        .and_then(|()| stderr.flush());
}
