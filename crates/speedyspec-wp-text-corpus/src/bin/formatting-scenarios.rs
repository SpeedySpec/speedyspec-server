//! `formatting-scenarios`: rewrites `compat/formatting/scenarios.json` from WordPress's own PHPUnit data providers,
//! as `compat/formatting/providers.json` lists them, plus the hand-written cases in `compat/formatting/literals.json`
//! (see `compat/formatting/generate.php`).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    if let Err(why) = run() {
        eprintln!("{why}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = root();
    let compat = root.join("compat/formatting");
    let tests = root.join("tests/php/wordpress");
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .map_err(|e| e.to_string())?;
    let code = format!(
        "define('SPEEDYSPEC_FORMATTING_PROVIDERS', {:?}); define('SPEEDYSPEC_FORMATTING_LITERALS', {:?}); \
         define('SPEEDYSPEC_FORMATTING_TESTS', {:?}); define('DIR_TESTDATA', {:?}); require {:?};",
        compat.join("providers.json").display().to_string(),
        compat.join("literals.json").display().to_string(),
        tests.join("formatting").display().to_string(),
        tests.join("data").display().to_string(),
        compat.join("generate.php").display().to_string(),
    );
    php.eval("speedyspec://formatting/generate", &code)
        .map_err(|e| format!("loading the generator: {e}"))?;
    let output = php
        .call(&Value::Str("speedyspec_formatting_scenarios".into()), &[])
        .map_err(|e| format!("generating the scenarios: {e}"))?;
    let Value::Str(json) = output else {
        return Err(format!("the generator returned {output:?}, not JSON"));
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("the generator's output: {e}"))?;
    let mut text = serde_json::to_string_pretty(&parsed).map_err(|e| e.to_string())?;
    text.push('\n');
    let path = compat.join("scenarios.json");
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}
