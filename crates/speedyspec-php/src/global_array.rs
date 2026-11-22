//! Reading a global PHP array in place, without converting the whole array.
//!
//! An area that consults a global on every call (the Shortcode API reads `$shortcode_tags` per `do_shortcode()`)
//! needs its keys and one entry, not a copy of every value. These functions run no PHP code and allocate no PHP
//! memory.

use ext_php_rs::ffi::{
    Bucket, IS_ARRAY, IS_UNDEF, zend_array, zend_hash_index_find, zend_hash_str_find, zval,
};
use speedyspec_hooks::Value;

use crate::convert;

/// The live array `$GLOBALS[$name]` holds, references followed.
///
/// # Safety
///
/// A request is open, on the engine's thread; the pointer is valid until PHP code runs.
unsafe fn global_table(name: &str) -> Option<*const zend_array> {
    if !crate::boundary::alive() {
        return None;
    }
    // SAFETY: forwarded from the caller; the slot found is a live zval.
    unsafe {
        let slot = crate::runtime::global_slot(name)?;
        let value = convert::deref(slot);
        (u32::from(convert::type_of(value)) == IS_ARRAY).then(|| (*value).value.arr.cast_const())
    }
}

/// PHP's canonical integer key for `key` (`"7"` but not `"07"`), as array access converts it.
fn int_key(key: &[u8]) -> Option<i64> {
    let digits = key.strip_prefix(b"-").unwrap_or(key);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits[0] == b'0' && (digits.len() > 1 || key.len() != digits.len()) {
        return None;
    }
    std::str::from_utf8(key).ok()?.parse().ok()
}

/// `$GLOBALS[$name][$key]`'s slot, PHP's key conversion applied.
///
/// # Safety
///
/// As [`global_table`].
unsafe fn entry(name: &str, key: &[u8]) -> Option<*const zval> {
    // SAFETY: forwarded from the caller; both lookups only read the live table.
    unsafe {
        let table = global_table(name)?;
        let slot = match int_key(key) {
            Some(i) => zend_hash_index_find(table, i as u64),
            None => zend_hash_str_find(table, key.as_ptr().cast(), key.len()),
        };
        if slot.is_null() || u32::from(convert::type_of(convert::deref(slot))) == IS_UNDEF {
            return None;
        }
        Some(slot.cast_const())
    }
}

/// The keys of the array in `$GLOBALS[$name]`, in PHP's order, as bytes (integer keys in decimal); `None` when the
/// global is unset or not an array.
pub fn global_array_keys(name: &str) -> Option<Vec<Vec<u8>>> {
    // SAFETY: the table is read and nothing runs PHP code meanwhile. A packed array stores `nNumUsed` zvals in
    // `arPacked`, any other array `nNumUsed` buckets in `arData`; holes are skipped, as PHP's own iteration does.
    unsafe {
        let ht = global_table(name)?;
        let used = (*ht).nNumUsed as usize;
        let mut keys = Vec::with_capacity((*ht).nNumOfElements as usize);
        if (*ht).u.flags & convert::HASH_FLAG_PACKED != 0 {
            let packed = (*ht).__bindgen_anon_1.arPacked;
            for i in 0..used {
                if !convert::is_hole(packed.add(i)) {
                    keys.push(i.to_string().into_bytes());
                }
            }
        } else {
            let data: *const Bucket = (*ht).__bindgen_anon_1.arData;
            for i in 0..used {
                let bucket = data.add(i);
                if convert::is_hole(&raw const (*bucket).val) {
                    continue;
                }
                keys.push(if (*bucket).key.is_null() {
                    ((*bucket).h as i64).to_string().into_bytes()
                } else {
                    convert::str_bytes((*bucket).key).to_vec()
                });
            }
        }
        Some(keys)
    }
}

/// `array_key_exists( $key, $GLOBALS[$name] )`.
pub fn global_array_contains(name: &str, key: &[u8]) -> bool {
    // SAFETY: a lookup in the live table.
    unsafe { entry(name, key).is_some() }
}

/// `$GLOBALS[$name][$key]`, converted to a [`Value`]; `None` when absent. Only that entry is converted.
pub fn global_array_get(name: &str, key: &[u8]) -> Option<Value> {
    // SAFETY: the entry is live, and `to_value` runs no PHP code.
    unsafe { convert::to_value(entry(name, key)?).ok() }
}
