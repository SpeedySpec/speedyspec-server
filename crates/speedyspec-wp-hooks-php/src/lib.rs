//! WordPress's hook functions, `WP_Hook` and the hook globals in PHP, over the Rust Hook API.
//!
//! [`WpHooksArea`] declares, at the start of every request (from `hooks.php`), WordPress's hook functions with their
//! exact signatures and defaults, the `WP_Hook` class, and proxies for `$wp_filter`, `$wp_actions`, `$wp_filters`
//! and `$wp_current_filter`. All of them are thin: every function calls a native function that dispatches the use
//! case through its handler (resolved once, `HandlerRef`), and every `WP_Hook` is a handle to a Rust [`WpHook`].
//! The state lives in the hook store bound under [`HOOK_STORE`] (the worker's [`MemoryHookStore`], or a PHP store).
//!
//! A PHP callable registered as a callback is a `Callback::Foreign`; calling it is a boundary crossing. A Rust
//! caller whose hook has only Rust callbacks never enters PHP.
//!
//! The hot functions (`add_filter`, `apply_filters`, `do_action`, ... and their `WP_Hook` methods) take their
//! arguments unconverted ([`native::raw_function`]):
//!
//! - only what Rust reads is converted: the hook name, the priority, the accepted-args count and a callback's
//!   identity. The filtered value and the arguments pass through as the zvals PHP gave, to the PHP callbacks and back
//!   (a Rust callback converts what it reads);
//! - a hook is looked up by the hash PHP already keeps in the name's `zend_string` ([`HookKey::prehashed`]);
//! - `apply_filters()` and `do_action()` on a hook with no callbacks (and no `all` hook) count the run and hand the
//!   value straight back, before any argument list is built;
//! - each PHP callback is resolved once per request and called through Zend's fast path
//!   ([`speedyspec_php::Callable`]).
//!
//! `speedyspec_php_ext::boundary_crossings()` counts the entries into PHP: none for a hook with no callbacks, one per
//! PHP callback run.
//!
//! **The documented break:** a hook global replaced wholesale (`$wp_filter = [...]`) is a plain PHP array, no longer
//! the proxy, and the Rust store does not see it; `$wp_filter[$name]` holds only `WP_Hook` objects
//! (`compat/hooks/breaks.json`).

mod callback;
mod mirror;
mod registry;
mod stores;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::CommandHandler;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, Args, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::{
    AddFilter, ApplyFilters, ApplyFiltersDeprecated, ApplyFiltersRefArray, CurrentFilter,
    DidAction, DidFilter, DoAction, DoActionDeprecated, DoActionRefArray, DoingFilter, Entry,
    HasFilter, WpHookApi, HookStore, Partition, PrioritySlice, RemoveAllFilters, RemoveFilter,
    WpHook, register_hook_handlers,
};
use speedyspec_wp_hooks_contracts::{
    ArrayKey, Count, HasFilterResult, HookError, HookKey, HookName,
};
use speedyspec_wp_hooks_memory::MemoryHookStore;

pub use callback::{PhpCallable, php_callback, php_callback_of, unique_id};
pub use stores::{HOOK_STORE, HookStorage, PhpHookStorage, PhpHookStore, ResolvedHookStore};

const FACADE: &str = include_str!("hooks.php");
const TESTING: &str = r#"namespace SpeedySpec\WpHooks;

/**
 * Saves and restores the whole hook store, for test cases that back up their hooks.
 */
final class Testing
{
    public static function snapshot(): int
    {
        return Native\testing_snapshot();
    }

    public static function restore(int $snapshot): void
    {
        Native\testing_restore($snapshot);
    }
}
"#;
const NATIVE: &str = "SpeedySpec\\WpHooks\\Native\\";

/// The handlers PHP's functions call, each resolved once.
struct Handlers {
    add_filter: HandlerRef<AddFilter>,
    remove_filter: HandlerRef<RemoveFilter>,
    remove_all_filters: HandlerRef<RemoveAllFilters>,
    apply_filters: HandlerRef<ApplyFilters>,
    apply_filters_ref_array: HandlerRef<ApplyFiltersRefArray>,
    apply_filters_deprecated: HandlerRef<ApplyFiltersDeprecated>,
    do_action: HandlerRef<DoAction>,
    do_action_ref_array: HandlerRef<DoActionRefArray>,
    do_action_deprecated: HandlerRef<DoActionDeprecated>,
    has_filter: QueryRef<HasFilter>,
    did_action: QueryRef<DidAction>,
    did_filter: QueryRef<DidFilter>,
    doing_filter: QueryRef<DoingFilter>,
    current_filter: QueryRef<CurrentFilter>,
}

struct State {
    api: Rc<WpHookApi>,
    memory: Rc<MemoryHookStore>,
    handlers: Handlers,
}

thread_local! {
    static STATE: std::cell::RefCell<Option<Rc<State>>> = const { std::cell::RefCell::new(None) };
}

fn state() -> Result<Rc<State>, NativeError> {
    STATE
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the Hook API is not available"))
}

/// The native area of WordPress's Hook API.
pub struct WpHooksArea {
    state: Rc<State>,
    testing: bool,
    legacy: bool,
}

impl WpHooksArea {
    /// Binds the hook store (`memory`, for the worker) and every Hook API handler in `container`.
    pub fn new(container: Rc<Container>, memory: Rc<MemoryHookStore>) -> WpHooksArea {
        let worker: Rc<dyn HookStore> = memory.clone();
        let bound = worker.clone();
        container.singleton_port::<dyn HookStore>(HOOK_STORE, move |_, _| Ok(bound.clone()));
        let store = Rc::new(ResolvedHookStore::new(&container, worker));
        let api = Rc::new(WpHookApi::new(store, Rc::new(stores::PhpDeprecations)));
        register_hook_handlers(&container, api.clone());
        let handlers = Handlers {
            add_filter: HandlerRef::new(container.clone()),
            remove_filter: HandlerRef::new(container.clone()),
            remove_all_filters: HandlerRef::new(container.clone()),
            apply_filters: HandlerRef::new(container.clone()),
            apply_filters_ref_array: HandlerRef::new(container.clone()),
            apply_filters_deprecated: HandlerRef::new(container.clone()),
            do_action: HandlerRef::new(container.clone()),
            do_action_ref_array: HandlerRef::new(container.clone()),
            do_action_deprecated: HandlerRef::new(container.clone()),
            has_filter: QueryRef::new(container.clone()),
            did_action: QueryRef::new(container.clone()),
            did_filter: QueryRef::new(container.clone()),
            doing_filter: QueryRef::new(container.clone()),
            current_filter: QueryRef::new(container),
        };
        let state = Rc::new(State {
            api,
            memory,
            handlers,
        });
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        WpHooksArea {
            state,
            testing: false,
            legacy: true,
        }
    }

    /// Leaves out WordPress's legacy names: no `hooks.php` (the global hook functions, `WP_Hook`) and no hook
    /// globals, so PHP that declares those names itself still loads. The native API
    /// (`SpeedySpec\WpHooks\Native\*`) and the hook store stay. For hosts that never run WordPress's own
    /// `plugin.php`-era code, such as the speedyspec CLI.
    pub fn without_legacy(mut self) -> WpHooksArea {
        self.legacy = false;
        self
    }

    /// Adds `SpeedySpec\WpHooks\Testing::snapshot()` and `restore()`, which save and restore the whole hook store
    /// (WordPress's test case backs up its hooks around each test). For test runs only.
    pub fn with_testing(mut self) -> WpHooksArea {
        self.testing = true;
        self
    }

    /// The Hook API (`plugin.php`) for Rust callers.
    pub fn api(&self) -> Rc<WpHookApi> {
        self.state.api.clone()
    }
}

/// The native functions find the area's state through a thread-local; it goes with the area, so the area's container
/// is freed once nothing else holds it. A newer area's state is left in place.
impl Drop for WpHooksArea {
    fn drop(&mut self) {
        // `try_with`: at thread exit the thread-local may already be gone (the areas are thread-locals too).
        let _ = STATE.try_with(|s| {
            let mut s = s.borrow_mut();
            if s.as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, &self.state))
            {
                *s = None;
            }
        });
    }
}

impl NativeArea for WpHooksArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        type Native = fn(&[Value]) -> Result<Value, NativeError>;
        type Raw = fn(&Args<'_>) -> Result<Value, NativeError>;
        // The hot functions read their own arguments, and sync `$wp_current_filter` only when they run callbacks.
        let raw: Vec<(&str, Raw)> = vec![
            ("add_filter", n_add_filter),
            ("apply_filters", n_apply_filters),
            ("apply_filters_ref_array", n_apply_filters_ref_array),
            ("has_filter", n_has_filter),
            ("remove_filter", n_remove_filter),
            ("do_action", n_do_action),
            ("do_action_ref_array", n_do_action_ref_array),
            ("hook_add_filter", n_hook_add_filter),
            ("hook_apply_filters", n_hook_apply_filters),
            ("hook_do_action", n_hook_do_action),
            ("hook_do_all_hook", n_hook_do_all_hook),
            ("call_all_hook", n_call_all_hook),
        ];
        let mut entries: Vec<(&str, Native)> = vec![
            ("remove_all_filters", n_remove_all_filters),
            ("current_filter", n_current_filter),
            ("doing_filter", n_doing_filter),
            ("did_filter", n_did_filter),
            ("did_action", n_did_action),
            ("apply_filters_deprecated", n_apply_filters_deprecated),
            ("do_action_deprecated", n_do_action_deprecated),
            ("unique_id", n_unique_id),
            ("hook_new", n_hook_new),
            ("hook_clone", n_hook_clone),
            ("hook_remove_filter", n_hook_remove_filter),
            ("hook_has_filter", n_hook_has_filter),
            ("hook_has_filters", n_hook_has_filters),
            ("hook_remove_all_filters", n_hook_remove_all_filters),
            ("hook_current_priority", n_hook_current_priority),
            ("hook_callbacks", n_hook_callbacks),
            ("hook_replace_callbacks", n_hook_replace_callbacks),
            ("hook_priorities", n_hook_priorities),
            ("hook_offset_exists", n_hook_offset_exists),
            ("hook_offset_get", n_hook_offset_get),
            ("hook_offset_set", n_hook_offset_set),
            ("hook_offset_unset", n_hook_offset_unset),
            ("global_has", n_global_has),
            ("global_get", n_global_get),
            ("global_set", n_global_set),
            ("global_unset", n_global_unset),
            ("global_all", n_global_all),
            ("global_count", n_global_count),
        ];
        if self.testing {
            entries.push(("testing_snapshot", n_testing_snapshot));
            entries.push(("testing_restore", n_testing_restore));
        }
        let raw = raw
            .into_iter()
            .map(|(name, f)| native::raw_function(&format!("{NATIVE}{name}"), f));
        entries
            .into_iter()
            .map(|(name, f)| {
                native::function(&format!("{NATIVE}{name}"), move |args: &[Value]| {
                    // PHP may have changed $wp_current_filter; it sees the store's stack again on return.
                    let store = state().map(|s| s.api.store().clone());
                    if let Ok(store) = &store {
                        mirror::sync_in(store.as_ref());
                    }
                    let result = f(args);
                    if let Ok(store) = &store {
                        mirror::sync_out(store.as_ref());
                    }
                    result
                })
            })
            .chain(raw)
            .collect()
    }

    fn request_start(&self) {
        self.state.memory.start_request();
        if !self.legacy {
            return;
        }
        let installed = speedyspec_php::eval("speedyspec://wp-hooks", FACADE)
            .and_then(|_| {
                if self.testing {
                    speedyspec_php::eval("speedyspec://wp-hooks-testing", TESTING)
                } else {
                    Ok(Value::Null)
                }
            })
            .and_then(|_| {
                speedyspec_php::call(
                    &Value::Str("SpeedySpec\\WpHooks\\install_globals".into()),
                    &[],
                )
            });
        mirror::start();
        if let Err(err) = installed {
            eprintln!("speedyspec_wordpress: installing the Hook API failed: {err}");
        }
    }

    fn request_end(&self) {
        mirror::end();
        registry::clear();
        self.state.memory.end_request();
    }
}

/// A runner of Task 6's hook scenarios through these functions (nothing to load: they are native).
pub fn hooks_scenario_runner(php: speedyspec_php::Php) -> speedyspec_compat::PhpRunner {
    speedyspec_compat::PhpRunner::new(php, |_| Ok(()))
}

// ----- conversions

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// Runs `f` with `$wp_current_filter` synced: PHP's changes come in first, and PHP sees the store's stack on return.
fn synced<T>(s: &State, f: impl FnOnce() -> T) -> T {
    let store = s.api.store();
    mirror::sync_in(store.as_ref());
    let result = f();
    mirror::sync_out(store.as_ref());
    result
}

/// The hook name in argument `i`, converted (Rust reads it) and keyed by the hash PHP keeps in its `zend_string`.
fn hook_name(args: &Args<'_>, i: usize) -> Result<HookName, NativeError> {
    let value = args.value(i)?;
    if let Value::Str(name) = &value
        && let key @ ArrayKey::Str(_) = ArrayKey::from_str_key(name)
        && let Some(hash) = args.string_hash(i)
    {
        return Ok(HookName::prehashed(value, HookKey::prehashed(key, hash)));
    }
    // An integer key hashes to itself; anything else keeps PHP's error for later.
    Ok(HookName::new(value))
}

/// True while `handler` is the Hook API itself (PHP did not bind its own), so its fast path may skip it.
fn is_default<C>(handler: &HandlerRef<C>, api: &Rc<WpHookApi>) -> bool
where
    C: Command,
    WpHookApi: CommandHandler<C>,
{
    handler
        .handler()
        .is_ok_and(|bound| std::ptr::addr_eq(Rc::as_ptr(&bound), Rc::as_ptr(api)))
}

fn values_of(array: &Value) -> Vec<Value> {
    match array {
        Value::Array(entries) => entries.iter().map(|(_, v)| v.clone()).collect(),
        _ => Vec::new(),
    }
}

fn is_false(value: &Value) -> bool {
    matches!(value, Value::Bool(false))
}

fn hook_error(err: HookError) -> NativeError {
    if let Some(payload) = &err.payload {
        if let Some(exception) = payload.downcast_ref::<Value>() {
            return NativeError::Rethrow(exception.clone());
        }
        if payload.downcast_ref::<callback::Unwinding>().is_some() {
            return NativeError::Silent;
        }
    }
    let class = if err.message.starts_with("Cannot access offset") {
        "TypeError"
    } else {
        "Error"
    };
    NativeError::throw(class, err.message)
}

fn bus_error(err: BusError) -> NativeError {
    NativeError::throw("Error", err.to_string())
}

fn command<C: Command<Result = Result<T, HookError>>, T>(
    handler: &HandlerRef<C>,
    message: C,
) -> Result<T, NativeError> {
    handler
        .call(message)
        .map_err(bus_error)?
        .map_err(hook_error)
}

fn query<Q: Query<Result = Result<T, HookError>>, T>(
    handler: &QueryRef<Q>,
    message: Q,
) -> Result<T, NativeError> {
    handler
        .call(message)
        .map_err(bus_error)?
        .map_err(hook_error)
}

fn has_filter_value(result: HasFilterResult) -> Value {
    match result {
        HasFilterResult::No => Value::Bool(false),
        HasFilterResult::Any => Value::Bool(true),
        HasFilterResult::At(priority) => priority.to_value(),
    }
}

/// `has_filter()`'s callback (`None` for `false`) and priority (only an integer counts), from argument `first` on.
fn has_filter_args(
    args: &Args<'_>,
    first: usize,
) -> Result<
    (
        Option<speedyspec_wp_hooks_contracts::CallbackId>,
        Option<i64>,
    ),
    NativeError,
> {
    let callback = if args.is_false(first) {
        None
    } else {
        Some(callback::callback_id(&args.value(first)?))
    };
    Ok((callback, args.int(first + 1)))
}

fn list(values: Vec<Value>) -> Value {
    Value::Array(
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

fn key(value: &Value) -> Result<ArrayKey, NativeError> {
    ArrayKey::from_value(value).map_err(hook_error)
}

/// A hook name PHP used as an offset of a hook global, as a store key.
fn hook_key(value: &Value) -> Result<HookKey, NativeError> {
    key(value).map(HookKey::new)
}

fn key_value(key: &ArrayKey) -> Key {
    match key {
        ArrayKey::Int(n) => Key::Int(*n),
        ArrayKey::Str(s) => Key::Str(s.clone()),
    }
}

// ----- WordPress's functions

fn n_add_filter(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let message = AddFilter {
        hook: hook_name(args, 0)?,
        callback: php_callback_of(args.value(1)?, args.pass(1)),
        priority: args.value(2)?,
        accepted_args: args.int(3).unwrap_or(1),
    };
    command(&s.handlers.add_filter, message).map(Value::Bool)
}

fn n_apply_filters(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = hook_name(args, 0)?;
    // Most filters have no callbacks: the value goes straight back, no argument built.
    if is_default(&s.handlers.apply_filters, &s.api)
        && s.api
            .skip_if_empty(&hook, Count::Filters)
            .map_err(hook_error)?
    {
        return Ok(args.pass(1));
    }
    let message = ApplyFilters {
        hook,
        value: args.pass(1),
        args: args.pass_from(2),
    };
    synced(&s, || command(&s.handlers.apply_filters, message))
}

fn n_apply_filters_ref_array(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let message = ApplyFiltersRefArray {
        hook: hook_name(args, 0)?,
        args: args.items(1),
    };
    synced(&s, || command(&s.handlers.apply_filters_ref_array, message))
}

fn n_has_filter(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let (callback, priority) = has_filter_args(args, 1)?;
    query(
        &s.handlers.has_filter,
        HasFilter {
            hook: hook_name(args, 0)?,
            callback,
            priority,
        },
    )
    .map(has_filter_value)
}

fn n_remove_filter(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let message = RemoveFilter {
        hook: hook_name(args, 0)?,
        callback: callback::callback_id(&args.value(1)?),
        priority: args.value(2)?,
    };
    command(&s.handlers.remove_filter, message).map(Value::Bool)
}

fn n_remove_all_filters(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let priority = arg(args, 1);
    let message = RemoveAllFilters {
        hook: HookName::new(arg(args, 0)),
        priority: (!is_false(&priority)).then_some(priority),
    };
    command(&s.handlers.remove_all_filters, message).map(Value::Bool)
}

fn n_current_filter(_args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let current = s
        .handlers
        .current_filter
        .call(CurrentFilter)
        .map_err(bus_error)?;
    Ok(current.unwrap_or(Value::Bool(false)))
}

fn n_doing_filter(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = match arg(args, 0) {
        Value::Null => None,
        hook => Some(hook),
    };
    s.handlers
        .doing_filter
        .call(DoingFilter { hook })
        .map(Value::Bool)
        .map_err(bus_error)
}

fn n_did_filter(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = HookName::new(arg(args, 0));
    query(&s.handlers.did_filter, DidFilter { hook }).map(Value::Int)
}

fn n_did_action(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = HookName::new(arg(args, 0));
    query(&s.handlers.did_action, DidAction { hook }).map(Value::Int)
}

fn n_do_action(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = hook_name(args, 0)?;
    // Most actions have no callbacks: nothing to build or run.
    if is_default(&s.handlers.do_action, &s.api)
        && s.api
            .skip_if_empty(&hook, Count::Actions)
            .map_err(hook_error)?
    {
        return Ok(Value::Null);
    }
    let mut rest = args.pass_from(1);
    if let Some(object) = args.single_object(1) {
        // `array( &$this )`, which the Hook API unwraps (PHP4-style passing): given as the one-element array it is,
        // so it can see inside.
        rest[0] = Value::Array(vec![(Key::Int(0), object)]);
    }
    let message = DoAction { hook, args: rest };
    synced(&s, || command(&s.handlers.do_action, message)).map(|()| Value::Null)
}

fn n_do_action_ref_array(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let message = DoActionRefArray {
        hook: hook_name(args, 0)?,
        args: args.items(1),
    };
    synced(&s, || command(&s.handlers.do_action_ref_array, message)).map(|()| Value::Null)
}

fn n_apply_filters_deprecated(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let message = ApplyFiltersDeprecated {
        hook: HookName::new(arg(args, 0)),
        args: values_of(&arg(args, 1)),
        version: arg(args, 2),
        replacement: arg(args, 3),
        message: arg(args, 4),
    };
    command(&s.handlers.apply_filters_deprecated, message)
}

fn n_do_action_deprecated(args: &[Value]) -> Result<Value, NativeError> {
    let s = state()?;
    let message = DoActionDeprecated {
        hook: HookName::new(arg(args, 0)),
        args: values_of(&arg(args, 1)),
        version: arg(args, 2),
        replacement: arg(args, 3),
        message: arg(args, 4),
    };
    command(&s.handlers.do_action_deprecated, message).map(|()| Value::Null)
}

fn n_call_all_hook(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let Some(all) = s
        .api
        .store()
        .hook(&HookKey::new(ArrayKey::Str("all".into())))
    else {
        return Err(NativeError::throw(
            "Error",
            "Call to a member function do_all_hook() on null",
        ));
    };
    synced(&s, || all.do_all_hook(&args.items(0))).map_err(hook_error)?;
    Ok(Value::Null)
}

fn n_unique_id(args: &[Value]) -> Result<Value, NativeError> {
    Ok(unique_id(&arg(args, 0)).map_or(Value::Null, Value::Str))
}

// ----- WP_Hook

fn hook_arg(args: &[Value]) -> Result<WpHook, NativeError> {
    match args.first() {
        Some(Value::Int(n)) => hook_by_id(*n),
        _ => Err(NativeError::throw("Error", "not a WP_Hook")),
    }
}

/// The hook of a raw function's first argument, a `WP_Hook`'s id.
fn raw_hook_arg(args: &Args<'_>) -> Result<WpHook, NativeError> {
    match args.int(0) {
        Some(n) => hook_by_id(n),
        None => Err(NativeError::throw("Error", "not a WP_Hook")),
    }
}

fn hook_by_id(id: i64) -> Result<WpHook, NativeError> {
    registry::hook(id as usize)
        .ok_or_else(|| NativeError::throw("Error", "this WP_Hook belongs to an earlier request"))
}

fn partition() -> Partition {
    state().map_or(Partition::Request, |s| s.api.store().partition())
}

fn n_hook_new(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Int(registry::register_new(arg(args, 0)) as i64))
}

fn n_hook_clone(args: &[Value]) -> Result<Value, NativeError> {
    let original = match arg(args, 1) {
        Value::Int(n) => n as usize,
        _ => return Err(NativeError::throw("Error", "not a WP_Hook")),
    };
    registry::register_clone(arg(args, 0), original)
        .map(|id| Value::Int(id as i64))
        .ok_or_else(|| NativeError::throw("Error", "this WP_Hook belongs to an earlier request"))
}

fn n_hook_add_filter(args: &Args<'_>) -> Result<Value, NativeError> {
    let hook = raw_hook_arg(args)?;
    hook.add_filter(
        php_callback_of(args.value(2)?, args.pass(2)),
        &args.value(3)?,
        args.int(4).unwrap_or(0),
        partition(),
    )
    .map_err(hook_error)?;
    Ok(Value::Null)
}

fn n_hook_remove_filter(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let id = unique_id(&arg(args, 2));
    hook.remove_filter(id.as_deref(), &arg(args, 3))
        .map(Value::Bool)
        .map_err(hook_error)
}

fn n_hook_has_filter(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let callback = arg(args, 2);
    let callback = (!is_false(&callback)).then(|| callback::callback_id(&callback));
    let priority = match arg(args, 3) {
        Value::Int(n) => Some(n),
        _ => None,
    };
    Ok(has_filter_value(
        hook.has_filter(callback.as_ref().map(|id| &id.0), priority),
    ))
}

fn n_hook_has_filters(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Bool(hook_arg(args)?.has_filters()))
}

fn n_hook_remove_all_filters(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let priority = arg(args, 1);
    hook.remove_all_filters((!is_false(&priority)).then_some(&priority))
        .map_err(hook_error)?;
    Ok(Value::Null)
}

fn n_hook_apply_filters(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = raw_hook_arg(args)?;
    synced(&s, || hook.apply_filters(args.pass(1), args.items(2))).map_err(hook_error)
}

fn n_hook_do_action(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = raw_hook_arg(args)?;
    synced(&s, || hook.do_action(args.items(1))).map_err(hook_error)?;
    Ok(Value::Null)
}

fn n_hook_do_all_hook(args: &Args<'_>) -> Result<Value, NativeError> {
    let s = state()?;
    let hook = raw_hook_arg(args)?;
    synced(&s, || hook.do_all_hook(&args.items(1))).map_err(hook_error)?;
    Ok(Value::Null)
}

fn n_hook_current_priority(args: &[Value]) -> Result<Value, NativeError> {
    Ok(hook_arg(args)?
        .current_priority()
        .map_or(Value::Bool(false), |p| p.to_value()))
}

/// One priority's callbacks as PHP sees them: id => `['function' => .., 'accepted_args' => ..]`.
fn slice_value(slice: &PrioritySlice) -> Value {
    Value::Array(
        slice
            .iter()
            .map(|(id, entry)| {
                let fields = vec![
                    (Key::Str("function".into()), entry.callback.value()),
                    (
                        Key::Str("accepted_args".into()),
                        Value::Int(entry.accepted_args),
                    ),
                ];
                (key_value(id), Value::Array(fields))
            })
            .collect(),
    )
}

fn callbacks_value(hook: &WpHook) -> Value {
    let state = hook.state();
    Value::Array(
        state
            .callbacks
            .iter()
            .map(|(p, slice)| (key_value(p), slice_value(slice)))
            .collect(),
    )
}

/// A priority's callbacks from PHP (`offsetSet()` or a write to `callbacks`).
/// A priority's callbacks from PHP (`offsetSet()` or a write to `callbacks`). An entry whose function is the one
/// `existing` already holds under the same id keeps its callback (a Rust callback stays Rust).
fn slice_from(
    value: &Value,
    existing: Option<&PrioritySlice>,
) -> Result<PrioritySlice, NativeError> {
    let Value::Array(entries) = value else {
        return Err(NativeError::throw(
            "TypeError",
            "WP_Hook callbacks of one priority must be an array",
        ));
    };
    let mut slice = PrioritySlice::new();
    for (id, entry) in entries {
        let Value::Array(fields) = entry else {
            return Err(NativeError::throw(
                "TypeError",
                "a WP_Hook callback must be an array",
            ));
        };
        let field = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| *k == Key::Str(name.into()))
                .map(|(_, v)| v.clone())
        };
        let function = field("function").unwrap_or(Value::Null);
        let accepted_args = match field("accepted_args") {
            Some(Value::Int(n)) => n,
            _ => 0,
        };
        let id = match id {
            Key::Int(n) => ArrayKey::Int(*n),
            Key::Str(s) => ArrayKey::Str(s.clone()),
        };
        let kept = existing
            .and_then(|slice| slice.get(&id))
            .filter(|entry| entry.callback.value() == function);
        let entry = match kept {
            Some(entry) => Entry {
                accepted_args,
                ..entry.clone()
            },
            None => Entry {
                callback: php_callback(function),
                accepted_args,
                partition: partition(),
            },
        };
        slice.insert(id, entry);
    }
    Ok(slice)
}

fn n_hook_callbacks(args: &[Value]) -> Result<Value, NativeError> {
    Ok(callbacks_value(&hook_arg(args)?))
}

fn n_hook_replace_callbacks(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let previous = hook.state().callbacks.clone();
    for priority in previous.keys() {
        hook.unset_priority(priority);
    }
    if let Value::Array(entries) = arg(args, 1) {
        for (priority, slice) in entries {
            let priority = match priority {
                Key::Int(n) => ArrayKey::Int(n),
                Key::Str(s) => ArrayKey::Str(s),
            };
            let slice = slice_from(&slice, previous.get(&priority).map(|s| &**s))?;
            hook.set_priority(Some(priority), slice);
        }
    }
    Ok(Value::Null)
}

fn n_hook_priorities(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let keys: Vec<Value> = hook
        .state()
        .callbacks
        .keys()
        .map(ArrayKey::to_value)
        .collect();
    Ok(list(keys))
}

fn n_hook_offset_exists(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let offset = key(&arg(args, 1))?;
    Ok(Value::Bool(hook.state().callbacks.contains_key(&offset)))
}

fn n_hook_offset_get(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let offset = key(&arg(args, 1))?;
    Ok(hook
        .state()
        .callbacks
        .get(&offset)
        .map_or(Value::Null, |slice| slice_value(slice)))
}

fn n_hook_offset_set(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    let offset = match arg(args, 1) {
        Value::Null => None,
        offset => Some(key(&offset)?),
    };
    let existing = offset
        .as_ref()
        .and_then(|p| hook.state().callbacks.get(p).cloned());
    let slice = slice_from(&arg(args, 2), existing.as_deref())?;
    hook.set_priority(offset, slice);
    Ok(Value::Null)
}

fn n_hook_offset_unset(args: &[Value]) -> Result<Value, NativeError> {
    let hook = hook_arg(args)?;
    hook.unset_priority(&key(&arg(args, 1))?);
    Ok(Value::Null)
}

// ----- the hook globals

enum Global {
    Filter,
    Counts(Count),
    Current,
}

fn global(args: &[Value]) -> Result<Global, NativeError> {
    match args.first() {
        Some(Value::Str(kind)) if kind == "filter" => Ok(Global::Filter),
        Some(Value::Str(kind)) if kind == "actions" => Ok(Global::Counts(Count::Actions)),
        Some(Value::Str(kind)) if kind == "filters" => Ok(Global::Counts(Count::Filters)),
        Some(Value::Str(kind)) if kind == "current" => Ok(Global::Current),
        _ => Err(NativeError::throw("Error", "unknown hook global")),
    }
}

/// The hook store, for a PHP callback's mirror of `$wp_current_filter`.
fn store_for_callbacks() -> Option<Rc<dyn HookStore>> {
    state().ok().map(|s| s.api.store().clone())
}

fn store() -> Result<Rc<dyn HookStore>, NativeError> {
    Ok(state()?.api.store().clone())
}

fn index(value: &Value) -> Option<usize> {
    match value {
        Value::Int(n) if *n >= 0 => Some(*n as usize),
        _ => None,
    }
}

fn n_global_has(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let offset = arg(args, 1);
    Ok(Value::Bool(match global(args)? {
        Global::Filter => store.has_hook(&hook_key(&offset)?),
        Global::Counts(kind) => store.count(kind, &hook_key(&offset)?).is_some(),
        Global::Current => index(&offset).is_some_and(|i| i < store.current_stack().len()),
    }))
}

fn n_global_get(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let offset = arg(args, 1);
    match global(args)? {
        Global::Filter => match store.hook(&hook_key(&offset)?) {
            Some(hook) => {
                registry::object_for(&hook).map_err(|e| NativeError::throw("Error", e.to_string()))
            }
            None => Ok(Value::Null),
        },
        Global::Counts(kind) => Ok(store
            .count(kind, &hook_key(&offset)?)
            .map_or(Value::Null, Value::Int)),
        Global::Current => Ok(index(&offset)
            .and_then(|i| store.current_stack().get(i).cloned())
            .unwrap_or(Value::Null)),
    }
}

/// A count written from PHP, as an integer.
fn count_of(value: &Value) -> Result<i64, NativeError> {
    match value {
        Value::Int(n) => Ok(*n),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Float(f) => Ok(*f as i64),
        Value::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| NativeError::throw("TypeError", "a hook count must be an integer")),
        _ => Err(NativeError::throw(
            "TypeError",
            "a hook count must be an integer",
        )),
    }
}

fn next_int_key(keys: impl Iterator<Item = ArrayKey>) -> ArrayKey {
    let max = keys.filter_map(|k| match k {
        ArrayKey::Int(n) => Some(n),
        ArrayKey::Str(_) => None,
    });
    ArrayKey::Int(max.max().map_or(0, |n| n.saturating_add(1)).max(0))
}

fn n_global_set(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let (offset, value) = (arg(args, 1), arg(args, 2));
    match global(args)? {
        Global::Filter => {
            let Some(hook) = registry::hook_of(&value) else {
                return Err(NativeError::throw(
                    "TypeError",
                    "$wp_filter holds only WP_Hook objects",
                ));
            };
            let name = match offset {
                Value::Null => next_int_key(store.hooks().into_iter().map(|(k, _)| k)),
                offset => key(&offset)?,
            };
            store.insert(HookKey::new(name), hook);
        }
        Global::Counts(kind) => {
            let name = match offset {
                Value::Null => next_int_key(store.counts(kind).into_iter().map(|(k, _)| k)),
                offset => key(&offset)?,
            };
            store.set_count(kind, &HookKey::new(name), Some(count_of(&value)?));
        }
        Global::Current => {
            let mut stack = store.current_stack();
            match index(&offset) {
                Some(i) if i < stack.len() => stack[i] = value,
                _ if matches!(offset, Value::Null) => stack.push(value),
                Some(i) if i == stack.len() => stack.push(value),
                _ => {
                    return Err(NativeError::throw(
                        "TypeError",
                        "$wp_current_filter is a list",
                    ));
                }
            }
            store.set_current_stack(stack);
        }
    }
    Ok(Value::Null)
}

fn n_global_unset(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let offset = arg(args, 1);
    match global(args)? {
        Global::Filter => store.remove(&hook_key(&offset)?),
        Global::Counts(kind) => store.set_count(kind, &hook_key(&offset)?, None),
        Global::Current => {
            let mut stack = store.current_stack();
            if let Some(i) = index(&offset).filter(|i| *i < stack.len()) {
                stack.remove(i);
                store.set_current_stack(stack);
            }
        }
    }
    Ok(Value::Null)
}

fn n_global_all(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    Ok(match global(args)? {
        Global::Filter => {
            let mut entries = Vec::new();
            for (name, hook) in store.hooks() {
                let object = registry::object_for(&hook)
                    .map_err(|e| NativeError::throw("Error", e.to_string()))?;
                entries.push((key_value(&name), object));
            }
            Value::Array(entries)
        }
        Global::Counts(kind) => Value::Array(
            store
                .counts(kind)
                .into_iter()
                .map(|(k, n)| (key_value(&k), Value::Int(n)))
                .collect(),
        ),
        Global::Current => list(store.current_stack()),
    })
}

fn n_global_count(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let count = match global(args)? {
        Global::Filter => store.hooks().len(),
        Global::Counts(kind) => store.counts(kind).len(),
        Global::Current => store.current_stack().len(),
    };
    Ok(Value::Int(count as i64))
}

// ----- testing

/// A saved hook store.
struct Snapshot {
    hooks: Vec<(ArrayKey, WpHook)>,
    actions: Vec<(ArrayKey, i64)>,
    filters: Vec<(ArrayKey, i64)>,
    current: Vec<Value>,
}

thread_local! {
    static SNAPSHOTS: std::cell::RefCell<Vec<Snapshot>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn n_testing_snapshot(_args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let snapshot = Snapshot {
        hooks: store
            .hooks()
            .into_iter()
            .map(|(name, hook)| (name, hook.duplicate()))
            .collect(),
        actions: store.counts(Count::Actions),
        filters: store.counts(Count::Filters),
        current: store.current_stack(),
    };
    let id = SNAPSHOTS.with(|s| {
        let mut s = s.borrow_mut();
        s.push(snapshot);
        s.len() - 1
    });
    Ok(Value::Int(id as i64))
}

fn n_testing_restore(args: &[Value]) -> Result<Value, NativeError> {
    let store = store()?;
    let id = match arg(args, 0) {
        Value::Int(n) => n as usize,
        _ => return Err(NativeError::throw("Error", "not a snapshot")),
    };
    let restored = SNAPSHOTS.with(|s| {
        s.borrow().get(id).map(|snapshot| {
            let hooks: Vec<(ArrayKey, WpHook)> = snapshot
                .hooks
                .iter()
                .map(|(name, hook)| (name.clone(), hook.duplicate()))
                .collect();
            (
                hooks,
                snapshot.actions.clone(),
                snapshot.filters.clone(),
                snapshot.current.clone(),
            )
        })
    });
    let Some((hooks, actions, filters, current)) = restored else {
        return Err(NativeError::throw("Error", "no such snapshot"));
    };
    store.clear();
    for (name, hook) in hooks {
        store.insert(HookKey::new(name), hook);
    }
    for (kind, saved) in [(Count::Actions, actions), (Count::Filters, filters)] {
        for (name, _) in store.counts(kind) {
            store.set_count(kind, &HookKey::new(name), None);
        }
        for (name, n) in saved {
            store.set_count(kind, &HookKey::new(name), Some(n));
        }
    }
    store.set_current_stack(current);
    Ok(Value::Null)
}
