//! A PHP callable that Rust calls many times (a hook callback), through Zend's fast path.
//!
//! `call_user_func_array()` resolves its callable on every call (looks the function or method up, checks it) and
//! copies its arguments into a new array. A [`Callable`] resolves once, with `zend_is_callable_ex()`, keeps the
//! resulting `zend_fcall_info_cache`, and from then on calls `zend_call_function()` with it directly. The cache lives
//! for the request: functions, classes and the objects the callable holds do not outlive it, so a new request
//! resolves again.
//!
//! Arguments go to PHP as they are (a value passed through Rust unconverted is the very same zval), and the result
//! comes back unconverted ([`crate::convert::pass_through_owned`]). One call is one entry into PHP.
//!
//! What cannot be cached is called as before, through `call_user_func_array()` (or `call_user_func()` with no
//! arguments), so its errors and deprecations are PHP's own: a callable that does not resolve (yet), one that needs
//! a calling scope or is deprecated (`self::`, `parent::`, `static::`, `"A::parent::b"`), and one PHP calls through a
//! trampoline (`__call()`, `__callStatic()`), whose resolution is single-use.

use std::cell::Cell;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ptr;

use ext_php_rs::ffi::{
    IS_ARRAY, IS_OBJECT, IS_STRING, ZEND_ACC_CALL_VIA_TRAMPOLINE, ZEND_RESULT_CODE_SUCCESS,
    ext_php_rs_executor_globals, zend_array, zend_execute_data, zend_fcall_info,
    zend_fcall_info_cache, zend_hash_index_find, zend_is_callable_ex, zval,
};
use ext_php_rs::types::Zval;
use speedyspec_hooks::{Key, Value};

use crate::boundary::{
    self, CallError, ENGINE, discard_exception, drain, guard, release_later, settle_stray,
    take_thrown,
};
use crate::convert::{self, Unconvertible};
use crate::runtime::{CallFailure, PhpError, failure_of};

// Engine functions that ext-php-rs's bindings leave out; ZEND_API symbols of the linked libphp.
unsafe extern "C" {
    fn zend_call_function(fci: *mut zend_fcall_info, fcc: *mut zend_fcall_info_cache) -> i32;
    fn zend_release_fcall_info_cache(fcc: *mut zend_fcall_info_cache);
}

/// `IS_CALLABLE_SUPPRESS_DEPRECATIONS` (`Zend/zend_API.h`): the callables that would warn are not cached anyway.
const SUPPRESS_DEPRECATIONS: u32 = 1 << 1;

thread_local! {
    /// The request the engine runs now; a cache from an earlier one is stale.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// A new request: every cached resolution is stale.
pub(crate) fn new_request() {
    GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
}

fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

#[derive(Clone, Copy)]
enum Cached {
    /// Not resolved in this request yet.
    Unknown,
    Resolved {
        generation: u64,
        fcc: zend_fcall_info_cache,
    },
    /// Called through `call_user_func_array()` for the rest of the request.
    Uncacheable { generation: u64 },
}

/// A PHP callable held by Rust, called through Zend's fast path. See the module docs.
pub struct Callable {
    value: Value,
    cache: Cell<Cached>,
    /// The callable's shape allows caching at all.
    shape_ok: bool,
}

impl Callable {
    /// `value` is the callable as PHP gave it; best passed through unconverted (`Value::Opaque`), so calling it never
    /// writes it back to PHP.
    pub fn new(value: Value) -> Callable {
        let shape_ok = cacheable_shape(&value);
        Callable {
            value,
            cache: Cell::new(Cached::Unknown),
            shape_ok,
        }
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Calls the callable with `args`, as WordPress's `call_user_func_array()` would; the result is passed through.
    pub fn call(&self, args: &[Value]) -> Result<Value, CallFailure> {
        let plain = |error: PhpError| CallFailure {
            error,
            exception: None,
        };
        if !boundary::alive() {
            return Err(plain(PhpError::Boot("no PHP request is open".to_owned())));
        }
        let now = generation();
        let fallback = !self.shape_ok
            || matches!(self.cache.get(), Cached::Uncacheable { generation } if generation == now);
        if !fallback {
            match self.call_fast(args, now) {
                Fast::Called(result) => return result.map_err(failure_of),
                Fast::Unresolved => {}
            }
        }
        self.call_user_func(args)
    }

    /// As WordPress calls a callback, through `call_user_func()` / `call_user_func_array()`.
    fn call_user_func(&self, args: &[Value]) -> Result<Value, CallFailure> {
        if args.is_empty() {
            return crate::call_catching(
                &Value::Str("call_user_func".into()),
                std::slice::from_ref(&self.value),
            );
        }
        let list = args
            .iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v.clone()))
            .collect();
        crate::call_catching(
            &Value::Str("call_user_func_array".into()),
            &[self.value.clone(), Value::Array(list)],
        )
    }

    fn call_fast(&self, args: &[Value], now: u64) -> Fast {
        settle_stray();
        let handle = convert::handle_of(&self.value);
        // A callable Rust converted (not passed through) is written for the call.
        let mut written = ManuallyDrop::new(Zval::new());
        let written_ptr: *mut zval = &raw mut *written;
        let callable_ptr: *mut zval = match handle {
            Some(handle) => handle.as_ptr().cast_mut(),
            None => written_ptr,
        };
        let needs_write = handle.is_none();
        let value = &self.value;

        let mut params: Vec<ManuallyDrop<Zval>> = args
            .iter()
            .map(|_| ManuallyDrop::new(Zval::new()))
            .collect();
        let params_ptr: *mut zval = params.as_mut_ptr().cast();
        let count = u32::try_from(params.len()).unwrap_or(u32::MAX);
        let mut retval = ManuallyDrop::new(Zval::new());
        let retval_ptr: *mut zval = &raw mut *retval;
        let dummy_func = ENGINE.with(|e| e.dummy_func.get());
        let cache = &self.cache;

        let called = guard(|| -> Result<Option<bool>, Unconvertible> {
            // SAFETY: every pointer is to zval storage owned by the enclosing frame (`params` has `count` slots;
            // ManuallyDrop<Zval> is a transparent Zval) or to the live callable the handle holds. The dummy frame
            // lives in this frame for the duration of the call; a bailout resets EG(current_execute_data) itself.
            unsafe {
                if needs_write {
                    convert::write_value(written_ptr, value)?;
                }
                for (i, arg) in args.iter().enumerate() {
                    convert::write_value(params_ptr.add(i), arg)?;
                }
                let eg = ext_php_rs_executor_globals();
                let mut dummy = MaybeUninit::<zend_execute_data>::zeroed();
                let installed = (*eg).current_execute_data.is_null();
                if installed {
                    (*dummy.as_mut_ptr()).func = dummy_func;
                    (*eg).current_execute_data = dummy.as_mut_ptr();
                }
                let mut fcc = match cache.get() {
                    Cached::Resolved { generation, fcc } if generation == now => Some(fcc),
                    _ => resolve(callable_ptr, cache, now),
                };
                let result = fcc.as_mut().map(|fcc| {
                    let mut fci = zend_fcall_info {
                        size: std::mem::size_of::<zend_fcall_info>(),
                        // Left empty: the cache names the function, and zend_call_function releases a string it
                        // finds here (a borrowed view of the callable would lose a reference on every call).
                        function_name: Zval::new(),
                        retval: retval_ptr,
                        params: params_ptr,
                        object: fcc.object,
                        param_count: count,
                        named_params: ptr::null_mut(),
                    };
                    zend_call_function(&raw mut fci, fcc) == ZEND_RESULT_CODE_SUCCESS
                });
                if installed {
                    (*eg).current_execute_data = ptr::null_mut();
                }
                Ok(result)
            }
        });

        if needs_write {
            release_later(written);
        }
        for param in params {
            release_later(param);
        }
        let outcome = match called {
            Ok(Ok(Some(_))) => {
                // SAFETY: `retval` holds what the callable returned (or null); its reference moves to the value.
                Some(unsafe { convert::pass_through_owned(retval) })
            }
            _ => {
                release_later(retval);
                None
            }
        };
        // SAFETY: a plain read of the executor globals on the engine's thread.
        let clear_before = matches!(called, Ok(Ok(Some(true))))
            && unsafe { (*ext_php_rs_executor_globals()).exception.is_null() };
        drain();
        if clear_before {
            settle_stray();
        }
        match called {
            Err(interrupted) => {
                discard_exception();
                Fast::Called(Err(CallError::Interrupted(interrupted)))
            }
            Ok(Err(why)) => Fast::Called(Err(CallError::Convert(why.to_string()))),
            Ok(Ok(None)) => {
                // Not resolved: nothing ran. Anything resolving threw (an autoloader) is the fallback's to raise
                // again, from PHP's own call.
                discard_exception();
                Fast::Unresolved
            }
            Ok(Ok(Some(true))) => match take_thrown() {
                Some(thrown) => Fast::Called(Err(CallError::Thrown(thrown))),
                None => Fast::Called(Ok(outcome.unwrap_or(Value::Null))),
            },
            Ok(Ok(Some(false))) => match take_thrown() {
                Some(thrown) => Fast::Called(Err(CallError::Thrown(thrown))),
                None => Fast::Called(Err(CallError::Convert(
                    "PHP refused the call (the executor is inactive)".to_owned(),
                ))),
            },
        }
    }
}

enum Fast {
    Called(Result<Value, CallError>),
    /// The callable did not resolve to something cacheable; nothing ran.
    Unresolved,
}

/// Resolves `callable` and caches the result for this request; `None` when it cannot be cached (it is then called
/// as WordPress calls it).
///
/// # Safety
///
/// Inside a guard, on the engine's thread; `callable` is a live zval.
unsafe fn resolve(
    callable: *mut zval,
    cache: &Cell<Cached>,
    now: u64,
) -> Option<zend_fcall_info_cache> {
    // SAFETY: forwarded from the caller; a zeroed fcc is what zend_is_callable_ex fills in.
    unsafe {
        let mut fcc = MaybeUninit::<zend_fcall_info_cache>::zeroed().assume_init();
        let ok = zend_is_callable_ex(
            callable,
            ptr::null_mut(),
            SUPPRESS_DEPRECATIONS,
            ptr::null_mut(),
            &raw mut fcc,
            ptr::null_mut(),
        );
        if !ok || fcc.function_handler.is_null() {
            // Maybe defined later in the request: try again next time.
            return None;
        }
        let flags = (*fcc.function_handler).common.fn_flags;
        if flags & ZEND_ACC_CALL_VIA_TRAMPOLINE != 0 {
            zend_release_fcall_info_cache(&raw mut fcc);
            cache.set(Cached::Uncacheable { generation: now });
            return None;
        }
        cache.set(Cached::Resolved {
            generation: now,
            fcc,
        });
        Some(fcc)
    }
}

/// True for the callables whose resolution needs no calling scope and raises no deprecation: a function name,
/// `"Class::method"`, a closure or invokable object, `[$object, 'method']` and `['Class', 'method']`, with no
/// `self`, `parent` or `static` and no `::` in the method.
fn cacheable_shape(value: &Value) -> bool {
    fn plain_class(class: &[u8]) -> bool {
        !class.is_empty()
            && !["self", "parent", "static"]
                .iter()
                .any(|kw| class.eq_ignore_ascii_case(kw.as_bytes()))
    }
    fn plain_name(name: &[u8]) -> bool {
        match name.windows(2).position(|w| w == b"::") {
            None => !name.is_empty(),
            Some(at) => {
                let (class, method) = (&name[..at], &name[at + 2..]);
                plain_class(class) && !method.is_empty() && !method.windows(2).any(|w| w == b"::")
            }
        }
    }
    fn method_ok(method: &[u8]) -> bool {
        !method.is_empty() && !method.windows(2).any(|w| w == b"::")
    }
    match value {
        Value::Str(name) => plain_name(name.as_bytes()),
        Value::Bytes(name) => plain_name(name),
        Value::Array(entries) => {
            let at = |i: i64| {
                entries
                    .iter()
                    .find(|(k, _)| *k == Key::Int(i))
                    .map(|(_, v)| v)
            };
            entries.len() == 2
                && match (at(0), at(1)) {
                    (Some(Value::Str(class)), Some(Value::Str(method))) => {
                        plain_class(class.as_bytes()) && method_ok(method.as_bytes())
                    }
                    (Some(Value::Opaque(_)), Some(Value::Str(method))) => {
                        method_ok(method.as_bytes())
                    }
                    _ => false,
                }
        }
        Value::Opaque(_) => match convert::handle_of(value) {
            // SAFETY: the handle holds a live zval.
            Some(handle) => unsafe { zval_shape_ok(handle.as_ptr()) },
            None => false,
        },
        _ => false,
    }
}

/// [`cacheable_shape`] of a callable PHP passed through.
///
/// # Safety
///
/// `zv` is a live zval.
unsafe fn zval_shape_ok(zv: *const zval) -> bool {
    // SAFETY: the caller guarantees `zv` is live; reading runs no PHP code.
    unsafe {
        match u32::from(convert::type_of(zv)) {
            IS_OBJECT => true,
            IS_STRING | IS_ARRAY => match convert::to_value(zv) {
                Ok(value @ (Value::Str(_) | Value::Bytes(_))) => cacheable_shape(&value),
                Ok(Value::Array(_)) => {
                    let ht: *const zend_array = (*zv).value.arr;
                    if (*ht).nNumOfElements != 2 {
                        return false;
                    }
                    let (target, method) =
                        (zend_hash_index_find(ht, 0), zend_hash_index_find(ht, 1));
                    if target.is_null() || method.is_null() {
                        return false;
                    }
                    let (target, method) = (convert::deref(target), convert::deref(method));
                    let method = match convert::to_value(method) {
                        Ok(Value::Str(method)) => method,
                        _ => return false,
                    };
                    match u32::from(convert::type_of(target)) {
                        IS_OBJECT => !method.is_empty() && !method.contains("::"),
                        IS_STRING => match convert::to_value(target) {
                            Ok(class @ Value::Str(_)) => cacheable_shape(&Value::Array(vec![
                                (Key::Int(0), class),
                                (Key::Int(1), Value::Str(method)),
                            ])),
                            _ => false,
                        },
                        _ => false,
                    }
                }
                _ => false,
            },
            _ => false,
        }
    }
}
