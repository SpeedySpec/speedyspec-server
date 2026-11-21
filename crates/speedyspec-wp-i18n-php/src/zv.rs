//! The few zval operations the hot path needs, straight on the engine's structures so a call converts nothing.

use std::cell::RefCell;
use std::os::raw::c_char;

use speedyspec_php::ext_php_rs::ffi::{
    IS_ARRAY, IS_INDIRECT, IS_LONG, IS_OBJECT, IS_REFERENCE, IS_STRING, IS_TYPE_REFCOUNTED,
    Z_TYPE_FLAGS_SHIFT, ext_php_rs_executor_globals, zend_class_entry, zend_function,
    zend_hash_find, zend_hash_str_find, zend_hash_str_find_ptr_lc, zend_object, zend_string, zval,
};
use speedyspec_php::ext_php_rs::types::Zval;
use speedyspec_php::ext_php_rs::zend::ExecuteData;

/// How many arguments the call passed.
pub fn num_args(ex: &ExecuteData) -> usize {
    // SAFETY: every field of the union is the same integer.
    unsafe { ex.This.u2.num_args as usize }
}

/// The `i`th argument's slot.
pub fn arg(ex: &ExecuteData, i: usize) -> Option<*mut zval> {
    if i >= num_args(ex) {
        return None;
    }
    // SAFETY: `i < num_args`, so the slot is inside the live frame.
    unsafe { ex.zend_call_arg(i) }.map(|z| z as *mut Zval as *mut zval)
}

fn type_of(zv: *const zval) -> u32 {
    // SAFETY: callers pass live zvals.
    u32::from(unsafe { (*zv).u1.v.type_ })
}

/// Follows `IS_INDIRECT` (a global's slot) and `IS_REFERENCE`.
fn deref(mut zv: *mut zval) -> Option<*mut zval> {
    loop {
        if zv.is_null() {
            return None;
        }
        match type_of(zv) {
            // SAFETY: the type says which union member is live.
            t if t == IS_INDIRECT => zv = unsafe { (*zv).value.zv },
            t if t == IS_REFERENCE => zv = unsafe { &raw mut (*(*zv).value.ref_).val },
            _ => return Some(zv),
        }
    }
}

/// The argument's string, when it is one (not converted otherwise).
pub fn string_arg(ex: &ExecuteData, i: usize) -> Option<*mut zend_string> {
    let zv = deref(arg(ex, i)?)?;
    // SAFETY: an IS_STRING zval's `str_` is its live string.
    (type_of(zv) == IS_STRING).then(|| unsafe { (*zv).value.str_ })
}

/// The argument's integer, when it is one.
pub fn long_arg(ex: &ExecuteData, i: usize) -> Option<i64> {
    let zv = deref(arg(ex, i)?)?;
    // SAFETY: an IS_LONG zval's `lval` is live.
    (type_of(zv) == IS_LONG).then(|| unsafe { (*zv).value.lval })
}

/// A zend string's bytes.
pub fn bytes<'a>(s: *const zend_string) -> &'a [u8] {
    // SAFETY: a live zend_string holds `len` bytes at `val`.
    unsafe { std::slice::from_raw_parts((*s).val.as_ptr().cast::<u8>(), (*s).len) }
}

/// `$GLOBALS['l10n'][$domain]` when it is an object.
pub fn l10n_entry(domain: *mut zend_string) -> Option<*mut zend_object> {
    // SAFETY: executor globals are live while a request runs; the lookups return live slots or null.
    unsafe {
        let eg = ext_php_rs_executor_globals();
        let l10n = deref(zend_hash_str_find(
            &raw const (*eg).symbol_table,
            c"l10n".as_ptr(),
            4,
        ))?;
        if type_of(l10n) != IS_ARRAY {
            return None;
        }
        let entry = deref(zend_hash_find((*l10n).value.arr, domain))?;
        (type_of(entry) == IS_OBJECT).then(|| (*entry).value.obj)
    }
}

/// `spl_object_id()` and class of an object.
pub fn object_parts(obj: *const zend_object) -> (u32, *const zend_class_entry) {
    // SAFETY: a live object.
    unsafe { ((*obj).handle, (*obj).ce) }
}

thread_local! {
    /// Per-request lookups: (request generation, lower-case name) → pointer.
    static LOOKUPS: RefCell<(u64, Vec<(&'static str, usize)>)> = const { RefCell::new((0, Vec::new())) };
}

fn cached(generation: u64, name: &'static str, find: impl FnOnce() -> usize) -> usize {
    LOOKUPS.with(|l| {
        let mut l = l.borrow_mut();
        if l.0 != generation {
            *l = (generation, Vec::new());
        }
        if let Some((_, p)) = l.1.iter().find(|(n, _)| *n == name) {
            return *p;
        }
        let p = find();
        if p != 0 {
            l.1.push((name, p));
        }
        p
    })
}

/// A class by lower-case name, looked up once per request (classes are declared per request).
pub fn class(generation: u64, lower: &'static str) -> *const zend_class_entry {
    cached(generation, lower, || {
        // SAFETY: the class table is live while a request runs.
        unsafe {
            let eg = ext_php_rs_executor_globals();
            zend_hash_str_find_ptr_lc(
                (*eg).class_table,
                lower.as_ptr().cast::<c_char>(),
                lower.len(),
            ) as usize
        }
    }) as *const zend_class_entry
}

/// A function by lower-case name, looked up once per request.
pub fn function(generation: u64, lower: &'static str) -> *mut zend_function {
    cached(generation, lower, || {
        // SAFETY: the function table is live while a request runs.
        unsafe {
            let eg = ext_php_rs_executor_globals();
            zend_hash_str_find_ptr_lc(
                (*eg).function_table,
                lower.as_ptr().cast::<c_char>(),
                lower.len(),
            ) as usize
        }
    }) as *mut zend_function
}

/// `ZVAL_COPY( ret, arg )`: the caller's own argument as the return value.
pub fn return_arg(ret: &mut Zval, ex: &ExecuteData, i: usize) {
    let Some(src) = arg(ex, i).and_then(deref) else {
        return;
    };
    // SAFETY: `ret` is the engine's return slot (IS_NULL, nothing to release); a refcounted value gains the
    // reference the copy holds.
    unsafe {
        let out = ret as *mut Zval as *mut zval;
        std::ptr::copy_nonoverlapping(src, out, 1);
        if (*src).u1.type_info & (IS_TYPE_REFCOUNTED << Z_TYPE_FLAGS_SHIFT) != 0 {
            (*(*src).value.counted).gc.refcount += 1;
        }
    }
}
