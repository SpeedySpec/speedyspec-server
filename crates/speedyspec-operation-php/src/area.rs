//! The native area: the PHP facade, its native functions, and each request's start and end.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value as Json;
use speedyspec_container::Container;
use speedyspec_container::Lifetime;
use speedyspec_operation_contracts::{
    Caller, Category, INTERNAL_TRANSPORT, Identity, OperationError,
};
use speedyspec_operation_dispatch::{OperationHandler, bind_handler};
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value as PhpValue};

use crate::convert::{from_php, to_php};
use crate::descriptor;
use crate::handler::PhpOperationHandler;
use crate::permission;
use crate::sites::{Site, SiteId, SiteKey, Sites};

const FACADE: [(&str, &str); 4] = [
    (
        "speedyspec://operation/schema",
        include_str!("php/schema.php"),
    ),
    (
        "speedyspec://operation/types",
        include_str!("php/types.php"),
    ),
    (
        "speedyspec://operation/internal",
        include_str!("php/internal.php"),
    ),
    (
        "speedyspec://operation/operation",
        include_str!("php/operation.php"),
    ),
];
const NATIVE: &str = "SpeedySpec\\Operation\\Native\\";
pub(crate) const REGISTRATION_EXCEPTION: &str = "SpeedySpec\\Operation\\RegistrationException";
pub(crate) const OPERATION_EXCEPTION: &str = "SpeedySpec\\Operation\\OperationException";
/// The action fired, once per request, the first time the request needs operations.
pub const INIT_HOOK: &str = "wp_operations_init";

/// Why a site could not be made ready: the error, and the PHP exception `wp_operations_init` threw, if any.
pub(crate) struct InitFailure {
    pub error: OperationError,
    pub exception: Option<PhpValue>,
}

pub(crate) struct State {
    pub container: Rc<Container>,
    pub identity: Rc<dyn Identity>,
    site_key: Rc<dyn SiteKey>,
    pub sites: Sites,
    current: RefCell<Option<SiteId>>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
}

pub(crate) fn state() -> Result<Rc<State>, NativeError> {
    STATE.with(|s| s.borrow().clone()).ok_or_else(|| {
        NativeError::throw(REGISTRATION_EXCEPTION, "the operation API is not available")
    })
}

impl State {
    fn current_id(&self) -> SiteId {
        if let Some(id) = self.current.borrow().clone() {
            return id;
        }
        let id = self.site_key.current();
        *self.current.borrow_mut() = Some(id.clone());
        id
    }

    /// The site this request registers into; fires nothing.
    pub fn registering(&self) -> Rc<Site> {
        self.sites.registering(&self.current_id())
    }

    /// The site this request serves, ready: the first time in a request, `wp_operations_init` runs (filling and
    /// closing the registries the first time, re-binding this request's callables after that).
    pub fn site(&self) -> Result<Rc<Site>, InitFailure> {
        let id = self.current_id();
        let site = self.sites.registering(&id);
        if self.sites.mark_initialized(&id) {
            fire_init()?;
            site.close();
        }
        Ok(site)
    }
}

fn fire_init() -> Result<(), InitFailure> {
    let exists = speedyspec_php::call(
        &PhpValue::Str("function_exists".into()),
        &[PhpValue::Str("do_action".into())],
    );
    if !matches!(exists, Ok(PhpValue::Bool(true))) {
        return Ok(());
    }
    speedyspec_php::call_catching(
        &PhpValue::Str("do_action".into()),
        &[PhpValue::Str(INIT_HOOK.into())],
    )
    .map(drop)
    .map_err(|failure| InitFailure {
        error: OperationError::new(
            "operations_init_failed",
            Category::Internal,
            format!("{INIT_HOOK} failed: {}", failure.error),
        ),
        exception: failure.exception,
    })
}

/// The native area that gives PHP the operation API.
pub struct OperationArea {
    state: Rc<State>,
}

impl OperationArea {
    /// The area over `container` (where handlers are bound), checking capabilities through `identity` and telling
    /// sites apart with `site_key`.
    pub fn new(
        container: Rc<Container>,
        identity: Rc<dyn Identity>,
        site_key: Rc<dyn SiteKey>,
    ) -> OperationArea {
        let state = Rc::new(State {
            sites: Sites::new(container.clone()),
            container,
            identity,
            site_key,
            current: RefCell::new(None),
        });
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        OperationArea { state }
    }

    /// The site this request serves, with its registries filled and closed (see [`State::site`]).
    pub fn site(&self) -> Result<Rc<Site>, OperationError> {
        self.state.site().map_err(|failure| failure.error)
    }

    /// The sites this worker serves.
    pub fn sites(&self) -> &Sites {
        &self.state.sites
    }
}

fn registration_failed(name: &str, reasons: &[String]) -> NativeError {
    NativeError::throw(
        REGISTRATION_EXCEPTION,
        format!("{name} could not be registered: {}", reasons.join("; ")),
    )
}

fn descriptor_name(value: &PhpValue) -> String {
    match value {
        PhpValue::Array(items) => items
            .iter()
            .find_map(|(k, v)| match (k, v) {
                (Key::Str(k), PhpValue::Str(name)) if k == "name" => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "an operation".into()),
        _ => "an operation".into(),
    }
}

fn native_register_operation(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let value = args.first().cloned().unwrap_or(PhpValue::Null);
    let name = descriptor_name(&value);
    let site = state.registering();
    let (spec, callable) = descriptor::operation(&value, site.registries.as_ref())
        .map_err(|reasons| registration_failed(&name, &reasons))?;
    site.registries
        .register_operation(spec)
        .map_err(|err| registration_failed(&name, &[err.to_string()]))?;
    if let Some(callable) = callable {
        permission::bind(&name, callable);
    }
    Ok(PhpValue::Null)
}

fn native_register_type(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let value = args.first().cloned().unwrap_or(PhpValue::Null);
    let name = descriptor_name(&value);
    let spec = descriptor::type_spec(&value).map_err(|why| registration_failed(&name, &[why]))?;
    state
        .registering()
        .registries
        .register_type(spec)
        .map_err(|err| registration_failed(&name, &[err.to_string()]))?;
    Ok(PhpValue::Null)
}

fn native_restrict(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let (Some(PhpValue::Str(name)), Some(PhpValue::Array(list))) = (args.first(), args.get(1))
    else {
        return Err(NativeError::throw(
            REGISTRATION_EXCEPTION,
            "wp_operation_transports takes an operation name and a list of transport names",
        ));
    };
    let names = list
        .iter()
        .map(|(_, v)| match v {
            PhpValue::Str(s) => Ok(s.as_str()),
            _ => Err(registration_failed(
                name,
                &["transport names must be strings".into()],
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    state
        .registering()
        .registries
        .restrict(name, &names)
        .map_err(|err| registration_failed(name, &[err.to_string()]))?;
    Ok(PhpValue::Null)
}

fn native_document(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let site = state.site().map_err(init_error)?;
    Ok(json_to_php(&site.registries.document()))
}

/// A failed `wp_operations_init`, for PHP: its own exception rethrown, or a registration exception.
pub(crate) fn init_error(failure: InitFailure) -> NativeError {
    match failure.exception {
        Some(exception) => NativeError::Rethrow(exception),
        None => NativeError::throw(REGISTRATION_EXCEPTION, failure.error.message),
    }
}

fn json_to_php(json: &Json) -> PhpValue {
    match json {
        Json::Null => PhpValue::Null,
        Json::Bool(b) => PhpValue::Bool(*b),
        Json::Number(n) => n.as_i64().map_or_else(
            || PhpValue::Float(n.as_f64().unwrap_or(f64::NAN)),
            PhpValue::Int,
        ),
        Json::String(s) => PhpValue::Str(s.clone()),
        Json::Array(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), json_to_php(v)))
                .collect(),
        ),
        Json::Object(map) => PhpValue::Array(
            map.iter()
                .map(|(k, v)| (Key::Str(k.clone()), json_to_php(v)))
                .collect(),
        ),
    }
}

fn native_can(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let user = match args.first() {
        Some(PhpValue::Int(u)) if *u > 0 => Some(*u as u64),
        _ => None,
    };
    let Some(PhpValue::Str(capability)) = args.get(1) else {
        return Ok(PhpValue::Bool(false));
    };
    let extra = match args.get(2) {
        Some(PhpValue::Array(items)) => items
            .iter()
            .map(|(_, v)| from_php(v))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NativeError::throw(OPERATION_EXCEPTION, e.message))?,
        _ => Vec::new(),
    };
    Ok(PhpValue::Bool(state.identity.can(user, capability, &extra)))
}

/// The PHP `SpeedySpec\Operation\Caller` for `caller`.
pub fn php_caller(caller: &Caller) -> Result<PhpValue, OperationError> {
    let user = caller
        .user
        .and_then(|u| i64::try_from(u).ok())
        .map_or(PhpValue::Null, PhpValue::Int);
    let fields = caller
        .requested_fields
        .as_ref()
        .map_or(PhpValue::Null, |fields| {
            PhpValue::Array(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (Key::Int(i as i64), PhpValue::Str(f.clone())))
                    .collect(),
            )
        });
    speedyspec_php::call(
        &PhpValue::Str("SpeedySpec\\Operation\\Internal\\Callers::make".into()),
        &[user, PhpValue::Str(caller.transport.clone()), fields],
    )
    .map_err(|err| OperationError::new("caller_failed", Category::Internal, err.to_string()))
}

fn native_bind_handler(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let (Some(PhpValue::Str(name)), Some(callable)) = (args.first(), args.get(1)) else {
        return Err(NativeError::throw(
            REGISTRATION_EXCEPTION,
            "Handler::for takes an operation name and a callable",
        ));
    };
    let handler: Rc<dyn OperationHandler> = Rc::new(PhpOperationHandler {
        operation: name.clone(),
        callable: callable.clone(),
    });
    // PHP's bindings last for the request, so a PHP handler replaces a Rust one only until the request ends.
    let _request = state.container.registering(Lifetime::Request);
    bind_handler(&state.container, name, move |_, _| Ok(handler.clone()));
    Ok(PhpValue::Null)
}

fn native_call(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let state = state()?;
    let Some(PhpValue::Str(name)) = args.first() else {
        return Err(NativeError::throw(
            OPERATION_EXCEPTION,
            "the operation name must be a string",
        ));
    };
    let user = match args.get(2) {
        Some(PhpValue::Int(u)) if *u > 0 => Some(*u as u64),
        _ => None,
    };
    let site = state.site().map_err(init_error)?;
    let input = from_php(args.get(1).unwrap_or(&PhpValue::Null)).map_err(operation_error)?;
    let caller = Caller::new(user, INTERNAL_TRANSPORT, state.identity.clone());
    site.dispatcher
        .call(name, input, &caller)
        .map(|output| to_php(&output))
        .map_err(operation_error)
}

/// `error` thrown in PHP as an `OperationException`.
fn operation_error(error: OperationError) -> NativeError {
    let made = speedyspec_php::call(
        &PhpValue::Str("SpeedySpec\\Operation\\Internal\\Callers::exception".into()),
        &[
            PhpValue::Str(error.code.clone()),
            PhpValue::Str(error.category.as_str().to_owned()),
            PhpValue::Str(error.message.clone()),
            to_php(&error.details),
        ],
    );
    match made {
        Ok(exception @ PhpValue::Opaque(_)) => NativeError::Rethrow(exception),
        _ => NativeError::throw(
            OPERATION_EXCEPTION,
            format!("{}: {}", error.code, error.message),
        ),
    }
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for OperationArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 7] = [
            ("bind_handler", native_bind_handler),
            ("call", native_call),
            ("can", native_can),
            ("register_operation", native_register_operation),
            ("register_type", native_register_type),
            ("restrict", native_restrict),
            ("document", native_document),
        ];
        entries
            .into_iter()
            .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
            .collect()
    }

    fn request_start(&self) {
        for (label, source) in FACADE {
            if let Err(err) = speedyspec_php::eval(label, source) {
                eprintln!("speedyspec_wordpress: declaring {label} failed: {err}");
            }
        }
    }

    fn request_end(&self) {
        self.state.sites.end_request();
        *self.state.current.borrow_mut() = None;
        permission::clear();
    }
}
