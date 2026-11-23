//! The native area: `XmlRpc::bind` and the PHP adapter's declaration in each request.

use std::rc::Rc;

use speedyspec_operation_contracts::Value;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport_contracts::BindingStore;
use speedyspec_xmlrpc::XMLRPC;
use speedyspec_xmlrpc_contracts::{XmlRpcBinding, XmlRpcParam};

const FACADE: &str = include_str!("php/xmlrpc.php");
const NATIVE: &str = "SpeedySpec\\XmlRpc\\Native\\";
const REGISTRATION_EXCEPTION: &str = "SpeedySpec\\Operation\\RegistrationException";

thread_local! {
    static BINDINGS: std::cell::RefCell<Option<Rc<dyn BindingStore>>> = const { std::cell::RefCell::new(None) };
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
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for XmlRpcArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 1] = [("bind", native_bind)];
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
}
