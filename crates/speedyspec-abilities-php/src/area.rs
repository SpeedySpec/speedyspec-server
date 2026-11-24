//! The native area: `Abilities::publish` and the PHP adapter's declaration in each request.

use std::rc::Rc;

use speedyspec_abilities::ABILITIES;
use speedyspec_abilities_contracts::AbilityBinding;
use speedyspec_operation_php::convert::from_php;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value as PhpValue};
use speedyspec_transport_contracts::BindingStore;

const FACADE: &str = include_str!("php/abilities.php");
const TRUNK_ABILITY: &str = include_str!("php/trunk-ability.php");
const SHIM_ABILITY: &str = include_str!("php/wp-ability.php");
const OPERATION_ABILITY: &str = include_str!("php/operation-ability.php");
const NATIVE: &str = "SpeedySpec\\Abilities\\Native\\";
const REGISTRATION_EXCEPTION: &str = "SpeedySpec\\Operation\\RegistrationException";

thread_local! {
    static BINDINGS: std::cell::RefCell<Option<Rc<dyn BindingStore>>> = const { std::cell::RefCell::new(None) };
    static SERVICE: std::cell::RefCell<Option<Rc<crate::AbilitiesService>>> = const { std::cell::RefCell::new(None) };
}

fn service() -> Result<Rc<crate::AbilitiesService>, NativeError> {
    SERVICE.with(|s| s.borrow().clone()).ok_or_else(|| {
        NativeError::throw(
            REGISTRATION_EXCEPTION,
            "the Abilities service is not available",
        )
    })
}

/// `WP_Ability::execute()` (LegacyCallerAdapter): any ability, through the Rust Abilities server.
fn native_execute(args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let service = service()?;
    let ability = args.first().cloned().unwrap_or(PhpValue::Null);
    let input = match args.get(1) {
        None | Some(PhpValue::Null) => None,
        Some(value) => Some(crate::values::neutral(value)),
    };
    let object = speedyspec_php_ext::PhpObject::new(ability.clone()).ok_or_else(|| {
        NativeError::throw("InvalidArgumentException", "execute takes a WP_Ability")
    })?;
    let name = match object.call("get_name", vec![]) {
        Ok(PhpValue::Str(name)) => name,
        _ => String::new(),
    };
    let id = match speedyspec_php::call(
        &PhpValue::Str("spl_object_id".into()),
        std::slice::from_ref(&ability),
    ) {
        Ok(PhpValue::Int(id)) => id,
        _ => 0,
    };
    let operation = matches!(
        speedyspec_php::call(&PhpValue::Str("get_class".into()), std::slice::from_ref(&ability)),
        Ok(PhpValue::Str(class)) if class == "SpeedySpec\\Abilities\\OperationAbility"
    );
    let handle = crate::handles::add(ability);
    let outcome = crate::execute::run(&service, &name, id, handle, input, operation);
    Ok(match outcome {
        Ok(value) => crate::values::php(&value),
        Err(error) => crate::errors::to_wp_error(&error),
    })
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

/// `[ slug, … ]`: the default categories of the operations exposed as abilities.
fn native_operation_categories(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    let mut slugs: Vec<String> = Vec::new();
    for (_, binding, default_category) in abilities()? {
        if let Some(slug) = binding.category
            && default_category
            && !slugs.contains(&slug)
        {
            slugs.push(slug);
        }
    }
    Ok(PhpValue::Array(
        slugs
            .into_iter()
            .enumerate()
            .map(|(i, s)| (Key::Int(i as i64), PhpValue::Str(s)))
            .collect(),
    ))
}

fn abilities() -> Result<
    Vec<(
        Rc<speedyspec_operation_contracts::OperationSpec>,
        AbilityBinding,
        bool,
    )>,
    NativeError,
> {
    Ok(service()?.abilities().unwrap_or_else(|err| {
        eprintln!("speedyspec_wordpress: no operations are abilities: {err}");
        Vec::new()
    }))
}

/// `[ [ name, args ], … ]`: each operation exposed as an ability, with the arguments `wp_register_ability` takes.
fn native_operation_abilities(_args: &[PhpValue]) -> Result<PhpValue, NativeError> {
    use speedyspec_operation_contracts::Value;
    let mut rows = Vec::new();
    for (i, (spec, binding, _)) in abilities()?.into_iter().enumerate() {
        let flag = |b: Option<bool>| b.map_or(Value::Null, Value::Bool);
        let mut meta = vec![
            (
                "annotations".to_owned(),
                Value::map([
                    ("readonly", flag(binding.annotations.readonly)),
                    ("destructive", flag(binding.annotations.destructive)),
                    ("idempotent", flag(binding.annotations.idempotent)),
                ]),
            ),
            (
                "show_in_rest".to_owned(),
                Value::Bool(binding.show_in_rest.unwrap_or(false)),
            ),
            (
                "public".to_owned(),
                Value::Bool(binding.public.unwrap_or(false)),
            ),
        ];
        if let Value::Map(extra) = &binding.meta {
            for (key, value) in extra {
                if !meta.iter().any(|(k, _)| k == key) {
                    meta.push((key.clone(), value.clone()));
                }
            }
        }
        let schema = |s: &speedyspec_operation_contracts::Schema| {
            crate::service::from_json(&speedyspec_operation::export::json_schema(s))
        };
        let args = Value::map([
            ("label", Value::String(binding.label.unwrap_or_default())),
            (
                "description",
                Value::String(binding.description.unwrap_or_default()),
            ),
            (
                "category",
                Value::String(binding.category.unwrap_or_default()),
            ),
            ("input_schema", schema(&spec.input.schema)),
            ("output_schema", schema(&spec.output.schema)),
            ("meta", Value::Map(meta.into_iter().collect())),
        ]);
        rows.push((
            Key::Int(i as i64),
            PhpValue::Array(vec![
                (Key::Int(0), PhpValue::Str(spec.name.as_str().to_owned())),
                (
                    Key::Int(1),
                    speedyspec_operation_php::convert::to_php(&args),
                ),
            ]),
        ));
    }
    Ok(PhpValue::Array(rows))
}

/// The native area that gives PHP `Abilities::publish` and the Abilities adapter.
pub struct AbilitiesArea;

impl AbilitiesArea {
    /// The area writing `Abilities::publish` bindings to `bindings`.
    pub fn new(bindings: Rc<dyn BindingStore>) -> AbilitiesArea {
        BINDINGS.with(|b| *b.borrow_mut() = Some(bindings));
        AbilitiesArea
    }

    /// The service `WP_Ability::execute()` reaches the Abilities server through.
    pub fn with_service(self, service: Rc<crate::AbilitiesService>) -> Self {
        SERVICE.with(|s| *s.borrow_mut() = Some(service));
        self
    }
}

type NativeFn = fn(&[PhpValue]) -> Result<PhpValue, NativeError>;

impl NativeArea for AbilitiesArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let entries: [(&str, NativeFn); 4] = [
            ("publish", native_publish),
            ("execute", native_execute),
            ("operation_categories", native_operation_categories),
            ("operation_abilities", native_operation_abilities),
        ];
        entries
            .into_iter()
            .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
            .collect()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://abilities", FACADE) {
            eprintln!("speedyspec_wordpress: declaring SpeedySpec\\Abilities failed: {err}");
        }
        let declared = speedyspec_php::call(
            &PhpValue::Str("class_exists".into()),
            &[PhpValue::Str("WP_Ability".into()), PhpValue::Bool(false)],
        );
        if !matches!(declared, Ok(PhpValue::Bool(true))) {
            for (label, source) in [
                ("speedyspec://abilities/trunk-ability", TRUNK_ABILITY),
                ("speedyspec://abilities/ability", SHIM_ABILITY),
                (
                    "speedyspec://abilities/operation-ability",
                    OPERATION_ABILITY,
                ),
            ] {
                if let Err(err) = speedyspec_php::eval(label, source) {
                    eprintln!("speedyspec_wordpress: declaring {label} failed: {err}");
                }
            }
        }
        // Where WordPress's Hook API is native, it exists now: register operations as abilities on its init actions.
        crate::errors::adapter("hook", &[]);
    }

    fn request_end(&self) {
        crate::handles::clear();
        crate::legacy::clear();
    }
}
