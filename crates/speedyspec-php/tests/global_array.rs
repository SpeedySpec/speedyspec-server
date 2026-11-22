//! Reading a global PHP array in place: its keys in order and one entry, without copying the whole array.
//!
//! PHP boots once per process, so this file has its own `main()` (`harness = false`).

use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{
    BootOptions, Php, global_array_contains, global_array_get, global_array_keys,
};

type Case = fn(&Php) -> Result<(), String>;

const CASES: &[(&str, Case)] = &[
    ("keys_in_order", case_keys_in_order),
    (
        "keys_follow_writes_and_replacement",
        case_keys_follow_writes,
    ),
    ("keys_of_unset_or_scalar_global", case_keys_absent),
    ("numeric_keys_and_bytes", case_numeric_keys_and_bytes),
    ("get_and_contains", case_get_and_contains),
    ("keys_through_a_reference", case_keys_through_reference),
];

fn keys(name: &str) -> Option<Vec<String>> {
    global_array_keys(name).map(|k| {
        k.into_iter()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .collect()
    })
}

fn expect<T: PartialEq + std::fmt::Debug>(got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}, want {want:?}"))
    }
}

fn case_keys_in_order(php: &Php) -> Result<(), String> {
    php.eval("t", "$GLOBALS['ga1'] = ['b' => 1, 'a' => 2, 'c' => 3];")
        .map_err(|e| e.to_string())?;
    expect(keys("ga1"), Some(vec!["b".into(), "a".into(), "c".into()]))
}

fn case_keys_follow_writes(php: &Php) -> Result<(), String> {
    php.eval("t", "$GLOBALS['ga2'] = ['a' => 1, 'b' => 2]; unset($GLOBALS['ga2']['a']); $GLOBALS['ga2']['a'] = 3;")
        .map_err(|e| e.to_string())?;
    expect(keys("ga2"), Some(vec!["b".into(), "a".into()]))?;
    php.eval(
        "t",
        "$saved = $GLOBALS['ga2']; $GLOBALS['ga2'] = ['x' => 1]; $GLOBALS['ga2_saved'] = $saved;",
    )
    .map_err(|e| e.to_string())?;
    expect(keys("ga2"), Some(vec!["x".into()]))?;
    php.eval("t", "$GLOBALS['ga2'] = $GLOBALS['ga2_saved'];")
        .map_err(|e| e.to_string())?;
    expect(keys("ga2"), Some(vec!["b".into(), "a".into()]))
}

fn case_keys_absent(php: &Php) -> Result<(), String> {
    php.eval("t", "$GLOBALS['ga3'] = 'scalar';")
        .map_err(|e| e.to_string())?;
    expect(keys("ga3"), None)?;
    expect(keys("ga_never_set"), None)
}

fn case_numeric_keys_and_bytes(php: &Php) -> Result<(), String> {
    php.eval("t", "$GLOBALS['ga4'] = []; $GLOBALS['ga4']['123'] = 1; $GLOBALS['ga4'][\"\\xff\"] = 2; $GLOBALS['ga4'][] = 3;")
        .map_err(|e| e.to_string())?;
    expect(
        global_array_keys("ga4"),
        Some(vec![b"123".to_vec(), b"\xff".to_vec(), b"124".to_vec()]),
    )?;
    php.eval("t", "$GLOBALS['ga5'] = ['x', 'y'];")
        .map_err(|e| e.to_string())?;
    expect(
        global_array_keys("ga5"),
        Some(vec![b"0".to_vec(), b"1".to_vec()]),
    )
}

fn case_get_and_contains(php: &Php) -> Result<(), String> {
    php.eval(
        "t",
        "$GLOBALS['ga6'] = ['caption' => 'img_caption_shortcode', '7' => 'seven', 'n' => null];",
    )
    .map_err(|e| e.to_string())?;
    expect(
        global_array_get("ga6", b"caption"),
        Some(Value::Str("img_caption_shortcode".into())),
    )?;
    expect(
        global_array_get("ga6", b"7"),
        Some(Value::Str("seven".into())),
    )?;
    expect(global_array_get("ga6", b"missing"), None)?;
    expect(global_array_contains("ga6", b"n"), true)?;
    expect(global_array_contains("ga6", b"07"), false)?;
    expect(global_array_contains("ga_never_set", b"a"), false)
}

fn case_keys_through_reference(php: &Php) -> Result<(), String> {
    php.eval(
        "t",
        "$GLOBALS['ga7'] = ['a' => 1]; $r = &$GLOBALS['ga7']; $r['b'] = 2;",
    )
    .map_err(|e| e.to_string())?;
    expect(keys("ga7"), Some(vec!["a".into(), "b".into()]))
}

fn main() -> ExitCode {
    let php = match Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::default()),
        preload: &[],
        areas: Vec::new(),
    }) {
        Ok(php) => php,
        Err(err) => {
            println!("FAILED: boot: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = 0;
    for (name, case) in CASES {
        match case(&php) {
            Ok(()) => println!("ok: {name}"),
            Err(why) => {
                failed += 1;
                println!("FAILED: {name}: {why}");
            }
        }
    }
    drop(php);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
