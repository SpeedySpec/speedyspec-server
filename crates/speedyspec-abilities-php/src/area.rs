//! The native area: `Abilities::publish` and the PHP adapter's declaration in each request.

use std::rc::Rc;

use speedyspec_abilities::ABILITIES;
use speedyspec_abilities_contracts::AbilityBinding;
use speedyspec_operation_php::convert::from_php;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::Value as PhpValue;
use speedyspec_transport_contracts::BindingStore;

const FACADE: &str = include_str!("php/abilities.php");
const NATIVE: &str = "SpeedySpec\\Abilities\\Native\\";
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

/// `Abilities::publish( $operation )->…`: stores the operation's ability metadata, checked.
fn native_publish(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let store = BINDINGS.with(|b| b.borrow().clone()).ok_or_else(|| {
        NativeError::throw(REGISTRATION_EXCEPTION, "ability bindings are not available")
    })?;
    let operation = arg_text(args, 0);
    let value = args
        .get(1)
        .map(|v| from_php(v).unwrap_or(speedyspec_operation_contracts::Value::Null))
        .unwrap_or(speedyspec_operation_contracts::Value::Null);
    let binding = AbilityBinding::from_value(&value).map_err(|why| {
        NativeError::throw(
            REGISTRATION_EXCEPTION,
            format!("{operation} cannot be published as an ability: {why}"),
        )
    })?;
    store.bind(&operation, ABILITIES, binding.to_value());
    Ok(PhpValue::Null)
}

/// The native area that gives PHP `Abilities::publish` and the Abilities adapter.
pub struct AbilitiesArea;

impl AbilitiesArea {
    /// The area writing `Abilities::publish` bindings to `bindings`.
    pub fn new(bindings: Rc<dyn BindingStore>) -> AbilitiesArea {
        BINDINGS.with(|b| *b.borrow_mut() = Some(bindings));
        AbilitiesArea
    }
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for AbilitiesArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 1] = [("publish", native_publish)];
        entries
            .into_iter()
            .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
            .collect()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://abilities", FACADE) {
            eprintln!("speedyspec_wordpress: declaring SpeedySpec\\Abilities failed: {err}");
        }
    }

    fn request_end(&self) {
        crate::handles::clear();
    }
}
