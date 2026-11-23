//! The native area: `XmlRpc::bind` and the PHP adapter's declaration in each request.

use std::rc::Rc;

use speedyspec_http::{Message, Method, Response, ServerRequestFactory, StreamFactory};
use speedyspec_http_message::MessageFactory;
use speedyspec_operation_contracts::Value;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_transport_contracts::BindingStore;
use speedyspec_xmlrpc::XMLRPC;
use speedyspec_xmlrpc_contracts::{Fault, XmlRpcBinding, XmlRpcParam};

use crate::XmlRpcService;
use crate::convert::{from_tagged, to_tagged};

const FACADE: &str = include_str!("php/xmlrpc.php");
const TRUNK_SERVER: &str = include_str!("php/trunk-xmlrpc-server.php");
const SHIM_SERVER: &str = include_str!("php/wp-xmlrpc-server.php");
const NATIVE: &str = "SpeedySpec\\XmlRpc\\Native\\";
const REGISTRATION_EXCEPTION: &str = "SpeedySpec\\Operation\\RegistrationException";

thread_local! {
    static BINDINGS: std::cell::RefCell<Option<Rc<dyn BindingStore>>> = const { std::cell::RefCell::new(None) };
    static SERVICE: std::cell::RefCell<Option<Rc<XmlRpcService>>> = const { std::cell::RefCell::new(None) };
}

fn service() -> Result<Rc<XmlRpcService>, NativeError> {
    SERVICE.with(|s| s.borrow().clone()).ok_or_else(|| {
        NativeError::throw(
            REGISTRATION_EXCEPTION,
            "the XML-RPC service is not available",
        )
    })
}

/// A fault as PHP's adapter reads it: `[ 't' => 'fault', 'code' => …, 'message' => … ]`.
fn tagged_fault(fault: &Fault) -> PhpValue {
    PhpValue::Array(vec![
        (Key::Str("t".into()), PhpValue::Str("fault".into())),
        (Key::Str("code".into()), PhpValue::Int(fault.code)),
        (
            Key::Str("message".into()),
            PhpValue::Str(fault.message.clone()),
        ),
    ])
}

/// A method's outcome for PHP: the tagged value, or the tagged fault.
fn tagged_outcome(outcome: Result<speedyspec_operation_contracts::Value, Fault>) -> PhpValue {
    match outcome {
        Ok(value) => to_tagged(&value),
        Err(fault) => tagged_fault(&fault),
    }
}

/// A fault for a service that could not be built.
fn unavailable_fault(error: &speedyspec_operation_contracts::OperationError) -> Fault {
    eprintln!("speedyspec_wordpress: the XML-RPC server is not available: {error}");
    Fault::new(500, "The request could not be processed.")
}

fn params_of(args: &[PhpValue], at: usize) -> Vec<speedyspec_operation_contracts::Value> {
    match args.get(at) {
        Some(PhpValue::Array(items)) => items.iter().map(|(_, v)| from_tagged(v)).collect(),
        _ => Vec::new(),
    }
}

/// A new `wp_xmlrpc_server`: the request's table and server start over.
fn native_reset(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    crate::legacy::reset();
    service()?.reset();
    Ok(PhpValue::Null)
}

/// `[ method => operation ]` for the exposed operations, for the method table.
fn native_operation_methods(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let methods = service()?.operation_methods().unwrap_or_else(|err| {
        eprintln!("speedyspec_wordpress: no operations are served over XML-RPC: {err}");
        Vec::new()
    });
    Ok(PhpValue::Array(
        methods
            .into_iter()
            .map(|(method, operation)| (Key::Str(method), PhpValue::Str(operation)))
            .collect(),
    ))
}

/// `wp_xmlrpc_server::login()`: the user's ID, or `[ code, message ]`.
fn native_login(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let fault = match service()?.server() {
        Ok(server) => match server.login(&arg_text(args, 0), &arg_text(args, 1)) {
            Ok(caller) => {
                return Ok(PhpValue::Int(
                    caller.user.and_then(|u| i64::try_from(u).ok()).unwrap_or(0),
                ));
            }
            Err(fault) => fault,
        },
        Err(error) => unavailable_fault(&error),
    };
    Ok(PhpValue::Array(vec![
        (Key::Int(0), PhpValue::Int(fault.code)),
        (Key::Int(1), PhpValue::Str(fault.message)),
    ]))
}

/// `wp_xmlrpc_server::call()` (LegacyCallerAdapter): any method, through the Rust server.
fn native_call(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let outcome = match service()?.server() {
        Ok(server) => server.call(&arg_text(args, 0), params_of(args, 1)),
        Err(error) => Err(unavailable_fault(&error)),
    };
    Ok(tagged_outcome(outcome))
}

/// An `OperationMethod` entry called from PHP.
fn native_call_operation(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let outcome = match service()?.server() {
        Ok(server) => server.call_operation(&arg_text(args, 0), params_of(args, 1)),
        Err(error) => Err(unavailable_fault(&error)),
    };
    Ok(tagged_outcome(outcome))
}

/// `wp_xmlrpc_server::serve_request()`: `[ status, headers, body ]` for this request's method and body.
fn native_serve(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let request = MessageFactory
        .create_server_request(
            Method::parse(&arg_text(args, 0)),
            "http://localhost/xmlrpc.php",
            vec![],
        )
        .map_err(|e| NativeError::throw("RuntimeException", e.to_string()))?
        .with_body(MessageFactory.create_stream(arg_text(args, 1).as_bytes()));
    let response = service()?
        .serve(&request)
        .map_err(|e| NativeError::throw("RuntimeException", e.to_string()))?;
    let headers = response
        .headers()
        .iter()
        .map(|(name, values)| (Key::Str(name.to_owned()), PhpValue::Str(values.join(", "))))
        .collect();
    let body = response.body().borrow_mut().read_all().unwrap_or_default();
    Ok(PhpValue::Array(vec![
        (
            Key::Int(0),
            PhpValue::Int(i64::from(response.status().as_u16())),
        ),
        (Key::Int(1), PhpValue::Array(headers)),
        (
            Key::Int(2),
            PhpValue::Str(String::from_utf8_lossy(&body).into_owned()),
        ),
    ]))
}

/// Declares `SpeedySpec_TrunkXmlRpcServer` and the `wp_xmlrpc_server` shim, once IXR is loaded.
fn native_declare_shim(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let declared = |class: &str| {
        matches!(
            speedyspec_php::call(
                &PhpValue::Str("class_exists".into()),
                &[PhpValue::Str(class.into()), PhpValue::Bool(false)],
            ),
            Ok(PhpValue::Bool(true))
        )
    };
    if !declared("IXR_Server") {
        return Ok(PhpValue::Bool(false));
    }
    for (class, label, source) in [
        (
            "SpeedySpec_TrunkXmlRpcServer",
            "speedyspec://xmlrpc/trunk-server",
            TRUNK_SERVER,
        ),
        (
            "wp_xmlrpc_server",
            "speedyspec://xmlrpc/server",
            SHIM_SERVER,
        ),
    ] {
        if !declared(class)
            && let Err(err) = speedyspec_php::eval(label, source)
        {
            eprintln!("speedyspec_wordpress: declaring {label} failed: {err}");
            return Ok(PhpValue::Bool(false));
        }
    }
    Ok(PhpValue::Bool(true))
}

fn arg_text(args: &[PhpValue], at: usize) -> String {
    match args.get(at) {
        Some(PhpValue::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// `XmlRpc::bind( $operation )->method( $name )[->params( … )]`. With no parameters (`null`) the binding keeps the
/// transport's default parameters.
fn native_bind(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let store = BINDINGS.with(|b| b.borrow().clone()).ok_or_else(|| {
        NativeError::throw(REGISTRATION_EXCEPTION, "XML-RPC bindings are not available")
    })?;
    let operation = arg_text(args, 0);
    let method = arg_text(args, 1);
    let params: Option<Vec<XmlRpcParam>> = match args.get(2) {
        Some(PhpValue::Array(items)) => Some(
            items
                .iter()
                .map(|(_, v)| match v {
                    PhpValue::Str(name) => XmlRpcParam::parse(name),
                    _ => XmlRpcParam::Rest,
                })
                .collect(),
        ),
        _ => None,
    };
    let binding = XmlRpcBinding {
        method: method.clone(),
        params: params.clone().unwrap_or_default(),
    };
    let checked = XmlRpcBinding::from_value(&binding.to_value()).map_err(|why| {
        NativeError::throw(
            REGISTRATION_EXCEPTION,
            format!("{operation} cannot be bound on XML-RPC: {why}"),
        )
    })?;
    let value = match params {
        Some(_) => checked.to_value(),
        None => Value::map([("method", Value::String(checked.method))]),
    };
    store.bind(&operation, XMLRPC, value);
    Ok(PhpValue::Null)
}

/// The native area that gives PHP `XmlRpc::bind` and the XML-RPC adapter.
pub struct XmlRpcArea;

impl XmlRpcArea {
    /// The area writing `XmlRpc::bind` bindings to `bindings`.
    pub fn new(bindings: Rc<dyn BindingStore>) -> XmlRpcArea {
        BINDINGS.with(|b| *b.borrow_mut() = Some(bindings));
        XmlRpcArea
    }

    /// The service the shim's natives reach the XML-RPC server through.
    pub fn with_service(self, service: Rc<XmlRpcService>) -> Self {
        SERVICE.with(|s| *s.borrow_mut() = Some(service));
        self
    }
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for XmlRpcArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 8] = [
            ("bind", native_bind),
            ("reset", native_reset),
            ("operation_methods", native_operation_methods),
            ("login", native_login),
            ("call", native_call),
            ("call_operation", native_call_operation),
            ("serve", native_serve),
            ("declare_shim", native_declare_shim),
        ];
        entries
            .into_iter()
            .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
            .collect()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://xmlrpc", FACADE) {
            eprintln!("speedyspec_wordpress: declaring SpeedySpec\\XmlRpc failed: {err}");
        }
    }

    fn request_end(&self) {
        crate::legacy::reset();
        if let Some(service) = SERVICE.with(|s| s.borrow().clone()) {
            service.reset();
        }
    }
}
