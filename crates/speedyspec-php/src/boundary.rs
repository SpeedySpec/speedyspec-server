//! The boundary between Rust and PHP: every entry into PHP, every native
//! function PHP calls, and the per-context state that keeps PHP's error
//! handling on PHP's side of it.
//!
//! # Bailouts, exceptions and `exit()`
//!
//! A PHP fatal error is a bailout: a `longjmp` to the innermost `zend_try`.
//! It must never cross a Rust frame that owns something with a destructor.
//! The rule this module enforces (see also `docs/php-embed.md`):
//!
//! 1. Every entry into PHP goes through [`guard`]: a `zend_try` around a
//!    closure that captures only references and raw pointers, so the only
//!    frames a bailout can skip own nothing. Whatever the closure works on
//!    is owned by the caller, outside, and is dropped normally afterwards.
//! 2. Native functions called by PHP go through [`native_entry`]. If a guard
//!    inside one catches a bailout, it is recorded instead of continuing; the
//!    native function returns normally through its Rust frames, and only
//!    then, in a frame that owns nothing, [`native_entry`] re-raises it to
//!    the PHP code below. While a re-raise is pending no PHP code runs.
//! 3. A Rust panic in a native function cannot unwind through PHP's C
//!    frames. It is caught there, parked, and carried through PHP as a
//!    bailout; the next guard below resumes it in Rust. A panic is never
//!    reported as a PHP fatal error.
//! 4. Releasing a PHP value can run PHP code (a destructor, the cycle
//!    collector), so Rust never releases one from `Drop`: dropping a
//!    [`ZvalHandle`](crate::convert::ZvalHandle) queues the reference with
//!    [`release_later`], and [`drain`] releases the queue, each value under
//!    its own guard, at the end of every entry from or into PHP.
//!
//! An exception is not a bailout: it is left in `EG(exception)` and taken
//! out by whoever called PHP. `exit()` throws PHP's internal unwind-exit
//! object; when it happens below a native function it is carried back up to
//! that function, which rethrows it, so the PHP code that called in unwinds
//! as PHP's own `exit()` would.
//!
//! Each fiber runs on its own C stack and may suspend while one of our native
//! calls is on it. The state the rules above use (native depth, pending
//! re-raise, parked exit, firing frames) is therefore kept per execution
//! context and swapped by fiber observers; see [`Engine`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::{CStr, c_void};
use std::io::Write;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::ptr;

use ext_php_rs::ffi::{
    _call_user_function_impl, _zend_oom_state_ZEND_OOM_STATE_HANDLED,
    _zend_oom_state_ZEND_OOM_STATE_NONE, _zend_oom_state_ZEND_OOM_STATE_PENDING, E_COMPILE_ERROR,
    E_CORE_ERROR, E_ERROR, E_PARSE, E_RECOVERABLE_ERROR, E_USER_ERROR, ZEND_RESULT_CODE_SUCCESS,
    ext_php_rs_executor_globals, ext_php_rs_process_globals, zend_execute_data, zend_function,
    zend_hash_str_find, zend_object, zend_read_property, zend_string, zval, zval_ptr_dtor,
};
use ext_php_rs::types::Zval;
use ext_php_rs::zend::{ExecuteData, bailout, try_catch};
use speedyspec_hooks::Value;

use crate::convert::{self, ConvertError, Unconvertible, ZvalHandle};
use crate::runtime::PhpError;

// Engine functions that ext-php-rs's bindings leave out. They are ZEND_API
// symbols of the statically linked libphp.
unsafe extern "C" {
    fn zend_is_unwind_exit(ex: *const zend_object) -> bool;
    fn zend_is_graceful_exit(ex: *const zend_object) -> bool;
    fn zend_throw_exception_internal(exception: *mut zend_object);
    fn zend_observer_fiber_init_register(handler: unsafe extern "C" fn(*mut c_void));
    fn zend_observer_fiber_switch_register(handler: unsafe extern "C" fn(*mut c_void, *mut c_void));
    fn zend_observer_fiber_destroy_register(handler: unsafe extern "C" fn(*mut c_void));
    // ZEND_FASTCALL is a plain C call on x86_64 and aarch64.
    fn zend_handle_delayed_oom();
}

// ---------------------------------------------------------------------------
// Engine state shared by the runtime and the native functions

/// Per-thread engine state. PHP only ever runs on one thread, the one that
/// booted it, and every native function and SAPI callback runs there too.
///
/// The first group of fields describes the *current execution context*: the
/// main context or one fiber, i.e. one C stack. Native calls nest strictly
/// on one stack, but not across stacks, because a fiber can suspend while
/// one of our native calls is on its stack. So these fields are swapped by
/// the fiber-switch observer ([`on_fiber_switch`]): the outgoing context's
/// values are saved in `contexts`, the incoming one's loaded. Everything
/// after them is process-wide; see `docs/php-embed.md` for why each is.
pub(crate) struct Engine {
    /// Native functions (called by PHP) currently on this context's stack.
    native_depth: Cell<u32>,
    /// A guard inside a native function caught a bailout; the innermost
    /// native function must re-raise it. No PHP code runs meanwhile.
    unwinding: Cell<bool>,
    /// `exit()` (or a fiber's graceful-exit unwind) happened below a native
    /// function; that function must rethrow this object (one owned
    /// reference). No PHP code runs meanwhile.
    pending_exit: Cell<*mut zend_object>,
    /// Firings started from PHP in this context; see [`Frame`].
    frames: RefCell<Vec<Frame>>,
    /// The saved records of the contexts not running now, by
    /// `zend_fiber_context` address.
    pub(crate) contexts: RefCell<BTreeMap<usize, ContextState>>,

    /// Between boot and the end of the request in `Drop`.
    pub(crate) alive: Cell<bool>,
    /// A panic carried through PHP by a bailout, to resume in Rust.
    pub(crate) panic: RefCell<Option<Box<dyn Any + Send>>>,
    /// References waiting to be released by [`drain`].
    pub(crate) releases: RefCell<Vec<ManuallyDrop<Zval>>>,
    /// How many times [`guard`] has entered PHP.
    entries: Cell<u64>,
    /// A bailout has happened; see [`Php::poisoned`](crate::Php::poisoned).
    pub(crate) poisoned: Cell<bool>,
    /// Gives a dummy frame a `func`; see [`call_php`].
    pub(crate) dummy_func: Cell<*mut zend_function>,
    /// The unwind-exit object PHP threw for a memory exhaustion, by address
    /// (never dereferenced), with the fatal error it stands for, while it is
    /// on its way to the PHP code below a native function. See
    /// [`settle_delayed_oom`].
    oom_exit: RefCell<Option<(usize, Fatal)>>,
}

thread_local! {
    pub(crate) static ENGINE: Engine = const {
        Engine {
            native_depth: Cell::new(0),
            unwinding: Cell::new(false),
            pending_exit: Cell::new(ptr::null_mut()),
            frames: RefCell::new(Vec::new()),
            contexts: RefCell::new(BTreeMap::new()),
            alive: Cell::new(false),
            panic: RefCell::new(None),
            releases: RefCell::new(Vec::new()),
            entries: Cell::new(0),
            poisoned: Cell::new(false),
            dummy_func: Cell::new(ptr::null_mut()),
            oom_exit: RefCell::new(None),
        }
    };
}

/// One firing started from PHP. A PHP callback's exception is kept here
/// (with the message its `CallbackError` carries) so the PHP caller can get
/// the original object back.
pub(crate) struct Frame {
    pub(crate) thrown: Option<(String, ZvalHandle)>,
}

/// The saved per-context part of [`Engine`] for a context that is not
/// running. Dropping one releases what it holds (later, see
/// [`release_later`]).
#[derive(Default)]
pub(crate) struct ContextState {
    native_depth: u32,
    unwinding: bool,
    pending_exit: Option<ZvalHandle>,
    frames: Vec<Frame>,
}

impl Engine {
    /// Moves the current context's state out, leaving a fresh one.
    pub(crate) fn take_current(&self) -> ContextState {
        let exit = self.pending_exit.replace(ptr::null_mut());
        ContextState {
            native_depth: self.native_depth.replace(0),
            unwinding: self.unwinding.replace(false),
            // SAFETY: `pending_exit` owns one reference, which moves here.
            pending_exit: (!exit.is_null()).then(|| unsafe { ZvalHandle::adopt_object(exit) }),
            frames: std::mem::take(&mut *self.frames.borrow_mut()),
        }
    }

    /// Makes `state` the current context's state. The current state must be
    /// fresh (just taken).
    fn load_current(&self, state: ContextState) {
        let ContextState {
            native_depth,
            unwinding,
            pending_exit,
            frames,
        } = state;
        self.native_depth.set(native_depth);
        self.unwinding.set(unwinding);
        self.pending_exit.set(
            pending_exit
                .and_then(ZvalHandle::into_object)
                .unwrap_or(ptr::null_mut()),
        );
        *self.frames.borrow_mut() = frames;
    }
}

/// Registers the fiber observers. Called from the module's MINIT: the
/// observer lists exist from `zend_observer_startup`, which
/// `php_module_startup` runs before any module's MINIT.
pub(crate) fn register_fiber_observers() {
    // SAFETY: plain registration of `extern "C"` handlers with the signatures
    // of zend_observer_fiber_{init,switch,destroy}_handler (pointers to
    // zend_fiber_context, which we never dereference).
    unsafe {
        zend_observer_fiber_init_register(on_fiber_init);
        zend_observer_fiber_switch_register(on_fiber_switch);
        zend_observer_fiber_destroy_register(on_fiber_destroy);
    }
}

/// Runs an observer body; a panic cannot leave an `extern "C"` function and
/// there is no PHP code to carry it through here, so it aborts.
fn observer(f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        eprintln!("speedyspec-php: panic in a fiber observer");
        std::process::abort();
    }
}

/// A new fiber context: forget any stale record at the same address (left by
/// a fiber PHP freed without destroying its context).
unsafe extern "C" fn on_fiber_init(context: *mut c_void) {
    observer(|| {
        let stale = ENGINE.with(|e| e.contexts.borrow_mut().remove(&(context as usize)));
        drop(stale);
    });
}

/// Called on the outgoing stack just before PHP jumps to `to`.
unsafe extern "C" fn on_fiber_switch(from: *mut c_void, to: *mut c_void) {
    observer(|| {
        ENGINE.with(|e| {
            let leaving = e.take_current();
            let (stale, entering) = {
                let mut contexts = e.contexts.borrow_mut();
                let stale = contexts.insert(from as usize, leaving);
                (stale, contexts.remove(&(to as usize)).unwrap_or_default())
            };
            e.load_current(entering);
            drop(stale);
        });
    });
}

/// A fiber context is gone (after the switch away from it).
unsafe extern "C" fn on_fiber_destroy(context: *mut c_void) {
    observer(|| {
        let record = ENGINE.with(|e| e.contexts.borrow_mut().remove(&(context as usize)));
        drop(record);
    });
}

/// Starts a firing in the current context.
pub(crate) fn push_frame() {
    ENGINE.with(|e| e.frames.borrow_mut().push(Frame { thrown: None }));
}

/// Ends the current context's innermost firing.
pub(crate) fn pop_frame() -> Option<Frame> {
    ENGINE.with(|e| e.frames.borrow_mut().pop())
}

/// Keeps a PHP callback's exception in the current context's innermost
/// firing; with none (fired from Rust), it is dropped.
pub(crate) fn keep_in_top_frame(message: String, object: ZvalHandle) {
    let replaced = ENGINE.with(|e| {
        e.frames
            .borrow_mut()
            .last_mut()
            .and_then(|frame| frame.thrown.replace((message, object)))
    });
    drop(replaced);
}

/// How many times the bridge has entered the PHP engine since the process
/// started. Every entry goes through [`guard`], which counts it.
pub(crate) fn entries() -> u64 {
    ENGINE.with(|e| e.entries.get())
}

/// How many per-context records are saved: one per context not running now
/// (suspended fibers, the main context while a fiber runs, and records of
/// fibers PHP abandoned).
pub(crate) fn fiber_records() -> usize {
    ENGINE.with(|e| e.contexts.borrow().len())
}

/// The running `zend_fiber_context` (main or a fiber), by address; 0 before
/// the engine starts and after it shuts down.
pub(crate) fn execution_context() -> usize {
    // SAFETY: a plain read of a pointer in the executor globals on the
    // engine's thread; it is never dereferenced.
    unsafe { (*ext_php_rs_executor_globals()).current_fiber_context as usize }
}

/// True from boot until the request ends in `Php::drop`.
pub(crate) fn alive() -> bool {
    ENGINE.with(|e| e.alive.get())
}

/// False after shutdown, or while a bailout or `exit()` is on its way to the
/// innermost native function (PHP must not be entered until it has been
/// re-raised).
pub(crate) fn can_enter() -> bool {
    alive() && !interrupted()
}

/// True while a bailout or `exit()` is on its way to the innermost native
/// function; PHP must not be entered until it has been re-raised.
fn interrupted() -> bool {
    ENGINE.with(|e| e.unwinding.get() || !e.pending_exit.get().is_null())
}

/// Why a [`guard`]ed call did not return.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Interrupted {
    /// PHP bailed out (a fatal error).
    Bailout,
    /// PHP was not entered: the engine is down, or a bailout or `exit()` is
    /// unwinding to a native function.
    Refused,
}

/// Runs `f` inside a `zend_try`, the only way any code here enters PHP.
///
/// `f` must capture only references and raw pointers: a bailout skips its
/// frame, so nothing it owns would be dropped. A panic inside `f` is resumed
/// as soon as `f` has unwound; a panic carried through PHP from a native
/// function deeper down is resumed here too.
pub(crate) fn guard<R>(f: impl FnOnce() -> R) -> Result<R, Interrupted> {
    if !can_enter() {
        return Err(Interrupted::Refused);
    }
    ENGINE.with(|e| e.entries.set(e.entries.get() + 1));
    // The plain (not "first") zend_try saves and restores the enclosing
    // EG(bailout), so guards nest: PHP code running inside one may call a
    // native function that enters another.
    let outcome = try_catch(AssertUnwindSafe(|| catch_unwind(AssertUnwindSafe(f))));
    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(payload)) => resume_unwind(payload),
        Err(_) => {
            // Every bailout, a panic carried through PHP included, leaves the
            // engine degraded for the rest of the request.
            ENGINE.with(|e| e.poisoned.set(true));
            // A memory exhaustion PHP handled with no PHP frame bails out
            // after recording the fatal error; end its handling (see
            // settle_delayed_oom) so later calls work.
            settle_delayed_oom();
            if let Some(payload) = ENGINE.with(|e| e.panic.borrow_mut().take()) {
                resume_unwind(payload);
            }
            ENGINE.with(|e| {
                if e.native_depth.get() > 0 {
                    // PHP code below a native function was abandoned
                    // mid-call (EG(current_execute_data) is now null): it
                    // must keep unwinding, see `native_entry`.
                    e.unwinding.set(true);
                }
            });
            Err(Interrupted::Bailout)
        }
    }
}

/// The body of every native function PHP calls. See the module docs.
pub(crate) fn native_entry(
    ex: &mut ExecuteData,
    retval: &mut Zval,
    body: fn(&mut ExecuteData, &mut Zval),
) {
    if run_native(ex, retval, body) {
        // SAFETY: this frame owns nothing; `run_native` has returned, so every
        // Rust value of the call has been dropped. The bailout continues to
        // the zend_try below, as it would have without us.
        unsafe { bailout() }
    }
}

/// Runs `body` and decides how the native call ends; true means re-raise a
/// bailout.
#[inline(never)]
fn run_native(
    ex: &mut ExecuteData,
    retval: &mut Zval,
    body: fn(&mut ExecuteData, &mut Zval),
) -> bool {
    // The body may suspend the fiber it runs in and be resumed much later;
    // the per-context state is switched under it by the fiber observer, so
    // every ENGINE access here reads this context's state.
    ENGINE.with(|e| e.native_depth.set(e.native_depth.get() + 1));
    // Releasing what the call let go of is still part of the call: a
    // destructor that bails out must be re-raised like any other bailout.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        body(ex, retval);
        drain();
    }));
    ENGINE.with(|e| e.native_depth.set(e.native_depth.get() - 1));
    if let Err(payload) = outcome {
        // Carry the panic through the PHP frames below as a bailout.
        ENGINE.with(|e| *e.panic.borrow_mut() = Some(payload));
        return true;
    }
    if ENGINE.with(|e| e.unwinding.replace(false)) {
        return true;
    }
    let exit = ENGINE.with(|e| e.pending_exit.replace(ptr::null_mut()));
    if !exit.is_null() {
        // SAFETY: hands our reference to EG(exception). The current frame is
        // this native function's, so PHP unwinds its caller as exit() does.
        unsafe { zend_throw_exception_internal(exit) };
    }
    false
}

/// Queues one reference for [`drain`]. After shutdown the reference points
/// into freed memory and is forgotten instead.
pub(crate) fn release_later(zv: ManuallyDrop<Zval>) {
    // SAFETY: reading the type flags of a zval we own.
    let counted =
        unsafe { u32::from(zv.u1.v.type_flags) & ext_php_rs::ffi::IS_TYPE_REFCOUNTED != 0 };
    if counted {
        let _ = ENGINE.try_with(|e| {
            if e.alive.get() {
                e.releases.borrow_mut().push(zv);
            }
        });
    }
}

/// Releases queued references, each under its own guard (a release can run
/// a destructor). Stops while PHP must not be entered; the queue is kept for
/// the next drain. An exception thrown by a destructor is left in
/// `EG(exception)` for the caller, as PHP itself does.
///
/// With no PHP code running, the release is made from a dummy frame, as
/// [`call_php`] calls: without a frame, `zend_call_function` turns a
/// destructor's exception into a fatal error ("Uncaught ...", a bailout that
/// poisons the engine; `zend_throw_exception_internal` in
/// `Zend/zend_exceptions.c`). With the frame it stays in `EG(exception)`,
/// where the caller reports it ([`take_thrown`]) or, outside any call,
/// [`settle_stray`] logs and clears it.
pub(crate) fn drain() {
    let dummy_func = ENGINE.with(|e| e.dummy_func.get());
    loop {
        if !can_enter() {
            return;
        }
        let Some(mut zv) = ENGINE.with(|e| e.releases.borrow_mut().pop()) else {
            return;
        };
        let zv_ptr: *mut zval = &raw mut *zv;
        let _ = guard(|| {
            // SAFETY: we own this reference; `zv` lives in this frame. The
            // dummy frame lives in this closure's frame for the duration of
            // the release; a bailout resets EG(current_execute_data) itself.
            unsafe {
                let eg = ext_php_rs_executor_globals();
                let mut dummy = MaybeUninit::<zend_execute_data>::zeroed();
                let installed = (*eg).current_execute_data.is_null() && !dummy_func.is_null();
                if installed {
                    (*dummy.as_mut_ptr()).func = dummy_func;
                    (*eg).current_execute_data = dummy.as_mut_ptr();
                }
                zval_ptr_dtor(zv_ptr);
                if installed {
                    (*eg).current_execute_data = ptr::null_mut();
                }
            }
        });
    }
}

/// What PHP threw, taken out of `EG(exception)`.
pub(crate) enum Thrown {
    /// `exit()` / `die()`, with `EG(exit_status)`.
    Exit(i32),
    /// Memory exhaustion: PHP reported the fatal error and unwinds with its
    /// exit object; see [`settle_delayed_oom`].
    Fatal(Fatal),
    /// PHP is destroying the suspended fiber this code runs in, unwinding it
    /// with its internal graceful-exit object. Not a process exit.
    FiberDestroyed,
    Exception {
        object: ZvalHandle,
        class: String,
        message: String,
        file: String,
        line: u32,
    },
}

impl Thrown {
    pub(crate) fn into_php_error(self) -> PhpError {
        match self {
            Self::Exit(status) => PhpError::Exit(status),
            Self::Fatal(fatal) => fatal.into_php_error(),
            Self::FiberDestroyed => PhpError::Exception {
                class: "GracefulExit".to_owned(),
                message: "the fiber running this code was destroyed".to_owned(),
                file: String::new(),
                line: 0,
            },
            Self::Exception {
                class,
                message,
                file,
                line,
                ..
            } => PhpError::Exception {
                class,
                message,
                file,
                line,
            },
        }
    }
}

/// Takes the pending exception, if any. An `exit()` below a native function
/// is parked for that function to rethrow (see [`run_native`]); one at the
/// top is released, and either way reported as [`Thrown::Exit`].
pub(crate) fn take_thrown() -> Option<Thrown> {
    // SAFETY: plain access to the executor globals on the engine's thread.
    // Taking EG(exception) moves its reference to us; the current frame is
    // internal (or none), so no opline needs restoring, as zend_clear_exception
    // shows.
    let obj = unsafe {
        let eg = ext_php_rs_executor_globals();
        let obj = (*eg).exception;
        if obj.is_null() {
            return None;
        }
        (*eg).exception = ptr::null_mut();
        obj
    };
    // SAFETY: `obj` is the live object EG(exception) held.
    let (unwind_exit, graceful_exit) =
        unsafe { (zend_is_unwind_exit(obj), zend_is_graceful_exit(obj)) };
    if unwind_exit || graceful_exit {
        // SAFETY: as above.
        let status = unsafe { (*ext_php_rs_executor_globals()).exit_status };
        // A memory exhaustion unwinds with the same object as exit(); tell
        // them apart before the object moves on.
        let oom = if unwind_exit {
            out_of_memory(obj)
        } else {
            None
        };
        // Either way the PHP code below a native function must keep
        // unwinding with this very object: parked for that function.
        ENGINE.with(|e| {
            if e.native_depth.get() > 0 && e.pending_exit.get().is_null() {
                e.pending_exit.set(obj);
                if let Some(fatal) = &oom {
                    // Recognised again by the next take_thrown that sees it,
                    // in this context or the one a fiber hands it to.
                    *e.oom_exit.borrow_mut() = Some((obj as usize, fatal.clone()));
                }
            } else {
                // The object is released here, so no later exit can be it.
                e.oom_exit.borrow_mut().take();
                // SAFETY: our reference, adopted and queued for release.
                drop(unsafe { ZvalHandle::adopt_object(obj) });
            }
        });
        return Some(match (graceful_exit, oom) {
            (true, _) => Thrown::FiberDestroyed,
            (false, Some(fatal)) => Thrown::Fatal(fatal),
            (false, None) => Thrown::Exit(status),
        });
    }
    // SAFETY: we own the reference just taken from EG(exception).
    let object = unsafe { ZvalHandle::adopt_object(obj) };
    let (class, message, file, line) = describe_throwable(obj);
    Some(Thrown::Exception {
        object,
        class,
        message,
        file,
        line,
    })
}

/// Releases what Rust has let go of and clears an exception that releasing
/// (or an earlier release) left in `EG(exception)`, logging it to stderr.
///
/// Called at the start and end of every top-level `Php` entry. A destructor
/// that throws while a value is released outside any PHP call leaves its
/// exception pending; the next `zend_call_function` would then return at
/// once and that exception would be reported as the next call's. Inside a
/// native function (some PHP code below us will see the exception) this
/// does nothing.
pub(crate) fn settle_stray() {
    if ENGINE.with(|e| e.native_depth.get()) > 0 {
        return;
    }
    // A released exception object can have a throwing destructor of its own;
    // a few rounds settle any sane chain.
    for _ in 0..8 {
        drain();
        let Some(thrown) = take_thrown() else {
            return;
        };
        let what = match &thrown {
            Thrown::Exception {
                class,
                message,
                file,
                line,
                ..
            } => format!("an uncaught {class} ({message}) thrown in {file}:{line}"),
            Thrown::Exit(status) => format!("exit({status})"),
            Thrown::Fatal(fatal) => format!("a fatal error ({})", fatal.message),
            Thrown::FiberDestroyed => "a fiber's unwinding".to_owned(),
        };
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "speedyspec: discarded {what} while releasing a PHP value outside any call"
        );
        drop(thrown);
    }
}

/// Drops whatever is left in `EG(exception)` after a bailout. A leftover
/// exception would make every later `zend_call_function` return at once.
pub(crate) fn discard_exception() {
    // SAFETY: plain access to the executor globals; the reference moves to
    // the handle, which queues its release.
    unsafe {
        let eg = ext_php_rs_executor_globals();
        let obj = (*eg).exception;
        if !obj.is_null() {
            (*eg).exception = ptr::null_mut();
            drop(ZvalHandle::adopt_object(obj));
        }
    }
}

/// Rethrows an exception previously taken with [`take_thrown`], unchanged.
pub(crate) fn rethrow(object: ZvalHandle) {
    if let Some(obj) = object.into_object() {
        // SAFETY: hands our reference to EG(exception) from inside a native
        // function, which is how internal functions throw.
        unsafe { zend_throw_exception_internal(obj) };
    }
}

/// Class, message, file and line of a Throwable.
fn describe_throwable(obj: *mut zend_object) -> (String, String, String, u32) {
    // SAFETY: the caller holds a reference to `obj`.
    let class = unsafe { class_name(obj) };
    let message = read_property(obj, c"message")
        .and_then(|v| match v {
            Value::Str(s) => Some(s),
            Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            _ => None,
        })
        .unwrap_or_default();
    let file = match read_property(obj, c"file") {
        Some(Value::Str(s)) => s,
        _ => String::new(),
    };
    let line = match read_property(obj, c"line") {
        Some(Value::Int(n)) => u32::try_from(n).unwrap_or(0),
        _ => 0,
    };
    (class, message, file, line)
}

/// Reads one property of an object we hold a reference to.
///
/// Message, file and line are protected properties declared by Exception
/// and Error; reading them with the object's own class as scope is allowed
/// and normally returns the property slot itself. If the engine fills the
/// `rv` buffer instead, the value is read and released while the buffer is
/// still alive.
fn read_property(obj: *mut zend_object, name: &CStr) -> Option<Value> {
    let mut rv = ManuallyDrop::new(Zval::new());
    rv.u1.type_info = ext_php_rs::ffi::IS_UNDEF;
    let rv_ptr: *mut zval = &raw mut *rv;
    let found = guard(|| {
        // SAFETY: `obj` is live; `rv` is zval storage owned by this frame.
        unsafe {
            zend_read_property(
                (*obj).ce,
                obj,
                name.as_ptr(),
                name.to_bytes().len(),
                true,
                rv_ptr,
            )
        }
    })
    .ok()
    .filter(|zv| !zv.is_null());
    // SAFETY: `found` is either a slot of the live object or `rv`, which is
    // still alive here; reading runs no PHP code.
    let value = found.and_then(|zv| unsafe { convert::to_value(zv) }.ok());
    release_later(rv);
    value
}

/// The name of an object's class.
///
/// # Safety
///
/// `obj` is a live object.
pub(crate) unsafe fn class_name(obj: *mut zend_object) -> String {
    // SAFETY: a live object has a class with a name.
    unsafe { String::from_utf8_lossy(convert::str_bytes((*(*obj).ce).name)).into_owned() }
}

/// A PHP fatal error: message, file and line.
#[derive(Clone, Debug)]
pub(crate) struct Fatal {
    message: String,
    file: String,
    line: u32,
}

impl Fatal {
    fn into_php_error(self) -> PhpError {
        PhpError::Fatal {
            message: self.message,
            file: self.file,
            line: self.line,
        }
    }
}

/// The last error PHP recorded, as the fatal error a bailout came from.
pub(crate) fn fatal_error(interrupted: Interrupted) -> PhpError {
    if let Interrupted::Refused = interrupted {
        return PhpError::Fatal {
            message: "PHP is unwinding from a fatal error or exit() and cannot run code".to_owned(),
            file: String::new(),
            line: 0,
        };
    }
    last_fatal().into_php_error()
}

/// The last error PHP recorded, if it is a fatal one.
fn last_fatal() -> Fatal {
    const FATAL: u32 =
        E_ERROR | E_CORE_ERROR | E_COMPILE_ERROR | E_USER_ERROR | E_RECOVERABLE_ERROR | E_PARSE;
    // SAFETY: plain reads of the core globals on the engine's thread; the
    // strings are owned by PHP until the next error replaces them.
    unsafe {
        let pg = ext_php_rs_process_globals();
        let kind = u32::try_from((*pg).last_error_type).unwrap_or(0);
        if kind & FATAL == 0 || (*pg).last_error_message.is_null() {
            return Fatal {
                message: "PHP bailed out".to_owned(),
                file: String::new(),
                line: 0,
            };
        }
        let text = |s: *mut zend_string| {
            if s.is_null() {
                String::new()
            } else {
                String::from_utf8_lossy(convert::str_bytes(s)).into_owned()
            }
        };
        Fatal {
            message: text((*pg).last_error_message),
            file: text((*pg).last_error_file),
            line: u32::try_from((*pg).last_error_lineno).unwrap_or(0),
        }
    }
}

/// Ends PHP's handling of a memory exhaustion it has reported, so the engine
/// keeps working; true if there was one.
///
/// This PHP handles memory exhaustion as a "delayed OOM"
/// (`Zend/zend_alloc.c`, `zend_mm_check_limit` and `zend_handle_delayed_oom`):
/// the allocation that crosses `memory_limit` succeeds and queues the error,
/// and the next VM interrupt or `zend_call_function` handles it. With PHP code
/// on the stack it throws the unwind-exit object (exit status 255) and reports
/// `E_ERROR` "Allowed memory size of ... exhausted", which bails out to a
/// `zend_try` of its own, so the engine is degraded as after any fatal error;
/// with none it bails out to the caller. Either way it leaves
/// `EG(delayed_oom).state` at `ZEND_OOM_STATE_HANDLED`, which PHP resets only
/// in `php_request_shutdown` (`main/main.c`); until then every
/// `zend_call_function` returns FAILURE (`Zend/zend_execute_API.c`, the
/// `ZEND_HAS_OOM()` check). speedyspec never ends its request, so this resets
/// it to `ZEND_OOM_STATE_NONE` as request shutdown does (the message and file
/// were already freed by `zend_mm_oom_info_dtor`) and poisons the engine.
fn settle_delayed_oom() -> bool {
    // SAFETY: plain access to the executor globals on the engine's thread.
    let handled = unsafe {
        let oom = &raw mut (*ext_php_rs_executor_globals()).delayed_oom;
        if (*oom).state == _zend_oom_state_ZEND_OOM_STATE_HANDLED {
            (*oom).state = _zend_oom_state_ZEND_OOM_STATE_NONE;
            true
        } else {
            false
        }
    };
    if handled {
        ENGINE.with(|e| e.poisoned.set(true));
    }
    handled
}

/// If the unwind-exit object `obj` stands for a memory exhaustion (just
/// handled, or recognised from an earlier [`take_thrown`] that parked it), the
/// fatal error PHP reported for it.
fn out_of_memory(obj: *mut zend_object) -> Option<Fatal> {
    if settle_delayed_oom() {
        // zend_handle_delayed_oom reports the error right after throwing.
        return Some(last_fatal());
    }
    ENGINE.with(|e| {
        let marked = e.oom_exit.borrow();
        match marked.as_ref() {
            Some((address, fatal)) if *address == obj as usize => Some(fatal.clone()),
            _ => None,
        }
    })
}

/// Lets PHP handle a memory exhaustion that is still queued once top-level
/// code has finished, as `zend_execute_scripts` does after each file
/// (`Zend/zend.c`). Otherwise the next, unrelated call would report it. With
/// no PHP code on the stack PHP records the fatal error and bails out (the
/// guard settles it); with a frame it throws the unwind-exit object, which
/// [`take_thrown`] recognises.
///
/// # Safety
///
/// Called inside a [`guard`], since PHP may bail out.
pub(crate) unsafe fn handle_queued_oom() {
    // SAFETY: plain read of the executor globals; a bailout from PHP's own
    // handler lands in the caller's guard.
    unsafe {
        if (*ext_php_rs_executor_globals()).delayed_oom.state
            == _zend_oom_state_ZEND_OOM_STATE_PENDING
        {
            zend_handle_delayed_oom();
        }
    }
}

/// Why [`call_php`] did not return a value.
pub(crate) enum CallError {
    Thrown(Thrown),
    Interrupted(Interrupted),
    Convert(String),
}

impl CallError {
    pub(crate) fn into_php_error(self) -> PhpError {
        match self {
            Self::Thrown(thrown) => thrown.into_php_error(),
            Self::Interrupted(interrupted) => fatal_error(interrupted),
            Self::Convert(why) => PhpError::Conversion(why),
        }
    }

    /// The message a hook callback reports for this failure.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Thrown(Thrown::Exit(status)) => format!("PHP code called exit({status})"),
            Self::Thrown(Thrown::Fatal(fatal)) => fatal.clone().into_php_error().to_string(),
            Self::Thrown(Thrown::FiberDestroyed) => {
                "the fiber running this callback was destroyed".to_owned()
            }
            Self::Thrown(Thrown::Exception {
                class,
                message,
                file,
                line,
                ..
            }) => format!("{class}: {message} in {file}:{line}"),
            Self::Interrupted(interrupted) => fatal_error(*interrupted).to_string(),
            Self::Convert(why) => why.clone(),
        }
    }
}

/// Calls the PHP callable `callable` with `args`.
///
/// Called from Rust with no PHP code running (`EG(current_execute_data)` is
/// null), `zend_call_function` turns an uncaught exception into a fatal
/// error, as it must for `{main}`. To get the exception back instead, the
/// call is made from a dummy frame of an internal function, which is what
/// calling from inside a native function looks like.
pub(crate) fn call_php(callable: *mut zval, args: &[Value]) -> Result<Value, CallError> {
    // Called from Rust with no PHP code running (a hook fired from Rust), values
    // Rust let go of earlier must be released, and any exception their
    // destructors throw cleared, before the call: otherwise the post-call drain
    // below would release them and the callback would be blamed for (and lose
    // its result to) an exception that is not its own. Inside a native call
    // this does nothing.
    settle_stray();
    // Owned here; the guard only sees pointers.
    let mut params: Vec<ManuallyDrop<Zval>> = args
        .iter()
        .map(|_| ManuallyDrop::new(Zval::new()))
        .collect();
    let params_ptr: *mut zval = params.as_mut_ptr().cast();
    let count = u32::try_from(params.len()).unwrap_or(u32::MAX);
    let mut retval = ManuallyDrop::new(Zval::new());
    let retval_ptr: *mut zval = &raw mut *retval;
    let dummy_func = ENGINE.with(|e| e.dummy_func.get());

    let called = guard(|| -> Result<bool, Unconvertible> {
        // SAFETY: `params` and `retval` are zval storage owned by the
        // enclosing frame; `params` has `count` slots (ManuallyDrop<Zval> is
        // a transparent Zval). The dummy frame lives in this frame for the
        // duration of the call; a bailout resets EG(current_execute_data)
        // itself.
        unsafe {
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
            let result = _call_user_function_impl(
                ptr::null_mut(),
                callable,
                retval_ptr,
                count,
                params_ptr,
                ptr::null_mut(),
            );
            if installed {
                (*eg).current_execute_data = ptr::null_mut();
            }
            Ok(result == ZEND_RESULT_CODE_SUCCESS)
        }
    });

    // SAFETY: `retval` is null or what the callable returned.
    let value = unsafe { convert::to_value(retval_ptr) };
    for param in params {
        release_later(param);
    }
    release_later(retval);
    // What the call left in EG(exception) is the call's own; anything the
    // releases below add while it was clear is not. (Releasing this call's
    // own temporaries does not destroy objects on the success path: every
    // object in the arguments is still held by `args`, and every object in
    // the result by `value`. This keeps a successful result from ever being
    // replaced by a release's exception regardless.)
    // SAFETY: a plain read of the executor globals on the engine's thread.
    let clear_before = matches!(called, Ok(Ok(true)))
        && unsafe { (*ext_php_rs_executor_globals()).exception.is_null() };
    drain();
    if clear_before {
        settle_stray();
    }

    match called {
        Err(interrupted) => {
            discard_exception();
            return Err(CallError::Interrupted(interrupted));
        }
        Ok(Err(why)) => return Err(CallError::Convert(why.to_string())),
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => {
            // zend_call_function also fails, with the unwind-exit object
            // thrown, when it handles a queued memory exhaustion first.
            if let Some(thrown) = take_thrown() {
                return Err(CallError::Thrown(thrown));
            }
            return Err(CallError::Convert(
                "PHP refused the call (the executor is inactive)".to_owned(),
            ));
        }
    }
    if let Some(thrown) = take_thrown() {
        return Err(CallError::Thrown(thrown));
    }
    value
        .map_err(|e: ConvertError| CallError::Convert(format!("the result cannot reach Rust: {e}")))
}

/// Looks up a function in the engine's function table.
///
/// # Safety
///
/// A request is open.
pub(crate) unsafe fn find_function(name: &str) -> *mut zend_function {
    // SAFETY: function_table is a live hash of IS_PTR zvals.
    unsafe {
        let table = (*ext_php_rs_executor_globals()).function_table;
        let zv = zend_hash_str_find(table, name.as_ptr().cast(), name.len());
        if zv.is_null() {
            ptr::null_mut()
        } else {
            (*zv).value.func
        }
    }
}

/// A zval view of an array pointer, for [`convert::to_value`].
pub(crate) fn table_as_zval(table: *mut ext_php_rs::ffi::zend_array) -> ManuallyDrop<Zval> {
    let mut zv = ManuallyDrop::new(Zval::new());
    zv.value.arr = table;
    // Not marked refcounted: this view holds no reference of its own.
    zv.u1.type_info = ext_php_rs::ffi::IS_ARRAY;
    zv
}
