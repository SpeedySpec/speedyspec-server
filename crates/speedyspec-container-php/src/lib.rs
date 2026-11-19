//! The container from PHP.
//!
//! [`ContainerArea`] adds `SpeedySpec\Container\Container` to PHP: a facade (declared at the start of every request,
//! from `container.php`) over native functions backed by the Rust [`Container`]. Everything PHP registers has
//! [`Lifetime::Request`] and is *foreign*, so a PHP binding of a port overrides the Rust one until the request ends
//! (also after a fatal error: the area's request end runs from the module's RSHUTDOWN). A Rust caller of a port with
//! no PHP binding never enters PHP.
//!
//! PHP values in the container are [`ForeignValue`]s. A PHP object bound to a key registered with
//! [`ContainerArea::port`] is also wrapped in that port's PHP adapter (`Php<Trait>`, from `#[php_port]`), as a
//! [`Bridged`] value, so `make_port::<dyn P>()` gives Rust callers the PHP implementation.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use speedyspec_container::{
    Autowirer, Bridged, Container, ContainerError, Lifetime, Params, Resolved,
};
use speedyspec_php::native::{self, NativeError};
use speedyspec_php::{NativeArea, PhpError};
use speedyspec_php_ext::{Key, PhpObject, Value};

/// The facade's PHP source.
const FACADE: &str = include_str!("container.php");
const EXCEPTION: &str = "SpeedySpec\\Container\\ContainerException";
const NATIVE: &str = "SpeedySpec\\Container\\Native\\";

/// A PHP value held by the container.
pub struct ForeignValue(pub Value);

type Wrap = Rc<dyn Fn(PhpObject) -> Resolved>;
type NativeFn = fn(&[Value]) -> Result<Value, NativeError>;

/// Keys that hold state for the rest of a request once used: PHP may bind them only before their first resolution
/// in a request (or for every request, in `app.<id>.php.bindings`).
#[derive(Default)]
pub struct StatefulPorts(RefCell<HashSet<String>>);

impl StatefulPorts {
    pub fn add(&self, key: &str) {
        self.0.borrow_mut().insert(key.to_string());
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.borrow().contains(key)
    }
}

struct State {
    container: Rc<Container>,
    app_id: String,
    ports: RefCell<HashMap<String, Wrap>>,
    stateful: StatefulPorts,
    bindings: RefCell<Vec<(String, String)>>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
}

fn state() -> Result<Rc<State>, NativeError> {
    STATE
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw(EXCEPTION, "the container is not available"))
}

/// The native area that gives PHP the container.
pub struct ContainerArea {
    state: Rc<State>,
}

impl ContainerArea {
    /// The container area of the app `app_id`, over `container`; also makes [`PhpAutowirer`] its autowirer.
    pub fn new(app_id: &str, container: Rc<Container>) -> ContainerArea {
        container.set_autowirer(Rc::new(PhpAutowirer));
        let state = Rc::new(State {
            container,
            app_id: app_id.to_string(),
            ports: RefCell::new(HashMap::new()),
            stateful: StatefulPorts::default(),
            bindings: RefCell::new(Vec::new()),
        });
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        ContainerArea { state }
    }

    /// `key` is the port `P`: a PHP object bound to it reaches Rust callers through `wrap` (its PHP adapter).
    pub fn port<P: ?Sized + 'static>(
        self,
        key: &str,
        wrap: impl Fn(PhpObject) -> Rc<P> + 'static,
    ) -> Self {
        let wrap: Wrap = Rc::new(move |object| Rc::new(wrap(object)) as Resolved);
        self.state.ports.borrow_mut().insert(key.to_string(), wrap);
        self
    }

    /// `key` holds state (see [`StatefulPorts`]).
    pub fn stateful(self, key: &str) -> Self {
        self.state.stateful.add(key);
        self
    }

    /// `app.<id>.php.bindings`: `(key, class)` pairs bound at the start of every request, before any PHP code.
    pub fn bindings(self, bindings: Vec<(String, String)>) -> Self {
        *self.state.bindings.borrow_mut() = bindings;
        self
    }
}

impl NativeArea for ContainerArea {
    fn functions(&self) -> Vec<speedyspec_php::ext_php_rs::builders::FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 14] = [
            ("bind", native_bind),
            ("instance", native_instance),
            ("make", native_make),
            ("has", native_has),
            ("resolved", native_resolved),
            ("alias", native_alias),
            ("is_alias", native_is_alias),
            ("get_alias", native_get_alias),
            ("extend", native_extend),
            ("give", native_give),
            ("tag", native_tag),
            ("tagged", native_tagged),
            ("on", native_on),
            ("forget", native_forget),
        ];
        entries
            .into_iter()
            .map(|(name, handler)| native::function(&format!("{NATIVE}{name}"), handler))
            .collect()
    }

    fn request_start(&self) {
        self.state.container.start_request();
        if let Err(err) = speedyspec_php::eval("speedyspec://container", FACADE) {
            eprintln!(
                "speedyspec_wordpress: declaring SpeedySpec\\Container\\Container failed: {err}"
            );
        }
        let bindings = self.state.bindings.borrow().clone();
        install_php_bindings(&bindings);
    }

    fn request_end(&self) {
        self.state.container.end_request();
    }
}

/// Binds each `(key, class)` for this request, shared (built once, by autowiring `class`), as PHP bindings. Runs no PHP code, so it
/// can run before any.
pub fn install_php_bindings(bindings: &[(String, String)]) {
    let Ok(state) = state() else {
        return;
    };
    let container = &state.container;
    let _request = container.registering(Lifetime::Request);
    for (key, class) in bindings {
        container.singleton_foreign(key, class_factory(key, class));
    }
}

/// Builds PHP classes the container has no binding for, through `ReflectionClass`, resolving each constructor
/// parameter from the container.
pub struct PhpAutowirer;

impl Autowirer for PhpAutowirer {
    fn build(
        &self,
        key: &str,
        _container: &Container,
        params: &Params,
    ) -> Option<Result<Resolved, ContainerError>> {
        let exists = speedyspec_php::call(
            &Value::Str("class_exists".into()),
            &[Value::Str(key.into())],
        );
        if !matches!(exists, Ok(Value::Bool(true))) {
            return None;
        }
        Some(autowire(key, key, params))
    }
}

// ----- values

/// `value` as the container holds it under `key`.
fn wrap(key: &str, value: Value) -> Resolved {
    let wrapper = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|state| state.ports.borrow().get(key).cloned())
    });
    match (wrapper, PhpObject::new(value.clone())) {
        (Some(wrap), Some(object)) => Rc::new(Bridged {
            port: wrap(object),
            foreign: Rc::new(ForeignValue(value)),
        }),
        _ => Rc::new(ForeignValue(value)),
    }
}

/// The PHP form of a resolved value, if it has one.
fn to_php(value: &Resolved) -> Option<Value> {
    if let Some(bridged) = value.downcast_ref::<Bridged>() {
        return to_php(&bridged.foreign);
    }
    if let Some(foreign) = value.downcast_ref::<ForeignValue>() {
        return Some(foreign.0.clone());
    }
    value.downcast_ref::<Value>().cloned()
}

fn to_php_or_fail(key: &str, value: &Resolved) -> Result<Value, ContainerError> {
    to_php(value).ok_or_else(|| {
        ContainerError::Unresolvable(
            key.to_string(),
            "it is a Rust value with no PHP form".into(),
        )
    })
}

fn params_to_php(params: &Params) -> Value {
    let mut entries: Vec<(Key, Value)> = params
        .iter()
        .map(|(name, value)| {
            (
                Key::Str(name.to_string()),
                to_php(value).unwrap_or(Value::Null),
            )
        })
        .collect();
    entries.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    Value::Array(entries)
}

fn params_from_php(value: Option<&Value>) -> Params {
    let mut params = Params::new();
    if let Some(Value::Array(entries)) = value {
        for (key, value) in entries {
            let name = match key {
                Key::Str(name) => name.clone(),
                Key::Int(n) => n.to_string(),
            };
            params.insert(&name, Rc::new(ForeignValue(value.clone())));
        }
    }
    params
}

fn container_object() -> Result<Value, PhpError> {
    speedyspec_php::call(
        &Value::Str("SpeedySpec\\Container\\Container::getInstance".into()),
        &[],
    )
}

fn php_failure(key: &str, err: PhpError) -> ContainerError {
    let why = match err {
        PhpError::Exception { message, .. } => message,
        other => other.to_string(),
    };
    ContainerError::Unresolvable(key.to_string(), why)
}

// ----- factories

type Factory = Box<dyn Fn(&Container, &Params) -> Result<Resolved, ContainerError>>;

/// Calls the PHP closure `closure($container, $params)`.
fn closure_factory(key: &str, closure: Value) -> Factory {
    let key = key.to_string();
    Box::new(move |_, params| {
        let container = container_object().map_err(|e| php_failure(&key, e))?;
        let value = speedyspec_php::call(&closure, &[container, params_to_php(params)])
            .map_err(|e| php_failure(&key, e))?;
        Ok(wrap(&key, value))
    })
}

/// Builds `class`: autowired when it is `key` itself, otherwise resolved (so its own bindings apply).
fn class_factory(key: &str, class: &str) -> Factory {
    let (key, class) = (key.to_string(), class.to_string());
    Box::new(move |container, params| {
        if class == key {
            return autowire(&key, &class, params);
        }
        let value = container.make_with(&class, params)?;
        let php = to_php_or_fail(&class, &value)?;
        Ok(wrap(&key, php))
    })
}

fn autowire(key: &str, class: &str, params: &Params) -> Result<Resolved, ContainerError> {
    let value = speedyspec_php::call(
        &Value::Str("SpeedySpec\\Container\\autowire".into()),
        &[Value::Str(class.to_string()), params_to_php(params)],
    )
    .map_err(|e| php_failure(key, e))?;
    Ok(wrap(key, value))
}

fn factory_for(key: &str, concrete: &Value) -> Result<Factory, NativeError> {
    match concrete {
        Value::Str(class) => Ok(class_factory(key, class)),
        Value::Opaque(_) => Ok(closure_factory(key, concrete.clone())),
        _ => Err(NativeError::throw(
            EXCEPTION,
            format!("{key}: a binding needs a closure or a class name"),
        )),
    }
}

// ----- native functions

fn fail(err: ContainerError) -> NativeError {
    NativeError::throw(EXCEPTION, err.to_string())
}

fn string_arg(args: &[Value], i: usize, what: &str) -> Result<String, NativeError> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        _ => Err(NativeError::throw(
            EXCEPTION,
            format!("the {what} must be a string"),
        )),
    }
}

fn optional_string(args: &[Value], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Refuses to rebind a stateful key already used in this request.
fn refuse_stateful(state: &State, key: &str) -> Result<(), NativeError> {
    let key = state.container.get_alias(key);
    if state.stateful.contains(&key) && state.container.resolved_in_request(&key) {
        return Err(NativeError::throw(
            EXCEPTION,
            format!(
                "{key} holds state and was already used in this request; bind it in app.{}.php.bindings instead",
                state.app_id
            ),
        ));
    }
    Ok(())
}

/// `bind(key, concrete, sharing)`: sharing 0 transient, 1 singleton, 2 scoped.
fn native_bind(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let key = string_arg(args, 0, "key")?;
    refuse_stateful(&state, &key)?;
    let factory = factory_for(&key, args.get(1).unwrap_or(&Value::Null))?;
    let c = &state.container;
    let _request = c.registering(Lifetime::Request);
    match args.get(2) {
        Some(Value::Int(1)) => c.singleton_foreign(&key, factory),
        Some(Value::Int(2)) => c.scoped_foreign(&key, factory),
        _ => c.bind_foreign(&key, factory),
    }
    Ok(Value::Null)
}

fn native_instance(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let key = string_arg(args, 0, "key")?;
    refuse_stateful(&state, &key)?;
    let value = args.get(1).cloned().unwrap_or(Value::Null);
    let _request = state.container.registering(Lifetime::Request);
    state.container.instance_foreign(&key, wrap(&key, value));
    Ok(Value::Null)
}

fn native_make(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let key = string_arg(args, 0, "key")?;
    let params = params_from_php(args.get(1));
    let value = state.container.make_with(&key, &params).map_err(fail)?;
    to_php_or_fail(&key, &value).map_err(fail)
}

fn native_has(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Bool(
        state()?.container.has(&string_arg(args, 0, "key")?),
    ))
}

fn native_resolved(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Bool(
        state()?.container.resolved(&string_arg(args, 0, "key")?),
    ))
}

fn native_alias(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let (key, alias) = (string_arg(args, 0, "key")?, string_arg(args, 1, "alias")?);
    let _request = state.container.registering(Lifetime::Request);
    state.container.try_alias(&key, &alias).map_err(fail)?;
    Ok(Value::Null)
}

fn native_is_alias(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Bool(
        state()?.container.is_alias(&string_arg(args, 0, "name")?),
    ))
}

fn native_get_alias(args: &[Value]) -> Result<Value, NativeError> {
    Ok(Value::Str(
        state()?.container.get_alias(&string_arg(args, 0, "key")?),
    ))
}

/// `extend(key, closure)`: `closure($value, $container)` replaces the value.
fn native_extend(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let key = string_arg(args, 0, "key")?;
    refuse_stateful(&state, &key)?;
    let closure = args.get(1).cloned().unwrap_or(Value::Null);
    let _request = state.container.registering(Lifetime::Request);
    let extended = key.clone();
    state.container.extend(&key, move |value, _| {
        let php = to_php_or_fail(&extended, &value)?;
        let container = container_object().map_err(|e| php_failure(&extended, e))?;
        let result = speedyspec_php::call(&closure, &[php, container])
            .map_err(|e| php_failure(&extended, e))?;
        Ok(wrap(&extended, result))
    });
    Ok(Value::Null)
}

/// `give(consumer, need, implementation)`: a class name resolves that key, a closure builds the value.
fn native_give(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let (consumer, need) = (
        string_arg(args, 0, "consumer")?,
        string_arg(args, 1, "needed key")?,
    );
    let _request = state.container.registering(Lifetime::Request);
    match args.get(2) {
        Some(Value::Str(other)) => state.container.when(&consumer).needs(&need).give_key(other),
        Some(closure @ Value::Opaque(_)) => state
            .container
            .when(&consumer)
            .needs(&need)
            .give(closure_factory(&need, closure.clone())),
        _ => {
            return Err(NativeError::throw(
                EXCEPTION,
                format!("{need}: give() needs a closure or a class name"),
            ));
        }
    }
    Ok(Value::Null)
}

fn native_tag(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let tag = string_arg(args, 1, "tag")?;
    let keys: Vec<String> = match args.first() {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|(_, v)| match v {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let _request = state.container.registering(Lifetime::Request);
    state.container.tag(&keys, &tag);
    Ok(Value::Null)
}

fn native_tagged(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let tag = string_arg(args, 0, "tag")?;
    let values = state.container.tagged(&tag).map_err(fail)?;
    let mut entries = Vec::with_capacity(values.len());
    for (i, value) in values.iter().enumerate() {
        entries.push((
            Key::Int(i as i64),
            to_php_or_fail(&tag, value).map_err(fail)?,
        ));
    }
    Ok(Value::Array(entries))
}

/// `on(kind, key|null, closure)`: kind 0 resolving, 1 after resolving, 2 before resolving.
fn native_on(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let key = optional_string(args, 1);
    let closure = args.get(2).cloned().unwrap_or(Value::Null);
    let c = &state.container;
    let _request = c.registering(Lifetime::Request);
    let on_value = move |value: &Resolved, _: &Container| {
        let php = to_php(value).unwrap_or(Value::Null);
        if let Ok(container) = container_object() {
            let _ = speedyspec_php::call(&closure, &[php, container]);
        }
    };
    match args.first() {
        Some(Value::Int(0)) => c.resolving(key.as_deref(), on_value),
        Some(Value::Int(1)) => c.after_resolving(key.as_deref(), on_value),
        _ => {
            let closure = args.get(2).cloned().unwrap_or(Value::Null);
            c.before_resolving(key.as_deref(), move |key, _| {
                if let Ok(container) = container_object() {
                    let _ =
                        speedyspec_php::call(&closure, &[Value::Str(key.to_string()), container]);
                }
            });
        }
    }
    Ok(Value::Null)
}

/// `forget(kind, key|null)`: kind 0 one instance, 1 every instance, 2 scoped instances, 3 everything PHP registered.
fn native_forget(args: &[Value]) -> Result<Value, NativeError> {
    let state = state()?;
    let c = &state.container;
    match args.first() {
        Some(Value::Int(0)) => c.forget_instance(&string_arg(args, 1, "key")?),
        Some(Value::Int(1)) => c.forget_instances(),
        Some(Value::Int(2)) => c.forget_scoped_instances(),
        _ => c.end_request(),
    }
    Ok(Value::Null)
}
