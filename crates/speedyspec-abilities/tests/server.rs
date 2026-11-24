use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_abilities::errors::wp_error;
use speedyspec_abilities::{ABILITIES, AbilitiesServer, OperationSteps};
use speedyspec_abilities_contracts::{
    AbilityHooks, AbilityOutcome, AbilityResult, AbilitySteps, NoAbilityHooks,
};
use speedyspec_container::Container;
use speedyspec_operation::identity::MemoryIdentity;
use speedyspec_operation::{Operation, Registries, S};
use speedyspec_operation_contracts::{
    Caller, Category, OperationError, RawInput, RawOutput, Value,
};
use speedyspec_operation_dispatch::{Dispatcher, OperationHandler, bind_handler};

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Default)]
struct Hooks {
    log: Log,
    pre: RefCell<Option<AbilityOutcome>>,
    fail_before: RefCell<bool>,
}

impl AbilityHooks for Hooks {
    fn invoked(&self, _: &str, _: &Option<Value>, _: u64) -> Result<(), OperationError> {
        self.log.borrow_mut().push("invoked".into());
        Ok(())
    }
    fn pre_execute(
        &self,
        _: &str,
        _: &Option<Value>,
        _: u64,
    ) -> Result<Option<AbilityOutcome>, OperationError> {
        self.log.borrow_mut().push("pre_execute".into());
        Ok(self.pre.borrow().clone())
    }
    fn normalize_input(
        &self,
        _: &str,
        input: Option<Value>,
        _: u64,
    ) -> AbilityResult<Option<Value>> {
        self.log.borrow_mut().push("filter:normalize".into());
        Ok(input)
    }
    fn validate_input(
        &self,
        _: &str,
        valid: AbilityResult<()>,
        _: &Option<Value>,
    ) -> AbilityResult<()> {
        self.log.borrow_mut().push("filter:validate".into());
        valid
    }
    fn permission_result(
        &self,
        _: &str,
        r: AbilityResult<bool>,
        _: &Option<Value>,
        _: u64,
    ) -> AbilityResult<bool> {
        self.log.borrow_mut().push("filter:permission".into());
        r
    }
    fn before_execute(&self, _: &str, _: &Option<Value>, _: u64) -> Result<(), OperationError> {
        self.log.borrow_mut().push("before_execute".into());
        if *self.fail_before.borrow() {
            return Err(OperationError::new("boom", Category::Internal, "boom"));
        }
        Ok(())
    }
    fn execute_result(
        &self,
        _: &str,
        r: AbilityOutcome,
        _: &Option<Value>,
        _: u64,
    ) -> AbilityOutcome {
        self.log.borrow_mut().push("filter:execute".into());
        r
    }
    fn validate_output(&self, _: &str, valid: AbilityResult<()>, _: &Value) -> AbilityResult<()> {
        self.log.borrow_mut().push("filter:validate_output".into());
        valid
    }
    fn after_execute(
        &self,
        _: &str,
        _: &Option<Value>,
        _: &Value,
        _: u64,
    ) -> Result<(), OperationError> {
        self.log.borrow_mut().push("after_execute".into());
        Ok(())
    }
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str) {
        self.log
            .borrow_mut()
            .push(format!("doing_it_wrong:{function}:{message}:{version}"));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fail {
    None,
    Normalize,
    Validate,
    PermissionFalse,
    PermissionError,
    Execute,
    Output,
}

struct Scripted(Log, Fail);

fn err(code: &str) -> OperationError {
    wp_error(code, format!("{code} failed"))
}

impl AbilitySteps for Scripted {
    fn normalize_input(&self, input: Option<Value>) -> AbilityResult<Option<Value>> {
        self.0.borrow_mut().push("normalize".into());
        if self.1 == Fail::Normalize {
            Err(err("n"))
        } else {
            Ok(input)
        }
    }
    fn validate_input(&self, _: &Option<Value>) -> AbilityResult<()> {
        self.0.borrow_mut().push("validate".into());
        if self.1 == Fail::Validate {
            Err(err("v"))
        } else {
            Ok(())
        }
    }
    fn check_permissions(&self, _: &Option<Value>) -> AbilityResult<bool> {
        self.0.borrow_mut().push("permission".into());
        match self.1 {
            Fail::PermissionFalse => Ok(false),
            Fail::PermissionError => Err(wp_error("nope", "No <b>way</b>.")),
            _ => Ok(true),
        }
    }
    fn execute(&self, input: &Option<Value>) -> AbilityOutcome {
        self.0.borrow_mut().push("execute".into());
        if self.1 == Fail::Execute {
            Err(err("e"))
        } else {
            Ok(input.clone().unwrap_or(Value::Null))
        }
    }
    fn validate_output(&self, _: &Value) -> AbilityResult<()> {
        self.0.borrow_mut().push("validate_output".into());
        if self.1 == Fail::Output {
            Err(err("o"))
        } else {
            Ok(())
        }
    }
}

fn run(fail: Fail, hooks: Hooks) -> (AbilityOutcome, Vec<String>) {
    let log = hooks.log.clone();
    let server = AbilitiesServer::new(Rc::new(hooks));
    let out = server.execute(
        "acme/x",
        Some(Value::Int(3)),
        &Scripted(log.clone(), fail),
        1,
    );
    let log = log.borrow().clone();
    (out, log)
}

#[test]
fn hooks_fire_in_wordpress_order() {
    let (out, log) = run(Fail::None, Hooks::default());
    assert_eq!(out, Ok(Value::Int(3)));
    assert_eq!(
        log,
        [
            "invoked",
            "pre_execute",
            "normalize",
            "validate",
            "permission",
            "before_execute",
            "execute",
            "validate_output",
            "after_execute"
        ]
    );
}

#[test]
fn pre_execute_short_circuits_with_any_value() {
    for value in [
        Ok(Value::Null),
        Ok(Value::Bool(false)),
        Ok(Value::Int(0)),
        Ok(Value::List(vec![])),
        Err(err("blocked")),
    ] {
        let hooks = Hooks::default();
        *hooks.pre.borrow_mut() = Some(value.clone());
        let (out, log) = run(Fail::None, hooks);
        assert_eq!(out, value);
        assert_eq!(log, ["invoked", "pre_execute"]);
    }
}

#[test]
fn a_failed_permission_is_wordpress_s_error() {
    let expected = wp_error(
        "ability_invalid_permissions",
        "Ability \"acme/x\" does not have necessary permission.",
    );
    let (out, log) = run(Fail::PermissionFalse, Hooks::default());
    assert_eq!(out, Err(expected.clone()));
    assert_eq!(out.unwrap_err().category, Category::PermissionDenied);
    assert!(!log.iter().any(|l| l.starts_with("doing_it_wrong")));
    let (out, log) = run(Fail::PermissionError, Hooks::default());
    assert_eq!(out, Err(expected));
    assert_eq!(
        log.iter()
            .filter(|l| l.starts_with("doing_it_wrong"))
            .collect::<Vec<_>>(),
        ["doing_it_wrong:WP_Ability::execute:No <b>way</b>.:6.9.0"]
    );
}

#[test]
fn an_error_at_each_step_stops_there() {
    for (fail, code, last) in [
        (Fail::Normalize, "n", "normalize"),
        (Fail::Validate, "v", "validate"),
        (Fail::Execute, "e", "execute"),
        (Fail::Output, "o", "validate_output"),
    ] {
        let (out, log) = run(fail, Hooks::default());
        assert_eq!(out.unwrap_err().code, code);
        assert_eq!(log.last().map(String::as_str), Some(last));
    }
}

#[test]
fn a_failing_hook_fails_closed() {
    let hooks = Hooks::default();
    *hooks.fail_before.borrow_mut() = true;
    let (out, log) = run(Fail::None, hooks);
    let error = out.unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.category),
        ("ability_hook_failed", Category::Internal)
    );
    assert!(!log.contains(&"execute".to_owned()));
}

struct Echo(fn(&Value) -> Value);

impl OperationHandler for Echo {
    fn handle(
        &self,
        _: Box<dyn std::any::Any>,
        _: &Caller,
    ) -> Result<Box<dyn std::any::Any>, OperationError> {
        unreachable!()
    }
    fn neutral(&self) -> bool {
        true
    }
    fn handle_neutral(&self, input: &Value, _: &Caller) -> Result<Value, OperationError> {
        Ok((self.0)(input))
    }
}

fn operation_world() -> (Rc<Dispatcher>, Rc<Registries>) {
    let r = Registries::new();
    for builder in [
        Operation::query("acme/get-report")
            .input::<RawInput>(S::object([("id", S::integer().required())]))
            .output::<RawOutput>(S::object([("id", S::integer().required())]))
            .permission(|c: &Caller, _: &RawInput| c.can("read_report", &[])),
        Operation::query("acme/broken")
            .input::<RawInput>(S::object([]))
            .output::<RawOutput>(S::object([("id", S::integer().required())]))
            .public(),
    ] {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    r.close().unwrap();
    let r = Rc::new(r);
    let c = Rc::new(Container::new());
    let echo: Rc<dyn OperationHandler> = Rc::new(Echo(|v| v.clone()));
    bind_handler(&c, "acme/get-report", move |_, _| Ok(echo.clone()));
    let bad: Rc<dyn OperationHandler> = Rc::new(Echo(|_| Value::from("not an object")));
    bind_handler(&c, "acme/broken", move |_, _| Ok(bad.clone()));
    (Rc::new(Dispatcher::new(c, r.clone())), r)
}

fn steps(name: &str, user: Option<u64>) -> OperationSteps {
    let (dispatcher, registries) = operation_world();
    let spec =
        speedyspec_operation_contracts::OperationCatalog::get_operation(registries.as_ref(), name)
            .unwrap();
    let caller = Caller::new(
        user,
        ABILITIES,
        Rc::new(MemoryIdentity::new().grant(7, "read_report")),
    );
    OperationSteps::new(dispatcher, spec, caller, Rc::new(NoAbilityHooks), 1)
}

#[test]
fn operation_steps_follow_wordpress() {
    let s = steps("acme/get-report", Some(7));
    let input = Some(Value::map([("id", Value::Int(4))]));
    assert_eq!(s.normalize_input(input.clone()), Ok(input.clone()));
    assert_eq!(s.validate_input(&input), Ok(()));
    let invalid = s
        .validate_input(&Some(Value::map([("id", Value::from("x"))])))
        .unwrap_err();
    assert_eq!(invalid.code, "ability_invalid_input");
    assert!(
        invalid
            .message
            .starts_with("Ability \"acme/get-report\" has invalid input. Reason: "),
        "{}",
        invalid.message
    );
    assert_eq!(s.check_permissions(&input), Ok(true));
    assert_eq!(s.execute(&input), Ok(Value::map([("id", Value::Int(4))])));
    assert_eq!(
        steps("acme/get-report", Some(8)).check_permissions(&input),
        Ok(false)
    );
    assert_eq!(
        steps("acme/get-report", None).check_permissions(&input),
        Ok(false)
    );
}

#[test]
fn no_input_is_an_empty_object() {
    let s = steps("acme/broken", None);
    assert_eq!(s.validate_input(&None), Ok(()));
    assert_eq!(s.check_permissions(&None), Ok(true));
}

#[test]
fn an_invalid_output_is_ability_invalid_output() {
    let e = steps("acme/broken", None).execute(&None).unwrap_err();
    assert_eq!(e.code, "ability_invalid_output");
    assert!(
        e.message
            .starts_with("Ability \"acme/broken\" has invalid output. Reason: "),
        "{}",
        e.message
    );
}
