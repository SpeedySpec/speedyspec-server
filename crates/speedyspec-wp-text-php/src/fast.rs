//! A zero-copy path for the pure string functions, where copying the argument into Rust and the result back out
//! would cost more than the work: `convert_chars()`, `normalize_whitespace()`, `force_balance_tags()`,
//! `wp_strip_all_tags()`, `shortcode_unautop()` and `wp_rel_nofollow()` on text without links.
//!
//! A call is served here only when its arguments are plain strings (and simple flags) and the work runs no PHP: the
//! argument is read in place, and when nothing changes the caller's own string is returned with one more reference.
//! A call whose use case has been rebound in the container, and any other call, goes to the full native function (registered under `SpeedySpec\WpText\Slow\`) with the same
//! arguments, which handles arity errors, coercion, filters and diagnostics.

use std::borrow::Cow;
use std::os::raw::c_char;

use speedyspec_cqrs::Query;
use speedyspec_php::ext_php_rs::args::Arg;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::ext_php_rs::ffi::{
    IS_FALSE, IS_INDIRECT, IS_LONG, IS_REFERENCE, IS_STRING, IS_TRUE, IS_TYPE_REFCOUNTED, Z_TYPE_FLAGS_SHIFT,
    ext_php_rs_executor_globals, zend_call_known_function, zend_function, zend_hash_str_find_ptr_lc, zend_string, zval,
};
use speedyspec_php::ext_php_rs::flags::DataType;
use speedyspec_php::ext_php_rs::types::{ZendStr, Zval};
use speedyspec_php::ext_php_rs::zend::ExecuteData;
use speedyspec_wp_text::messages::{ConvertChars, ShortcodeUnautop, WpRelNofollow, WpStripAllTags};

/// Where the full native functions behind the fast ones are registered.
pub(crate) const SLOW: &str = "SpeedySpec\\WpText\\Slow\\";

/// The functions with a fast path.
pub(crate) const FAST: &[&str] =
    &["convert_chars", "normalize_whitespace", "force_balance_tags", "wp_strip_all_tags", "shortcode_unautop", "wp_rel_nofollow"];

fn num_args(ex: &ExecuteData) -> usize {
    // SAFETY: every member of the union is the call's argument count.
    unsafe { ex.This.u2.num_args as usize }
}

/// The `i`th argument's zval, references and indirect slots followed.
fn arg(ex: &ExecuteData, i: usize) -> Option<*mut zval> {
    if i >= num_args(ex) {
        return None;
    }
    // SAFETY: `i < num_args`, so the slot is in the live frame.
    let mut zv = unsafe { ex.zend_call_arg(i) }.map(|z| z as *mut Zval as *mut zval)?;
    loop {
        // SAFETY: a live zval; its type says which union member is set.
        let t = u32::from(unsafe { (*zv).u1.v.type_ });
        if t == IS_REFERENCE {
            zv = unsafe { &raw mut (*(*zv).value.ref_).val };
        } else if t == IS_INDIRECT {
            zv = unsafe { (*zv).value.zv };
        } else {
            return Some(zv);
        }
    }
}

fn type_of(zv: *const zval) -> u32 {
    // SAFETY: callers pass live zvals.
    u32::from(unsafe { (*zv).u1.v.type_ })
}

/// The `i`th argument's string, when it is one.
fn string_arg(ex: &ExecuteData, i: usize) -> Option<*mut zend_string> {
    let zv = arg(ex, i)?;
    // SAFETY: an IS_STRING zval's `str_` is its live string.
    (type_of(zv) == IS_STRING).then(|| unsafe { (*zv).value.str_ })
}

/// A zend string's bytes, borrowed for the call.
fn bytes<'a>(s: *const zend_string) -> &'a [u8] {
    // SAFETY: a live zend_string holds `len` bytes at `val`, alive while the frame holds it.
    unsafe { std::slice::from_raw_parts((*s).val.as_ptr().cast::<u8>(), (*s).len) }
}

/// The `i`th argument as a flag when it is `true`, `false` or an integer; `None` for anything else.
fn flag_arg(ex: &ExecuteData, i: usize) -> Option<bool> {
    let zv = arg(ex, i)?;
    match type_of(zv) {
        t if t == IS_TRUE => Some(true),
        t if t == IS_FALSE => Some(false),
        // SAFETY: an IS_LONG zval's `lval` is live.
        t if t == IS_LONG => Some(unsafe { (*zv).value.lval } != 0),
        _ => None,
    }
}

/// `ZVAL_COPY( ret, $args[0] )`: the caller's own first argument, one more reference.
fn return_first(ret: &mut Zval, ex: &ExecuteData) {
    let Some(src) = arg(ex, 0) else {
        return;
    };
    // SAFETY: `ret` is the engine's empty return slot; a refcounted value gains the reference the copy holds.
    unsafe {
        let out = ret as *mut Zval as *mut zval;
        std::ptr::copy_nonoverlapping(src, out, 1);
        if (*src).u1.type_info & (IS_TYPE_REFCOUNTED << Z_TYPE_FLAGS_SHIFT) != 0 {
            (*(*src).value.counted).gc.refcount += 1;
        }
    }
}

/// The result: the caller's own string when it is the whole input unchanged (a borrow can also be a slice of it, a
/// trimmed one say), otherwise a new string.
fn write(ret: &mut Zval, ex: &ExecuteData, input: &[u8], out: Cow<'_, [u8]>) {
    match out {
        Cow::Borrowed(b) if std::ptr::eq(b.as_ptr(), input.as_ptr()) && b.len() == input.len() => return_first(ret, ex),
        other => ret.set_zend_string(ZendStr::new(&other, false)),
    }
}

/// The work of one fast call, or `None` when the call must go to the full function.
fn serve_fast(name: &str, ex: &ExecuteData, ret: &mut Zval) -> Option<()> {
    // A use case whose handler has been rebound goes through the binding, on the full function.
    let key = match name {
        "convert_chars" => Some(ConvertChars::KEY),
        "wp_strip_all_tags" => Some(WpStripAllTags::KEY),
        "shortcode_unautop" => Some(ShortcodeUnautop::KEY),
        "wp_rel_nofollow" => Some(WpRelNofollow::KEY),
        _ => None,
    };
    if key.is_some_and(|k| !crate::native::fast_allowed(k)) {
        return None;
    }
    let n = num_args(ex);
    let text = bytes(string_arg(ex, 0)?);
    match name {
        "convert_chars" if n == 1 => write(ret, ex, text, speedyspec_wp_text::convert_chars(text)),
        "normalize_whitespace" if n == 1 => write(ret, ex, text, speedyspec_wp_text::normalize_whitespace(text)),
        "force_balance_tags" if n == 1 => write(ret, ex, text, speedyspec_wp_text::force_balance_tags(text)),
        "wp_strip_all_tags" if n <= 2 => {
            let remove_breaks = if n == 2 { flag_arg(ex, 1)? } else { false };
            write(ret, ex, text, speedyspec_wp_text::wp_strip_all_tags_core(text, remove_breaks))
        }
        "shortcode_unautop" if n == 1 => {
            // Reads `$shortcode_tags` without running PHP and fires no filter.
            let api = crate::native::shared_api()?;
            let out = api.shortcode_unautop(text).ok()?;
            write(ret, ex, text, out)
        }
        "wp_rel_nofollow" if n == 1 => {
            // Without an `<a ` there is nothing to rebuild: stripslashes, then addslashes, as WordPress does.
            let unslashed = speedyspec_wp_text_escape::stripslashes(text);
            let links = memchr::memchr_iter(b'<', &unslashed)
                .any(|at| unslashed.get(at + 1..at + 3).is_some_and(|w| (w[0] | 0x20) == b'a' && w[1] == b' '));
            if links {
                return None;
            }
            let reslashed = speedyspec_wp_text_escape::addslashes(&unslashed).into_owned();
            if reslashed == text {
                return_first(ret, ex);
            } else {
                ret.set_zend_string(ZendStr::new(&reslashed, false));
            }
        }
        _ => return None,
    }
    Some(())
}

/// The full native function for `name`.
fn slow_function(name: &str) -> *mut zend_function {
    let lower = format!("{SLOW}{name}").to_ascii_lowercase();
    // SAFETY: the executor globals and their function table are live while a request runs.
    unsafe {
        zend_hash_str_find_ptr_lc((*ext_php_rs_executor_globals()).function_table, lower.as_ptr().cast::<c_char>(), lower.len())
            .cast::<zend_function>()
    }
}

/// Calls the full native function with this call's own arguments.
fn serve_slow(name: &str, ex: &ExecuteData, ret: &mut Zval) {
    let function = slow_function(name);
    if function.is_null() {
        return;
    }
    let count = num_args(ex);
    // SAFETY: the frame's argument slots are contiguous from the first; the engine copies them into the callee's
    // frame. No Rust value that needs dropping is alive across the call, so a bailout from it unwinds nothing of ours.
    unsafe {
        let params = if count == 0 {
            std::ptr::null_mut()
        } else {
            ex.zend_call_arg(0).map_or(std::ptr::null_mut(), |z| z as *mut Zval as *mut zval)
        };
        zend_call_known_function(
            function,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ret as *mut Zval as *mut zval,
            count as u32,
            params,
            std::ptr::null_mut(),
        );
    }
}

fn serve(name: &'static str, ex: &mut ExecuteData, ret: &mut Zval) {
    // A panic must not unwind into the engine: the call then goes to the full function.
    let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve_fast(name, ex, ret))).ok().flatten();
    if served.is_none() {
        serve_slow(name, ex, ret);
    }
}

macro_rules! entry {
    ($fn:ident, $name:literal) => {
        extern "C" fn $fn(ex: &mut ExecuteData, ret: &mut Zval) {
            serve($name, ex, ret)
        }
    };
}

entry!(native_convert_chars, "convert_chars");
entry!(native_normalize_whitespace, "normalize_whitespace");
entry!(native_force_balance_tags, "force_balance_tags");
entry!(native_wp_strip_all_tags, "wp_strip_all_tags");
entry!(native_shortcode_unautop, "shortcode_unautop");
entry!(native_wp_rel_nofollow, "wp_rel_nofollow");

/// The fast functions, under WordPress's names.
pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    let handlers: [(&str, extern "C" fn(&mut ExecuteData, &mut Zval)); 6] = [
        ("convert_chars", native_convert_chars),
        ("normalize_whitespace", native_normalize_whitespace),
        ("force_balance_tags", native_force_balance_tags),
        ("wp_strip_all_tags", native_wp_strip_all_tags),
        ("shortcode_unautop", native_shortcode_unautop),
        ("wp_rel_nofollow", native_wp_rel_nofollow),
    ];
    handlers
        .into_iter()
        .map(|(name, handler)| {
            FunctionBuilder::new(name, handler)
                .not_required()
                .arg(Arg::new("args", DataType::Mixed).is_variadic())
                .returns(DataType::Mixed, false, true)
        })
        .collect()
}
