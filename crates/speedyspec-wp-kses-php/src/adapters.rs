//! The kses ports over PHP: the allowlist globals, `esc_attr()`, `current_user_can()`, `wp_upload_dir()` and PHP
//! callables (Adapter).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_php::PhpError;
use speedyspec_php::ext_php_rs::ffi::{
    IS_ARRAY, IS_INDIRECT, IS_REFERENCE, IS_UNDEF, ext_php_rs_executor_globals, zend_array,
    zend_array_destroy, zend_hash_str_find, zval,
};
use speedyspec_php_ext::Value;
use speedyspec_wp_hooks::Callback;
use speedyspec_wp_hooks_php::php_callback;
use speedyspec_wp_kses::{AllowedGlobals, DefaultGlobals, UploadsLocation, ValueCallbacks};
use speedyspec_wp_kses_contracts::{
    AttributeEscaper, EntityNames, KsesError, Payload, UnfilteredHtmlPolicy, ValueCallback,
};

use crate::native::{bytes, truthy, value};

/// `GC_IMMUTABLE` and `GC_PERSISTENT` from `Zend/zend_types.h`: arrays PHP never counts or frees per request.
const NOT_COUNTED: u32 = (1 << 6) | (1 << 7);

/// Marks a failure raised because PHP is unwinding (a fatal error or `exit()`): nothing is thrown.
struct Unwinding;

/// A failed PHP call as a kses error that carries the thrown object (or the unwinding mark).
pub(crate) fn call_error(failure: speedyspec_php::CallFailure) -> KsesError {
    let message = match &failure.error {
        PhpError::Exception { message, .. } => message.clone(),
        other => other.to_string(),
    };
    let payload: Rc<dyn std::any::Any> = match (failure.exception, &failure.error) {
        (Some(exception), _) => Rc::new(exception),
        (None, PhpError::Fatal { .. } | PhpError::Exit(_)) => Rc::new(Unwinding),
        (None, _) => return KsesError::Callback(message),
    };
    KsesError::Foreign {
        message,
        payload: Payload(payload),
    }
}

/// Calls the PHP function `name`.
pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, KsesError> {
    speedyspec_php::call_catching(&Value::Str(name.to_string()), args).map_err(call_error)
}

/// The array a global holds, followed through references and indirect slots; `None` when the global is unset or not
/// an array, or no PHP is running. Runs no PHP code.
fn global_array(name: &str) -> Option<*mut zend_array> {
    if speedyspec_php::execution_context() == 0 {
        return None;
    }
    // SAFETY: PHP is running, so the executor globals and the symbol table are live; the lookup neither allocates
    // nor runs PHP code, and references and indirect slots point to live zvals.
    unsafe {
        let table = &raw mut (*ext_php_rs_executor_globals()).symbol_table;
        let mut zv: *const zval = zend_hash_str_find(table, name.as_ptr().cast(), name.len());
        if zv.is_null() {
            return None;
        }
        loop {
            match u32::from((*zv).u1.v.type_) {
                IS_REFERENCE => zv = &raw const (*(*zv).value.ref_).val,
                IS_INDIRECT => zv = (*zv).value.zv,
                IS_ARRAY => return Some((*zv).value.arr),
                _ => return None,
            }
        }
    }
}

/// `HASH_FLAG_PACKED` from `Zend/zend_hash.h`.
const HASH_FLAG_PACKED: u32 = 1 << 2;

/// Whether `arr` holds a PHP reference at any depth (up to `depth` levels). A write through such a reference changes
/// the array without separating it, so its contents cannot be cached between reads.
///
/// # Safety
///
/// `arr` is a live array; nothing else runs while it is read.
unsafe fn holds_reference(arr: *const zend_array, depth: u32) -> bool {
    // SAFETY: a live array stores `nNumUsed` zvals (packed) or buckets; holes are `IS_UNDEF`.
    unsafe {
        let used = (*arr).nNumUsed as usize;
        let packed = (*arr).u.flags & HASH_FLAG_PACKED != 0;
        for i in 0..used {
            let zv: *const zval = if packed {
                (*arr).__bindgen_anon_1.arPacked.add(i)
            } else {
                &raw const (*(*arr).__bindgen_anon_1.arData.add(i)).val
            };
            match u32::from((*zv).u1.v.type_) {
                IS_UNDEF => {}
                IS_REFERENCE => return true,
                IS_ARRAY if depth > 0 && holds_reference((*zv).value.arr, depth - 1) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}

/// Counts a reference to `arr`.
///
/// # Safety
///
/// `arr` is null or a live array.
unsafe fn hold(arr: *mut zend_array) {
    // SAFETY: a live array's header.
    unsafe {
        if !arr.is_null() && (*arr).gc.u.type_info & NOT_COUNTED == 0 {
            (*arr).gc.refcount += 1;
        }
    }
}

/// Gives back a reference [`hold`] counted; frees the array when it was the last one and `free` is set.
///
/// # Safety
///
/// `arr` is null or an array this crate counted, still live.
unsafe fn release(arr: *mut zend_array, free: bool) {
    // SAFETY: as stated; `zend_array_destroy` frees an array nothing else refers to.
    unsafe {
        if arr.is_null() || (*arr).gc.u.type_info & NOT_COUNTED != 0 {
            return;
        }
        if (*arr).gc.refcount > 1 {
            (*arr).gc.refcount -= 1;
        } else if free {
            zend_array_destroy(arr);
        }
    }
}

/// One allowlist global, watched for writes.
///
/// The watcher keeps a counted reference to the array it last read. PHP then has to separate (copy) the array before
/// any write through the global, nested writes included, so a write always shows as a different array, and the old
/// address cannot be reused while it is held. Reading the pointer runs no PHP code; the array is converted again
/// only after a write.
struct Watched {
    name: &'static str,
    held: Cell<*mut zend_array>,
    /// The held array contains PHP references, so it is read afresh every time.
    volatile: Cell<bool>,
    value: RefCell<Option<Value>>,
    names: RefCell<Option<Rc<EntityNames>>>,
}

impl Watched {
    const fn new(name: &'static str) -> Watched {
        Watched {
            name,
            held: Cell::new(std::ptr::null_mut()),
            volatile: Cell::new(false),
            value: RefCell::new(None),
            names: RefCell::new(None),
        }
    }

    /// Looks at the global again: true when it holds a different array than last time, or none (a global that is
    /// unset or not an array is never cached).
    fn refresh(&self) -> bool {
        let now = global_array(self.name).unwrap_or(std::ptr::null_mut());
        let held = self.held.get();
        if !now.is_null() && now == held && !self.volatile.get() {
            return false;
        }
        self.value.replace(None);
        self.names.replace(None);
        if now == held {
            return true;
        }
        // The new array is held before the old one is given back: freeing the old one can run PHP destructors, and a
        // destructor that runs kses must find this watcher already on the new array.
        // SAFETY: `now` is null or live; it is read before anything else runs.
        unsafe { hold(now) };
        self.volatile
            .set(!now.is_null() && unsafe { holds_reference(now, 3) });
        let old = self.held.replace(now);
        // SAFETY: `old` is null or an array this watcher counted in this request.
        unsafe { release(old, true) };
        true
    }

    fn value(&self) -> Value {
        self.refresh();
        if let Some(v) = &*self.value.borrow() {
            return v.clone();
        }
        let v = speedyspec_php::global(self.name).unwrap_or(Value::Null);
        if !self.held.get().is_null() && !self.volatile.get() {
            self.value.replace(Some(v.clone()));
        }
        v
    }

    fn names(&self) -> Rc<EntityNames> {
        self.refresh();
        if let Some(n) = &*self.names.borrow() {
            return n.clone();
        }
        let names = match speedyspec_php::global(self.name) {
            Some(Value::Array(items)) => Rc::new(EntityNames(
                items
                    .iter()
                    .filter_map(|(_, v)| match v {
                        Value::Str(s) => Some(s.as_bytes().into()),
                        Value::Bytes(b) => Some(b.clone().into_boxed_slice()),
                        _ => None,
                    })
                    .collect(),
            )),
            _ => Rc::new(EntityNames::default()),
        };
        if !self.held.get().is_null() && !self.volatile.get() {
            self.names.replace(Some(names.clone()));
        }
        names
    }

    /// Forgets the array without touching it (at the start of a request, the last one's memory is gone), or, at
    /// the end of a request, gives its reference back without freeing it (the request's memory goes as a whole).
    fn reset(&self, give_back: bool) {
        if give_back {
            // SAFETY: `held` is null or an array counted in this request, which is still open.
            unsafe { release(self.held.get(), false) };
        }
        self.held.set(std::ptr::null_mut());
        self.volatile.set(false);
        self.value.replace(None);
        self.names.replace(None);
    }
}

/// `$allowedposttags`, `$allowedtags`, `$allowedentitynames` and `$allowedxmlentitynames`, read live from PHP, and
/// `CUSTOM_TAGS`. While no PHP runs (Rust callers outside a request) it answers WordPress's defaults.
pub struct PhpAllowedGlobals {
    post: Watched,
    tags: Watched,
    entities: Watched,
    xml_entities: Watched,
    generation: Cell<u64>,
    custom_tags: Cell<Option<bool>>,
    defaults: DefaultGlobals,
}

impl Default for PhpAllowedGlobals {
    fn default() -> Self {
        PhpAllowedGlobals {
            post: Watched::new("allowedposttags"),
            tags: Watched::new("allowedtags"),
            entities: Watched::new("allowedentitynames"),
            xml_entities: Watched::new("allowedxmlentitynames"),
            generation: Cell::new(0),
            custom_tags: Cell::new(None),
            defaults: DefaultGlobals::default(),
        }
    }
}

impl PhpAllowedGlobals {
    fn watched(&self) -> [&Watched; 4] {
        [&self.post, &self.tags, &self.entities, &self.xml_entities]
    }

    /// A new request: nothing read in the last one is valid.
    pub fn request_start(&self) {
        self.watched().iter().for_each(|w| w.reset(false));
        self.custom_tags.set(None);
        self.bump();
    }

    /// The request is ending: the counted references go back.
    pub fn request_end(&self) {
        self.watched().iter().for_each(|w| w.reset(true));
        self.custom_tags.set(None);
        self.bump();
    }

    fn bump(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    fn running() -> bool {
        speedyspec_php::execution_context() != 0
    }
}

impl AllowedGlobals for PhpAllowedGlobals {
    fn post(&self) -> Value {
        if !Self::running() {
            return self.defaults.post();
        }
        self.post.value()
    }

    fn tags(&self) -> Value {
        if !Self::running() {
            return self.defaults.tags();
        }
        self.tags.value()
    }

    fn entity_names(&self) -> Rc<EntityNames> {
        if !Self::running() {
            return self.defaults.entity_names();
        }
        self.entities.names()
    }

    fn entity_names_value(&self) -> Value {
        if !Self::running() {
            return self.defaults.entity_names_value();
        }
        self.entities.value()
    }

    fn xml_entity_names(&self) -> Rc<EntityNames> {
        if !Self::running() {
            return self.defaults.xml_entity_names();
        }
        self.xml_entities.names()
    }

    fn custom_tags(&self) -> bool {
        if let Some(known) = self.custom_tags.get() {
            return known;
        }
        if !Self::running() {
            return false;
        }
        let name = Value::Str("CUSTOM_TAGS".into());
        if !call("defined", std::slice::from_ref(&name)).is_ok_and(|v| truthy(&v)) {
            return false;
        }
        let on = call("constant", &[name]).is_ok_and(|v| truthy(&v));
        // A defined constant keeps its value for the rest of the request.
        self.custom_tags.set(Some(on));
        on
    }

    fn generation(&self) -> u64 {
        if !Self::running() {
            return self.generation.get();
        }
        let mut changed = false;
        for w in self.watched() {
            changed |= w.refresh() || w.held.get().is_null() || w.volatile.get();
        }
        if changed {
            self.bump();
        }
        self.generation.get()
    }
}

/// `esc_attr()`, WordPress's own (its `attribute_escape` filter included).
pub struct PhpEscaper;

impl AttributeEscaper for PhpEscaper {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, KsesError> {
        Ok(bytes(&call("esc_attr", &[value(v)])?))
    }
}

/// `current_user_can( 'unfiltered_html' )`.
pub struct PhpPolicy;

impl UnfilteredHtmlPolicy for PhpPolicy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(truthy(&call(
            "current_user_can",
            &[Value::Str("unfiltered_html".into())],
        )?))
    }
}

/// `wp_upload_dir()['url']`.
pub struct PhpUploads;

impl UploadsLocation for PhpUploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        let dir = call("wp_upload_dir", &[])?;
        Ok(match &*dir.read() {
            Value::Array(items) => items
                .iter()
                .find(|(k, _)| matches!(k, speedyspec_php_ext::Key::Str(s) if s == "url"))
                .map(|(_, v)| bytes(v))
                .unwrap_or_default(),
            _ => Vec::new(),
        })
    }
}

/// PHP callables: a `value_callback` is called as `wp_kses_check_attr_val()` calls it (`call_user_func()`, so an
/// invalid callable fails with PHP's own error; the result read as PHP's truth), and a function kses registers by
/// name is the PHP function.
pub struct PhpValueCallbacks;

impl ValueCallbacks for PhpValueCallbacks {
    fn resolve(&self, callable: &Value) -> Option<ValueCallback> {
        let target = callable.clone();
        let callback: ValueCallback = Rc::new(move |v: &[u8]| {
            Ok(truthy(&call(
                "call_user_func",
                &[target.clone(), value(v)],
            )?))
        });
        Some(callback)
    }

    fn named_filter(&self, name: &str) -> Option<Callback> {
        Some(php_callback(Value::Str(name.to_string())))
    }
}
