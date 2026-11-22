//! The native area: `Rest::bind`, the proxies' native functions, and each request's start and end.

use std::rc::Rc;

use speedyspec_operation_contracts::Value;
use speedyspec_operation_php::convert::{from_php, to_php};
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_rest::REST;
use speedyspec_rest_contracts::{RestBinding, RestMethod, header_key};
use speedyspec_transport_contracts::BindingStore;

use crate::handles;

const FACADE: &str = include_str!("php/rest.php");
const PROXIES: &str = include_str!("php/proxies.php");
const NATIVE: &str = "SpeedySpec\\Rest\\Native\\";
const REGISTRATION_EXCEPTION: &str = "SpeedySpec\\Operation\\RegistrationException";

thread_local! {
    static SERVICE: std::cell::RefCell<Option<Rc<crate::RestService>>> = const { std::cell::RefCell::new(None) };
    static BINDINGS: std::cell::RefCell<Option<Rc<dyn BindingStore>>> = const { std::cell::RefCell::new(None) };
}

/// Declares the proxy classes in this request, once WordPress's `WP_REST_Request` exists to extend.
pub(crate) fn ensure_proxies() {
    let exists = |class: &str| {
        matches!(
            speedyspec_php::call(
                &PhpValue::Str("class_exists".into()),
                &[PhpValue::Str(class.into())]
            ),
            Ok(PhpValue::Bool(true))
        )
    };
    if exists("SpeedySpec\\Rest\\RequestProxy") || !exists("WP_REST_Request") {
        return;
    }
    if let Err(err) = speedyspec_php::eval("speedyspec://rest/proxies", PROXIES) {
        eprintln!("speedyspec_wordpress: declaring the REST proxies failed: {err}");
    }
}

fn stale() -> NativeError {
    NativeError::throw(
        "LogicException",
        "the REST request or response this object stood for has ended",
    )
}

fn handle_of(args: &[PhpValue]) -> Result<u64, NativeError> {
    match args.first() {
        Some(PhpValue::Int(h)) if *h > 0 => Ok(*h as u64),
        _ => Err(stale()),
    }
}

fn arg_text(args: &[PhpValue], i: usize) -> String {
    match args.get(i) {
        Some(PhpValue::Str(s)) => s.clone(),
        Some(PhpValue::Int(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn plain(value: Option<&PhpValue>) -> Result<Value, NativeError> {
    from_php(value.unwrap_or(&PhpValue::Null))
        .map_err(|e| NativeError::throw("InvalidArgumentException", e.message))
}

fn map_value(map: &indexmap::IndexMap<String, Value>) -> PhpValue {
    to_php(&Value::Map(map.clone()))
}

fn native_request_get(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let request = handles::request(handle_of(args)?).ok_or_else(stale)?;
    let r = request.borrow();
    Ok(match arg_text(args, 1).as_str() {
        "method" => PhpValue::Str(if r.head {
            "HEAD".into()
        } else {
            r.method.as_str().into()
        }),
        "route" => PhpValue::Str(r.path.clone()),
        "param" => r
            .params()
            .get(&arg_text(args, 2))
            .map_or(PhpValue::Null, to_php),
        "has_param" => PhpValue::Bool(r.params().contains_key(&arg_text(args, 2))),
        "params" => map_value(&r.params()),
        "url_params" => map_value(&r.url_params),
        "query_params" => map_value(&r.query_params),
        "body_params" => map_value(&r.body_params),
        "json_params" => map_value(&r.json_params),
        "header_list" => {
            r.headers
                .get(&header_key(&arg_text(args, 2)))
                .map_or(PhpValue::Null, |values| {
                    PhpValue::Array(
                        values
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (Key::Int(i as i64), PhpValue::Str(v.clone())))
                            .collect(),
                    )
                })
        }
        "header" => r
            .headers
            .get(&header_key(&arg_text(args, 2)))
            .map_or(PhpValue::Null, |values| PhpValue::Str(values.join(","))),
        "headers" => PhpValue::Array(
            r.headers
                .iter()
                .map(|(k, values)| {
                    let list = values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (Key::Int(i as i64), PhpValue::Str(v.clone())))
                        .collect();
                    (Key::Str(k.clone()), PhpValue::Array(list))
                })
                .collect(),
        ),
        "body" => match String::from_utf8(r.body.clone()) {
            Ok(text) => PhpValue::Str(text),
            Err(e) => PhpValue::Bytes(e.into_bytes()),
        },
        other => {
            return Err(NativeError::throw(
                "LogicException",
                format!("unknown request field {other}"),
            ));
        }
    })
}

fn native_request_set(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let request = handles::request(handle_of(args)?).ok_or_else(stale)?;
    let mut guard = request.borrow_mut();
    let r = &mut *guard;
    match arg_text(args, 1).as_str() {
        "method" => {
            if let Some(method) = RestMethod::parse(&arg_text(args, 2)) {
                r.method = method;
            }
        }
        "route" => r.path = arg_text(args, 2),
        "param" => {
            let value = plain(args.get(3))?;
            r.set_param(&arg_text(args, 2), value);
        }
        "unset" => {
            let key = arg_text(args, 2);
            for map in [
                &mut r.url_params,
                &mut r.query_params,
                &mut r.body_params,
                &mut r.json_params,
            ] {
                map.shift_remove(&key);
            }
        }
        "add_header" => {
            let value = arg_text(args, 3);
            r.headers
                .entry(header_key(&arg_text(args, 2)))
                .or_default()
                .push(value);
        }
        "remove_header" => {
            r.headers.shift_remove(&header_key(&arg_text(args, 2)));
        }
        "headers" => {
            if !matches!(args.get(3), Some(PhpValue::Bool(false))) {
                r.headers.clear();
            }
            if let Value::Map(map) = plain(args.get(2))? {
                for (k, v) in map {
                    let values = match v {
                        Value::List(items) => items
                            .iter()
                            .filter_map(|i| match i {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect(),
                        Value::String(s) => vec![s],
                        _ => continue,
                    };
                    r.headers.insert(header_key(&k), values);
                }
            }
        }
        "url_params" | "query_params" | "body_params" => {
            let map = match plain(args.get(2))? {
                Value::Map(map) => map,
                _ => indexmap::IndexMap::new(),
            };
            match arg_text(args, 1).as_str() {
                "url_params" => r.url_params = map,
                "query_params" => r.query_params = map,
                _ => r.body_params = map,
            }
        }
        "body" => r.body = arg_text(args, 2).into_bytes(),
        "header" => {
            let values = match args.get(3) {
                Some(PhpValue::Array(items)) => items
                    .iter()
                    .filter_map(|(_, v)| match v {
                        PhpValue::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => vec![arg_text(args, 3)],
            };
            r.headers.insert(header_key(&arg_text(args, 2)), values);
        }
        other => {
            return Err(NativeError::throw(
                "LogicException",
                format!("unknown request field {other}"),
            ));
        }
    }
    Ok(PhpValue::Null)
}

fn native_response_get(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let response = handles::response(handle_of(args)?).ok_or_else(stale)?;
    let r = response.borrow();
    Ok(match arg_text(args, 1).as_str() {
        "data" => to_php(&r.data),
        "status" => PhpValue::Int(i64::from(r.status)),
        "headers" => PhpValue::Array(
            r.headers
                .iter()
                .map(|(k, v)| (Key::Str(k.clone()), PhpValue::Str(v.clone())))
                .collect(),
        ),
        "links" => {
            let mut by_rel: indexmap::IndexMap<String, Vec<PhpValue>> = indexmap::IndexMap::new();
            for link in &r.links {
                let mut attributes = vec![];
                if link.embeddable {
                    attributes.push((Key::Str("embeddable".into()), PhpValue::Bool(true)));
                }
                by_rel
                    .entry(link.rel.clone())
                    .or_default()
                    .push(PhpValue::Array(vec![
                        (Key::Str("href".into()), PhpValue::Str(link.href.clone())),
                        (Key::Str("attributes".into()), PhpValue::Array(attributes)),
                    ]));
            }
            PhpValue::Array(
                by_rel
                    .into_iter()
                    .map(|(rel, list)| {
                        let list = list
                            .into_iter()
                            .enumerate()
                            .map(|(i, v)| (Key::Int(i as i64), v))
                            .collect();
                        (Key::Str(rel), PhpValue::Array(list))
                    })
                    .collect(),
            )
        }
        other => {
            return Err(NativeError::throw(
                "LogicException",
                format!("unknown response field {other}"),
            ));
        }
    })
}

fn native_bind(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let store = BINDINGS.with(|b| b.borrow().clone()).ok_or_else(|| {
        NativeError::throw(REGISTRATION_EXCEPTION, "REST bindings are not available")
    })?;
    let operation = arg_text(args, 0);
    let methods = match args.get(3) {
        Some(PhpValue::Array(items)) => items
            .iter()
            .map(|(_, v)| plain(Some(v)))
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let value = Value::map([
        ("namespace", Value::from(arg_text(args, 1).as_str())),
        ("route", Value::from(arg_text(args, 2).as_str())),
        ("methods", Value::List(methods)),
    ]);
    let binding = RestBinding::from_value(&value).map_err(|why| {
        NativeError::throw(
            REGISTRATION_EXCEPTION,
            format!("{operation} cannot be bound on REST: {why}"),
        )
    })?;
    store.bind(&operation, REST, binding.to_value());
    Ok(PhpValue::Null)
}

/// The native area that gives PHP `Rest::bind` and the REST proxies.
pub struct RestArea;

impl RestArea {
    /// The area writing `Rest::bind` bindings to `bindings`.
    pub fn new(bindings: Rc<dyn BindingStore>) -> RestArea {
        BINDINGS.with(|b| *b.borrow_mut() = Some(bindings));
        RestArea
    }

    /// The service the shim's natives (`rest_do_request`, legacy routes) reach the REST server through.
    pub fn with_service(self, service: Rc<crate::RestService>) -> Self {
        SERVICE.with(|s| *s.borrow_mut() = Some(service));
        self
    }
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for RestArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 4] = [
            ("request_get", native_request_get),
            ("request_set", native_request_set),
            ("response_get", native_response_get),
            ("bind", native_bind),
        ];
        entries
            .into_iter()
            .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
            .collect()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://rest", FACADE) {
            eprintln!("speedyspec_wordpress: declaring SpeedySpec\\Rest failed: {err}");
        }
    }

    fn request_end(&self) {
        handles::clear();
    }
}
