//! Converts values between PHP (`zval`) and the hook contract's [`Value`].
//!
//! null, bool, int and float map directly. A PHP string becomes `Value::Str`
//! when it is valid UTF-8 and `Value::Bytes` otherwise. A PHP array becomes
//! `Value::Array` with its keys and order. Objects (closures included) and
//! resources become `Value::Opaque` holding a [`ZvalHandle`]: a counted
//! reference to the very same PHP value, so the object that comes out of a
//! hook is the object that went in. Converting back reverses each rule.
//!
//! [`pass_through`] reads a value without converting it: a string or an array
//! becomes `Value::Opaque` holding a [`ZvalHandle`] too, so a value Rust only
//! carries (a hook's argument, a filter's result) goes back to PHP as the
//! very same zval, with no copy either way. [`read_opaque`] converts such a
//! handle when Rust code reads it ([`Value::read`]).
//!
//! Reading a `zval` ([`to_value`]) never allocates PHP memory and never runs
//! PHP code. Writing one ([`write_value`]) allocates PHP memory. In this PHP
//! that does not bail out: an allocation that crosses `memory_limit`
//! succeeds and queues a delayed OOM, which PHP reports at its next check
//! (see `boundary::settle_delayed_oom`), and exhausting the hard limit or the
//! system's memory ends the process. It still runs inside
//! [`crate::boundary::guard`], and its frames own nothing with a destructor,
//! so that the rule "every entry into the engine is guarded" has no
//! exceptions.

use std::fmt;
use std::mem::ManuallyDrop;
use std::ptr;
use std::rc::Rc;

use ext_php_rs::ffi::{
    _zend_new_array, Bucket, IS_ARRAY, IS_ARRAY_EX, IS_DOUBLE, IS_FALSE, IS_INDIRECT, IS_LONG,
    IS_NULL, IS_OBJECT, IS_REFERENCE, IS_RESOURCE, IS_STRING, IS_STRING_EX, IS_TRUE,
    IS_TYPE_REFCOUNTED, IS_UNDEF, ext_php_rs_zend_string_init, zend_array, zend_hash_index_update,
    zend_hash_str_update, zend_object, zend_string, zend_ulong, zval,
};
use ext_php_rs::types::Zval;
use speedyspec_hooks::{Key, Value};

use crate::boundary;

/// `HASH_FLAG_PACKED` from `Zend/zend_hash.h`; bindgen does not export it.
pub(crate) const HASH_FLAG_PACKED: u32 = 1 << 2;
/// `GC_IMMUTABLE` (also `IS_STR_INTERNED`) from `Zend/zend_types.h`.
const GC_IMMUTABLE: u32 = 1 << 6;
/// Nesting deeper than this is refused rather than risking the Rust stack.
const MAX_DEPTH: usize = 512;

/// Why a PHP value could not become a [`Value`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConvertError {
    /// The array contains itself (through a reference).
    Recursive,
    /// Nested more than `MAX_DEPTH` levels.
    TooDeep,
    /// A string array key is not valid UTF-8; [`Key::Str`] holds a `String`.
    NonUtf8Key,
    /// A zval type that cannot occur in a user value (constant AST, ...).
    Unsupported,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Recursive => "the array contains itself",
            Self::TooDeep => "the value is nested too deeply",
            Self::NonUtf8Key => "an array key is not valid UTF-8",
            Self::Unsupported => "the value has an unsupported type",
        })
    }
}

/// Why a [`Value`] could not become a PHP value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unconvertible {
    /// A `Value::Opaque` that does not wrap a PHP value (a Rust callback's own
    /// opaque data); PHP has no way to hold it.
    ForeignOpaque,
    /// Nested more than `MAX_DEPTH` levels.
    TooDeep,
}

impl fmt::Display for Unconvertible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ForeignOpaque => "an opaque Rust value cannot be passed to PHP",
            Self::TooDeep => "the value is nested too deeply",
        })
    }
}

/// One counted reference to a PHP value, held by Rust.
///
/// Creating a handle adds a reference; dropping it hands the reference to
/// [`boundary::release_later`], which releases it at the next point where PHP
/// code may safely run (releasing can run a destructor or the cycle
/// collector), or never, if the engine has already shut down and freed it.
pub(crate) struct ZvalHandle {
    zv: ManuallyDrop<Zval>,
}

impl ZvalHandle {
    /// Takes a new reference to the value `src` points at.
    ///
    /// # Safety
    ///
    /// `src` points to a live, initialised zval that is not a reference or
    /// indirect slot (callers dereference first), on the engine's thread.
    pub(crate) unsafe fn copy_of(src: *const zval) -> Self {
        // SAFETY: the caller guarantees `src` is live. The bitwise copy plus
        // the added reference below is exactly ZVAL_COPY.
        let zv = unsafe { ptr::read(src.cast::<ManuallyDrop<Zval>>()) };
        // SAFETY: `zv` is a valid zval copied from a live one.
        unsafe { add_ref(&raw const *zv) };
        Self { zv }
    }

    /// Wraps an object reference the caller already owns (for example one
    /// taken out of `EG(exception)`); no reference is added.
    ///
    /// # Safety
    ///
    /// `obj` is a live object and the caller owns one reference to it, which
    /// moves into the handle.
    pub(crate) unsafe fn adopt_object(obj: *mut zend_object) -> Self {
        let mut zv = ManuallyDrop::new(Zval::new());
        zv.value.obj = obj;
        zv.u1.type_info = ext_php_rs::ffi::IS_OBJECT_EX;
        Self { zv }
    }

    pub(crate) fn as_ptr(&self) -> *const zval {
        &raw const *self.zv
    }

    /// Wraps a value the caller owns (one reference, moved into the handle).
    ///
    /// # Safety
    ///
    /// `zv` is a live, initialised zval, not a reference or indirect slot,
    /// whose reference the caller owns and gives up.
    pub(crate) unsafe fn adopt(zv: ManuallyDrop<Zval>) -> Self {
        Self { zv }
    }

    /// The object, if this handle holds one.
    pub(crate) fn object(&self) -> Option<*mut zend_object> {
        // SAFETY: the handle always holds an initialised zval.
        unsafe { (type_of(self.as_ptr()) == IS_OBJECT as u8).then(|| self.zv.value.obj) }
    }

    /// Gives the held object reference to the caller, who becomes responsible
    /// for releasing it. `None` (and the handle is dropped normally) if the
    /// handle does not hold an object.
    pub(crate) fn into_object(self) -> Option<*mut zend_object> {
        let obj = self.object()?;
        std::mem::forget(self);
        Some(obj)
    }
}

impl Drop for ZvalHandle {
    fn drop(&mut self) {
        // SAFETY: moving the zval out of a handle that is being dropped; it is
        // never read again.
        let zv = unsafe { ManuallyDrop::take(&mut self.zv) };
        boundary::release_later(ManuallyDrop::new(zv));
    }
}

/// Reads a PHP value for Rust to carry, not to look into: null, bool, int
/// and float are converted (they are copied anyway); a string, an array, an
/// object or a resource becomes a `Value::Opaque` holding one reference to
/// the very same zval. Nothing is copied, and the value written back to PHP
/// is the one that came in.
///
/// # Safety
///
/// `zv` points to a live zval, on the engine's thread.
pub(crate) unsafe fn pass_through(zv: *const zval) -> Value {
    // SAFETY: the caller guarantees `zv` is live; each branch reads the union
    // member that matches the zval's type.
    unsafe {
        let zv = deref(zv);
        match u32::from(type_of(zv)) {
            IS_LONG => Value::Int((*zv).value.lval),
            IS_DOUBLE => Value::Float((*zv).value.dval),
            IS_FALSE => Value::Bool(false),
            IS_TRUE => Value::Bool(true),
            IS_STRING | IS_ARRAY | IS_OBJECT | IS_RESOURCE => {
                Value::Opaque(Rc::new(ZvalHandle::copy_of(zv)))
            }
            _ => Value::Null,
        }
    }
}

/// Like [`pass_through`], for a value the caller owns (a call's result): its
/// reference moves into the handle.
///
/// # Safety
///
/// As [`ZvalHandle::adopt`].
pub(crate) unsafe fn pass_through_owned(zv: ManuallyDrop<Zval>) -> Value {
    // SAFETY: forwarded from the caller; a scalar owns no reference.
    unsafe {
        match u32::from(type_of(&raw const *zv)) {
            IS_STRING | IS_ARRAY | IS_OBJECT | IS_RESOURCE => {
                Value::Opaque(Rc::new(ZvalHandle::adopt(zv)))
            }
            IS_REFERENCE => {
                let value = pass_through(&raw const *zv);
                boundary::release_later(zv);
                value
            }
            _ => pass_through(&raw const *zv),
        }
    }
}

/// True when `any` is a carried string or array, which [`read_opaque`]
/// converts; objects and resources stay opaque.
pub(crate) fn readable_opaque(any: &dyn std::any::Any) -> bool {
    any.downcast_ref::<ZvalHandle>().is_some_and(|handle| {
        // SAFETY: the handle holds an initialised zval.
        let ty = u32::from(unsafe { type_of(handle.as_ptr()) });
        ty == IS_STRING || ty == IS_ARRAY
    })
}

/// The value a carried string or array holds, converted; `None` for anything
/// else (an object stays opaque, as [`to_value`] leaves it).
pub(crate) fn read_opaque(any: &dyn std::any::Any) -> Option<Value> {
    if !readable_opaque(any) {
        return None;
    }
    let handle = any.downcast_ref::<ZvalHandle>()?;
    // SAFETY: the handle keeps the value alive; reading runs no PHP code.
    unsafe { to_value(handle.as_ptr()).ok() }
}

/// The hash of a string zval (`ZSTR_H`, computed and cached in the string if
/// PHP had not yet). `None` for anything else.
///
/// # Safety
///
/// `zv` points to a live zval, on the engine's thread.
pub(crate) unsafe fn string_hash(zv: *const zval) -> Option<u64> {
    // SAFETY: the caller guarantees `zv` is live; a string zval's `str_`
    // points to its live zend string. Hashing runs no PHP code and does not
    // allocate: it fills in the string's own `h`, as PHP itself does on first
    // use as a key.
    unsafe {
        let zv = deref(zv);
        if u32::from(type_of(zv)) != IS_STRING {
            return None;
        }
        let s = (*zv).value.str_;
        let h = (*s).h;
        Some(if h != 0 { h } else { zend_string_hash_func(s) })
    }
}

// `ZEND_API zend_ulong ZEND_FASTCALL zend_string_hash_func(zend_string *str)`,
// a symbol of the linked libphp. ZEND_FASTCALL is a plain C call on x86_64
// and aarch64.
unsafe extern "C" {
    fn zend_string_hash_func(s: *mut zend_string) -> zend_ulong;
}

/// The handle inside a `Value::Opaque`, if it wraps a PHP value.
pub(crate) fn handle_of(value: &Value) -> Option<&ZvalHandle> {
    match value {
        Value::Opaque(any) => any.downcast_ref::<ZvalHandle>(),
        _ => None,
    }
}

/// `Z_TYPE_P`.
///
/// # Safety
///
/// `zv` points to a live zval.
pub(crate) unsafe fn type_of(zv: *const zval) -> u8 {
    // SAFETY: every zval carries its type in `u1.v.type_`.
    unsafe { (*zv).u1.v.type_ }
}

/// `Z_TRY_ADDREF_P`.
///
/// # Safety
///
/// `zv` points to a live zval.
pub(crate) unsafe fn add_ref(zv: *const zval) {
    // SAFETY: a refcounted zval's `value.counted` points to its live header.
    unsafe {
        if u32::from((*zv).u1.v.type_flags) & IS_TYPE_REFCOUNTED != 0 {
            (*(*zv).value.counted).gc.refcount += 1;
        }
    }
}

/// Follows references and indirect slots to the value itself (`ZVAL_DEREF`
/// plus `Z_INDIRECT`).
///
/// # Safety
///
/// `zv` points to a live zval.
pub(crate) unsafe fn deref(mut zv: *const zval) -> *const zval {
    // SAFETY: a reference's `ref_` and an indirect's `zv` point to live zvals.
    unsafe {
        loop {
            match u32::from(type_of(zv)) {
                IS_REFERENCE => zv = &raw const (*(*zv).value.ref_).val,
                IS_INDIRECT => zv = (*zv).value.zv,
                _ => return zv,
            }
        }
    }
}

/// The bytes of a zend string.
///
/// # Safety
///
/// `s` points to a live zend string, which must outlive the returned slice.
pub(crate) unsafe fn str_bytes<'a>(s: *const zend_string) -> &'a [u8] {
    // SAFETY: a zend string stores `len` bytes inline starting at `val`.
    unsafe { std::slice::from_raw_parts((&raw const (*s).val).cast::<u8>(), (*s).len) }
}

/// Reads a PHP value as a [`Value`]. Objects and resources gain a reference
/// held by the returned `Opaque`. Runs no PHP code and allocates no PHP
/// memory, so it is safe outside the guard.
///
/// # Safety
///
/// `zv` points to a live zval, on the engine's thread.
pub(crate) unsafe fn to_value(zv: *const zval) -> Result<Value, ConvertError> {
    let mut arrays = Vec::new();
    // SAFETY: forwarded from the caller.
    unsafe { read(zv, &mut arrays) }
}

/// # Safety
///
/// As [`to_value`]. `arrays` holds the arrays being read on this path, to
/// detect an array that contains itself.
unsafe fn read(
    zv: *const zval,
    arrays: &mut Vec<*const zend_array>,
) -> Result<Value, ConvertError> {
    // SAFETY: the caller guarantees `zv` is live; each branch reads the union
    // member that matches the zval's type.
    unsafe {
        let zv = deref(zv);
        Ok(match u32::from(type_of(zv)) {
            IS_UNDEF | IS_NULL => Value::Null,
            IS_FALSE => Value::Bool(false),
            IS_TRUE => Value::Bool(true),
            IS_LONG => Value::Int((*zv).value.lval),
            IS_DOUBLE => Value::Float((*zv).value.dval),
            IS_STRING => {
                let bytes = str_bytes((*zv).value.str_);
                match std::str::from_utf8(bytes) {
                    Ok(s) => Value::Str(s.to_owned()),
                    Err(_) => Value::Bytes(bytes.to_vec()),
                }
            }
            IS_ARRAY => read_array((*zv).value.arr, arrays)?,
            IS_OBJECT | IS_RESOURCE => Value::Opaque(Rc::new(ZvalHandle::copy_of(zv))),
            _ => return Err(ConvertError::Unsupported),
        })
    }
}

/// # Safety
///
/// `ht` points to a live array; see [`read`].
unsafe fn read_array(
    ht: *const zend_array,
    arrays: &mut Vec<*const zend_array>,
) -> Result<Value, ConvertError> {
    if arrays.contains(&ht) {
        return Err(ConvertError::Recursive);
    }
    if arrays.len() >= MAX_DEPTH {
        return Err(ConvertError::TooDeep);
    }
    arrays.push(ht);
    // SAFETY: `ht` is live. A packed array stores `nNumUsed` zvals in
    // `arPacked`, any other array `nNumUsed` buckets in `arData`; holes are
    // skipped, as PHP's own iteration does.
    let items = unsafe {
        let used = (*ht).nNumUsed as usize;
        let mut items = Vec::with_capacity((*ht).nNumOfElements as usize);
        if (*ht).u.flags & HASH_FLAG_PACKED != 0 {
            let packed = (*ht).__bindgen_anon_1.arPacked;
            for i in 0..used {
                let slot = packed.add(i);
                if is_hole(slot) {
                    continue;
                }
                let key = Key::Int(i64::try_from(i).unwrap_or(i64::MAX));
                items.push((key, read(slot, arrays)?));
            }
        } else {
            let data: *const Bucket = (*ht).__bindgen_anon_1.arData;
            for i in 0..used {
                let bucket = data.add(i);
                let slot = &raw const (*bucket).val;
                if is_hole(slot) {
                    continue;
                }
                let key = if (*bucket).key.is_null() {
                    // Integer keys are stored as zend_ulong; PHP reads them back signed.
                    Key::Int((*bucket).h as i64)
                } else {
                    let bytes = str_bytes((*bucket).key);
                    let s = std::str::from_utf8(bytes).map_err(|_| ConvertError::NonUtf8Key)?;
                    Key::Str(s.to_owned())
                };
                items.push((key, read(slot, arrays)?));
            }
        }
        items
    };
    arrays.pop();
    Ok(Value::Array(items))
}

/// The value slots of an array, in order, holes skipped (PHP's `foreach`).
///
/// # Safety
///
/// `ht` points to a live array, which must outlive the returned pointers.
pub(crate) unsafe fn array_values(ht: *const zend_array) -> Vec<*const zval> {
    // SAFETY: as in `read_array`.
    unsafe {
        let used = (*ht).nNumUsed as usize;
        let mut slots = Vec::with_capacity((*ht).nNumOfElements as usize);
        if (*ht).u.flags & HASH_FLAG_PACKED != 0 {
            let packed = (*ht).__bindgen_anon_1.arPacked;
            for i in 0..used {
                let slot = packed.add(i).cast_const();
                if !is_hole(slot) {
                    slots.push(slot);
                }
            }
        } else {
            let data: *const Bucket = (*ht).__bindgen_anon_1.arData;
            for i in 0..used {
                let slot = &raw const (*data.add(i)).val;
                if !is_hole(slot) {
                    slots.push(slot);
                }
            }
        }
        slots
    }
}

/// `array_pop()` on `ht`: deletes its last element and, when that was the
/// last integer key, lowers the next free index, as `array_pop()` does.
///
/// # Safety
///
/// `ht` is a live array only the caller holds; inside a guard (the deleted
/// value is released).
pub(crate) unsafe fn pop_last(ht: *mut zend_array) {
    // SAFETY: as in `read_array`; the key found is deleted through the hash API.
    unsafe {
        let used = (*ht).nNumUsed as usize;
        if (*ht).u.flags & HASH_FLAG_PACKED != 0 {
            let packed = (*ht).__bindgen_anon_1.arPacked;
            let Some(i) = (0..used).rev().find(|&i| !is_hole(packed.add(i))) else {
                return;
            };
            ext_php_rs::ffi::zend_hash_index_del(ht, i as zend_ulong);
            if i as i64 == (*ht).nNextFreeElement - 1 {
                (*ht).nNextFreeElement -= 1;
            }
        } else {
            let data: *mut Bucket = (*ht).__bindgen_anon_1.arData;
            let Some(i) = (0..used)
                .rev()
                .find(|&i| !is_hole(&raw const (*data.add(i)).val))
            else {
                return;
            };
            let bucket = data.add(i);
            if (*bucket).key.is_null() {
                let h = (*bucket).h;
                ext_php_rs::ffi::zend_hash_index_del(ht, h);
                if h as i64 == (*ht).nNextFreeElement - 1 {
                    (*ht).nNextFreeElement -= 1;
                }
            } else {
                ext_php_rs::ffi::zend_hash_del(ht, (*bucket).key);
            }
        }
        // array_pop() leaves the internal pointer at the start.
        (*ht).nInternalPointer = 0;
    }
}

/// True for a deleted slot, or an indirect slot (a declared property or a
/// global) whose target is unset, such as an uninitialised typed property.
///
/// # Safety
///
/// `slot` points to a live hash table slot.
pub(crate) unsafe fn is_hole(slot: *const zval) -> bool {
    // SAFETY: an indirect slot's `zv` points to a live zval.
    unsafe {
        let ty = u32::from(type_of(slot));
        ty == IS_UNDEF || (ty == IS_INDIRECT && u32::from(type_of((*slot).value.zv)) == IS_UNDEF)
    }
}

/// Writes `value` into `out` as a PHP value, overwriting `out` without
/// releasing it. On error `out` may hold a partly built array, which the
/// caller still owns and must release.
///
/// Allocates PHP memory, so call it only inside [`crate::boundary::guard`]
/// (see the module docs for why allocation does not bail out here). It owns
/// nothing with a destructor in any frame, so even a bailout would leak only
/// PHP memory, which the engine reclaims.
///
/// # Safety
///
/// `out` points to writable zval storage whose current contents the caller
/// does not need released, on the engine's thread with a request open.
pub(crate) unsafe fn write_value(out: *mut zval, value: &Value) -> Result<(), Unconvertible> {
    // SAFETY: forwarded from the caller.
    unsafe { write(out, value, 0) }
}

/// Writes `value` into `out` when that needs nothing from the engine: a
/// scalar, or a value passed through (a copy of the zval and one more
/// reference). Allocates nothing and runs no PHP code, so it needs no guard.
/// False, and `out` untouched, for anything else.
///
/// # Safety
///
/// As [`write_value`].
pub(crate) unsafe fn write_without_engine(out: *mut zval, value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {
            // SAFETY: forwarded from the caller; scalars allocate nothing.
            unsafe { write(out, value, 0) }.is_ok()
        }
        Value::Opaque(_) => match handle_of(value) {
            Some(handle) => {
                // SAFETY: `out` is writable; the handle's zval is live, and
                // the copy takes its own reference (ZVAL_COPY).
                unsafe {
                    ptr::copy_nonoverlapping(handle.as_ptr(), out, 1);
                    add_ref(out);
                }
                true
            }
            None => false,
        },
        _ => false,
    }
}

/// # Safety
///
/// As [`write_value`].
unsafe fn write(out: *mut zval, value: &Value, depth: usize) -> Result<(), Unconvertible> {
    if depth >= MAX_DEPTH {
        return Err(Unconvertible::TooDeep);
    }
    // SAFETY: `out` is writable; each branch sets the value and then the
    // type that describes it, as the ZVAL_* macros do.
    unsafe {
        match value {
            Value::Null => (*out).u1.type_info = IS_NULL,
            Value::Bool(b) => (*out).u1.type_info = if *b { IS_TRUE } else { IS_FALSE },
            Value::Int(i) => {
                (*out).value.lval = *i;
                (*out).u1.type_info = IS_LONG;
            }
            Value::Float(f) => {
                (*out).value.dval = *f;
                (*out).u1.type_info = IS_DOUBLE;
            }
            Value::Str(s) => write_string(out, s.as_bytes()),
            Value::Bytes(b) => write_string(out, b),
            Value::Array(items) => {
                let size = u32::try_from(items.len()).unwrap_or(u32::MAX);
                let ht = _zend_new_array(size);
                (*out).value.arr = ht;
                (*out).u1.type_info = IS_ARRAY_EX;
                for (key, item) in items {
                    // Insert a null first, then fill the slot in place, so a
                    // failure part-way leaves a valid array behind.
                    let mut null = ManuallyDrop::new(Zval::new());
                    let tmp: *mut zval = &raw mut *null;
                    let slot = match key {
                        Key::Int(i) => zend_hash_index_update(ht, *i as zend_ulong, tmp),
                        Key::Str(s) => match canonical_int_key(s) {
                            Some(i) => zend_hash_index_update(ht, i as zend_ulong, tmp),
                            None => zend_hash_str_update(ht, s.as_ptr().cast(), s.len(), tmp),
                        },
                    };
                    write(slot, item, depth + 1)?;
                }
            }
            Value::Opaque(_) => {
                let handle = handle_of(value).ok_or(Unconvertible::ForeignOpaque)?;
                ptr::copy_nonoverlapping(handle.as_ptr(), out, 1);
                add_ref(out);
            }
        }
    }
    Ok(())
}

/// # Safety
///
/// `out` is writable zval storage; inside the guard.
unsafe fn write_string(out: *mut zval, bytes: &[u8]) {
    // SAFETY: ext-php-rs's wrapper copies `bytes` into a new request string,
    // or returns an interned one for lengths 0 and 1, which must not be
    // marked refcounted (ZVAL_STR).
    unsafe {
        let s = ext_php_rs_zend_string_init(bytes.as_ptr().cast(), bytes.len(), false);
        (*out).value.str_ = s;
        (*out).u1.type_info = if (*s).gc.u.type_info & GC_IMMUTABLE != 0 {
            IS_STRING
        } else {
            IS_STRING_EX
        };
    }
}

/// The integer a string key stands for in PHP (`ZEND_HANDLE_NUMERIC_STR`):
/// `"5"` and `"-3"` are integer keys, `"05"`, `"-0"`, `"+1"` and `"1 "` are
/// not. PHP arrays never hold a canonical integer as a string key.
fn canonical_int_key(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && s != "-0";
    if canonical { s.parse().ok() } else { None }
}

#[cfg(test)]
mod tests {
    use super::canonical_int_key;

    #[test]
    fn canonical_int_keys_follow_php() {
        assert_eq!(canonical_int_key("5"), Some(5));
        assert_eq!(canonical_int_key("0"), Some(0));
        assert_eq!(canonical_int_key("-3"), Some(-3));
        for s in ["", "-", "05", "-0", "+1", "1 ", "a", "9223372036854775808"] {
            assert_eq!(canonical_int_key(s), None, "{s:?}");
        }
    }
}
