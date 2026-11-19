//! Running scenarios in the static PHP, with the driver (`driver.php`) that both sides share.

use speedyspec_hooks::Value as PhpValue;
use speedyspec_php::Php;

use crate::{Scenario, ScenarioRunner, StepResult};

/// The PHP that runs a scenario's steps and reports each one: `speedyspec_compat_run(string $scenario): string`.
pub const DRIVER: &str = include_str!("driver.php");

type Setup = Box<dyn Fn(&Php) -> Result<(), String>>;

/// Runs each scenario in a fresh request: `setup` loads the implementation under test (WordPress's files when
/// recording, nothing when the port is native), then the driver runs the steps.
pub struct PhpRunner {
    php: Php,
    setup: Setup,
}

impl PhpRunner {
    pub fn new(php: Php, setup: impl Fn(&Php) -> Result<(), String> + 'static) -> PhpRunner {
        PhpRunner {
            php,
            setup: Box::new(setup),
        }
    }

    pub fn php(&self) -> &Php {
        &self.php
    }
}

impl ScenarioRunner for PhpRunner {
    fn run(&mut self, scenario: &Scenario) -> Result<Vec<StepResult>, String> {
        self.php
            .restart_request()
            .map_err(|e| format!("starting a request: {e}"))?;
        (self.setup)(&self.php)?;
        self.php
            .eval("speedyspec://compat/driver", DRIVER)
            .map_err(|e| format!("loading the driver: {e}"))?;
        let input = serde_json::to_string(scenario).map_err(|e| e.to_string())?;
        let output = self
            .php
            .call(
                &PhpValue::Str("speedyspec_compat_run".into()),
                &[PhpValue::Str(input)],
            )
            .map_err(|e| format!("running the scenario: {e}"))?;
        let PhpValue::Str(output) = output else {
            return Err(format!("the driver returned {output:?}, not JSON"));
        };
        serde_json::from_str(&output).map_err(|e| format!("the driver's output: {e}"))
    }
}
