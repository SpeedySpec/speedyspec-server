//! Tests for the embedded PHP runtime and the hook bridge.
//!
//! PHP is process-global and must stay on the thread that booted it, so this
//! file has its own `main()` (`harness = false`): it boots PHP once, runs every
//! case in order on the main thread, and exits 1 if any case failed.
//!
//! Because PHP boots once, the boot options serve every case: the preload is
//! the one `case_preload_is_available` checks, and the hook backend is
//! [`TestHooks`], an in-memory registry that `case_backend_error_is_exception`
//! can switch into failing.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_hooks::{
    AddOptions, Callback, CallbackError, CallbackId, HookError, HookErrorKind, Hooks, Key, Value,
};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Output, Php, PhpError};

struct Ctx {
    php: Php,
    hooks: Rc<TestHooks>,
    /// Set by a Rust callback that a fiber parked inside a hook fires while
    /// PHP destroys it at shutdown.
    shutdown_seen: Rc<Cell<bool>>,
}

type Case = fn(&Ctx) -> Result<(), String>;

const CASES: &[(&str, Case)] = &[
    // Must stay first: it checks a firing before any PHP code has run since boot.
    ("rust_only_hook_skips_php", case_rust_only_hook_skips_php),
    ("version", case_version),
    ("ini_not_read", case_ini_not_read),
    ("hook_add_and_run", case_hook_add_and_run),
    ("filter_from_php", case_filter_from_php),
    ("priority_order", case_priority_order),
    ("accepted_args", case_accepted_args),
    ("own_hook_functions", case_own_hook_functions),
    (
        "extensions_wordpress_needs",
        case_extensions_wordpress_needs,
    ),
    ("std_handles", case_std_handles),
    (
        "remove_closure_by_identity",
        case_remove_closure_by_identity,
    ),
    ("has_with_callback", case_has_with_callback),
    ("numeric_priority", case_numeric_priority),
    ("extreme_priority", case_extreme_priority),
    ("object_identity_survives", case_object_identity_survives),
    ("rust_callback_seen_by_php", case_rust_callback_seen_by_php),
    ("rust_fires_into_php", case_rust_fires_into_php),
    ("exception_reaches_php", case_exception_reaches_php),
    ("exception_reaches_rust", case_exception_reaches_rust),
    ("exit_in_require_is_caught", case_exit_in_require_is_caught),
    ("exit_in_run_file", case_exit_in_run_file),
    ("run_file_argv", case_run_file_argv),
    ("run_file_script_name", case_run_file_script_name),
    (
        "globals_read_and_written_from_rust",
        case_globals_read_and_written_from_rust,
    ),
    ("fiber_fires_hook", case_fiber_fires_hook),
    ("fiber_without_hooks", case_fiber_without_hooks),
    (
        "execution_context_per_fiber",
        case_execution_context_per_fiber,
    ),
    ("fiber_in_callback", case_fiber_in_callback),
    ("fiber_suspends_in_hook", case_fiber_suspends_in_hook),
    ("fiber_filter_suspends", case_fiber_filter_suspends),
    (
        "fiber_parked_while_main_works",
        case_fiber_parked_while_main_works,
    ),
    ("fiber_two_parked", case_fiber_two_parked),
    (
        "fiber_dropped_while_parked",
        case_fiber_dropped_while_parked,
    ),
    ("fiber_throw_into_parked", case_fiber_throw_into_parked),
    ("fiber_frames_per_context", case_fiber_frames_per_context),
    (
        "fiber_frames_are_not_shared",
        case_fiber_frames_are_not_shared,
    ),
    ("fiber_exit_while_parked", case_fiber_exit_while_parked),
    // Before the first fatal error: a bailout turns the cycle collector off.
    ("fiber_destroyed_by_gc", case_fiber_destroyed_by_gc),
    ("fiber_exit_in_callback", case_fiber_exit_in_callback),
    (
        "fiber_suspend_outside_fiber",
        case_fiber_suspend_outside_fiber,
    ),
    // Must run right before the first fatal error of the run.
    ("not_poisoned_before_fatal", case_not_poisoned_before_fatal),
    // The first bailout of the run: memory exhaustion must poison by itself.
    ("oom_is_fatal", case_oom_is_fatal),
    ("fatal_is_caught", case_fatal_is_caught),
    ("poisoned_after_fatal", case_poisoned_after_fatal),
    ("warning_goes_to_stderr", case_warning_goes_to_stderr),
    (
        "deprecation_goes_to_stderr",
        case_deprecation_goes_to_stderr,
    ),
    ("fatal_goes_to_stderr", case_fatal_goes_to_stderr),
    (
        "uncaught_in_run_file_not_on_stdout",
        case_uncaught_in_run_file_not_on_stdout,
    ),
    ("fatal_while_fiber_parked", case_fatal_while_fiber_parked),
    (
        "fiber_abandoned_after_fatal",
        case_fiber_abandoned_after_fatal,
    ),
    ("fatal_inside_callback", case_fatal_inside_callback),
    ("exit_inside_callback", case_exit_inside_callback),
    ("panic_propagates", case_panic_propagates),
    ("panic_inside_fiber", case_panic_inside_fiber),
    ("array_round_trip", case_array_round_trip),
    ("preload_is_available", case_preload_is_available),
    (
        "construct_builds_an_object",
        case_construct_builds_an_object,
    ),
    ("construct_unknown_class", case_construct_unknown_class),
    ("construct_wrong_args", case_construct_wrong_args),
    ("output_redirect", case_output_redirect),
    (
        "backend_error_is_exception",
        case_backend_error_is_exception,
    ),
    ("call_and_inspect", case_call_and_inspect),
    (
        "stray_destructor_exception",
        case_stray_destructor_exception,
    ),
    ("refcounts_balance", case_refcounts_balance),
    // Must stay last: it leaves a fiber parked inside a hook for shutdown.
    ("fiber_parked_at_shutdown", case_fiber_parked_at_shutdown),
];

fn main() -> ExitCode {
    let hooks = Rc::new(TestHooks::default());
    let php = match Php::boot(BootOptions {
        hooks: hooks.clone(),
        preload: &[("T.php", "<?php final class PreT {}")],
        areas: Vec::new(),
    }) {
        Ok(php) => php,
        Err(err) => {
            println!("FAILED: boot: {err}");
            return ExitCode::FAILURE;
        }
    };
    let ctx = Ctx {
        php,
        hooks,
        shutdown_seen: Rc::default(),
    };

    let mut failed = 0;
    for (name, case) in CASES {
        match case(&ctx) {
            Ok(()) => println!("ok: {name}"),
            Err(why) => {
                failed += 1;
                println!("FAILED: {name}: {why}");
            }
        }
    }

    // The registry outlives the engine here, still holding PHP closures.
    let Ctx {
        php,
        hooks,
        shutdown_seen,
    } = ctx;
    // Two abandoned fibers' records (and the parked one's) are still saved;
    // dropping `Php` must forget them without touching freed memory.
    let records = speedyspec_php::fiber_records();
    let (out, ()) = capture(1, || drop(php));
    if records < 3 || speedyspec_php::fiber_records() != 0 {
        failed += 1;
        println!(
            "FAILED: abandoned_records_forgotten_at_drop: {records} before, {} after",
            speedyspec_php::fiber_records()
        );
    } else {
        println!("ok: abandoned_records_forgotten_at_drop");
    }
    match out {
        Ok(out) if out.is_empty() && shutdown_seen.get() => {
            println!("ok: parked_fiber_unwound_at_shutdown");
        }
        other => {
            failed += 1;
            println!(
                "FAILED: parked_fiber_unwound_at_shutdown: output {other:?}, unwound {}",
                shutdown_seen.get()
            );
        }
    }
    match after_shutdown(&hooks) {
        Ok(()) => println!("ok: php_callbacks_inert_after_shutdown"),
        Err(why) => {
            failed += 1;
            println!("FAILED: php_callbacks_inert_after_shutdown: {why}");
        }
    }
    drop(hooks);

    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// Task 1

fn case_version(ctx: &Ctx) -> Result<(), String> {
    let out = stdout_of(|| ctx.php.eval("t", "echo PHP_VERSION;"))?;
    eq(&out, speedyspec_php::PHP_VERSION)
}

fn case_ini_not_read(ctx: &Ctx) -> Result<(), String> {
    let out = stdout_of(|| ctx.php.eval("t", "var_export(php_ini_loaded_file());"))?;
    eq(&out, "false")
}

// ---------------------------------------------------------------------------
// Hooks registered and fired from PHP

fn case_hook_add_and_run(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "hook_add_and_run.php")?, "a")
}

fn case_filter_from_php(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "filter_from_php.php")?, "42")
}

fn case_priority_order(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "priority_order.php")?, "ab")
}

fn case_accepted_args(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "accepted_args.php")?, "2")
}

fn case_extensions_wordpress_needs(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "extensions.php")?, "yyyyyyyyyyyy")
}

fn case_own_hook_functions(ctx: &Ctx) -> Result<(), String> {
    eq(
        &require_out(ctx, "own_hook_functions.php")?,
        "mine|nnnnnnn|21",
    )
}

fn case_std_handles(ctx: &Ctx) -> Result<(), String> {
    // STDIN, STDOUT and STDERR exist and are writable, as in the PHP CLI.
    // (They are `php://std*` streams, which under a non-CLI SAPI are dup()s of
    // fds 0-2 taken at boot, so this harness's fd capture cannot see what is
    // written to them; the CLI tests check where the bytes go.)
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "echo is_resource(STDIN) && is_resource(STDOUT) && is_resource(STDERR) ? 'y' : 'n';
                 echo fwrite(STDERR, '');",
            )
        })?,
        "y0",
    )
}

fn case_remove_closure_by_identity(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "remove_closure_by_identity.php")?, "1")
}

fn case_has_with_callback(ctx: &Ctx) -> Result<(), String> {
    // has($c) for the registered closure, another closure, an unregistered
    // function name, then has() with no callback.
    eq(&require_out(ctx, "has_with_callback.php")?, "ynny")
}

fn case_numeric_priority(ctx: &Ctx) -> Result<(), String> {
    // From a strict_types=1 file: ordering by a string priority, removal
    // across string and int spellings, integral floats, a string
    // acceptedArgs, then TypeError for 'abc', '20abc', 20.5, null, '20.5',
    // '', true and [20]; ValueError for acceptedArgs -1; TypeError for a
    // priority outside 64 bits and for acceptedArgs 'two'; nothing registered.
    eq(
        &require_out(ctx, "numeric_priority.php")?,
        "ab|111|11|2|TTTTTTTT|VRAn",
    )
}

fn case_extreme_priority(ctx: &Ctx) -> Result<(), String> {
    // PHP_INT_MIN first and PHP_INT_MAX last; removal at the wrong priority
    // fails, at PHP_INT_MAX and at (string) PHP_INT_MIN succeeds.
    eq(&require_out(ctx, "extreme_priority.php")?, "abc|011n")
}

fn case_object_identity_survives(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "object_identity_survives.php")?, "same")
}

// ---------------------------------------------------------------------------
// Rust and PHP sharing one registry

fn case_rust_only_hook_skips_php(ctx: &Ctx) -> Result<(), String> {
    let before = speedyspec_php::php_entries();
    add_rust(ctx, "rust.only", |args| match args.first() {
        Some(Value::Int(v)) => Ok(Value::Int(v + 1)),
        other => Err(CallbackError(format!("unexpected {other:?}"))),
    })?;
    let result = ctx
        .hooks
        .filter("rust.only", Value::Int(1), &[])
        .map_err(|e| e.to_string())?;
    let after = speedyspec_php::php_entries();
    if result != Value::Int(2) {
        return Err(format!("expected Int(2), got {result:?}"));
    }
    if after != before {
        return Err(format!(
            "the bridge entered PHP {} time(s) for a hook with only a Rust callback",
            after - before
        ));
    }

    // The counter is live: a hook with a PHP callback does enter PHP.
    ctx.php
        .eval(
            "t",
            "\\SpeedySpec\\Hooks\\Hook::add('rust.only.php', static fn (int $v): int => $v);",
        )
        .map_err(|e| e.to_string())?;
    let before = speedyspec_php::php_entries();
    ctx.hooks
        .filter("rust.only.php", Value::Int(1), &[])
        .map_err(|e| e.to_string())?;
    if speedyspec_php::php_entries() == before {
        return Err("php_entries() did not move when a PHP callback ran".to_owned());
    }
    Ok(())
}

fn case_rust_callback_seen_by_php(ctx: &Ctx) -> Result<(), String> {
    add_rust(ctx, "rust.append", |args| match args.first() {
        Some(Value::Str(s)) => Ok(Value::Str(format!("{s}!"))),
        other => Err(CallbackError(format!("unexpected {other:?}"))),
    })?;
    eq(&require_out(ctx, "rust_callback_seen_by_php.php")?, "hi!")
}

fn case_rust_fires_into_php(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "rust_fires_into_php.php")?, "")?;
    let result = ctx
        .hooks
        .filter("php.double", Value::Int(21), &[])
        .map_err(|e| e.to_string())?;
    if result == Value::Int(42) {
        Ok(())
    } else {
        Err(format!("expected Int(42), got {result:?}"))
    }
}

fn case_exception_reaches_php(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "exception_reaches_php.php")?, "boom")
}

fn case_exception_reaches_rust(ctx: &Ctx) -> Result<(), String> {
    ctx.php.set_owner(Some("acme/p"));
    let registered = require_out(ctx, "exception_reaches_rust.php");
    ctx.php.set_owner(None);
    eq(&registered?, "")?;

    let (out, result) = capture(1, || ctx.hooks.run("exception.rust", &[]));
    eq(&out?, "")?;
    let err = match result {
        Err(err) => err,
        Ok(()) => return Err("the exception did not reach Rust".to_owned()),
    };
    let text = err.to_string();
    if err.kind != HookErrorKind::Callback
        || err.owner.as_deref() != Some("acme/p")
        || !err.message.contains("boom")
        || !text.contains("boom")
        || !text.contains("acme/p")
    {
        return Err(format!("unexpected error {err:?} ({text})"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// exit(), fatal errors and argv

fn case_exit_in_require_is_caught(ctx: &Ctx) -> Result<(), String> {
    let (out, result) = capture(1, || ctx.php.require(&fixture("exit3.php")));
    eq(&out?, "")?;
    match result {
        Err(PhpError::Exit(3)) => {}
        other => return Err(format!("expected Err(Exit(3)), got {other:?}")),
    }
    eq(
        &stdout_of(|| ctx.php.eval("t", "echo 'still running';"))?,
        "still running",
    )
}

fn case_exit_in_run_file(ctx: &Ctx) -> Result<(), String> {
    let (out, result) = capture(1, || ctx.php.run_file(&fixture("exit3.php"), &[]));
    eq(&out?, "")?;
    match result {
        Ok(3) => Ok(()),
        other => Err(format!("expected Ok(3), got {other:?}")),
    }
}

fn case_run_file_argv(ctx: &Ctx) -> Result<(), String> {
    let path = fixture("argv.php");
    let argv = ["one".to_owned(), "two's".to_owned()];
    let (out, result) = capture(1, || ctx.php.run_file(&path, &argv));
    match result {
        Ok(0) => {}
        other => return Err(format!("expected Ok(0), got {other:?}")),
    }
    let joined = format!("{},one,two's", path.display());
    eq(&out?, &format!("{joined}|3|{joined}|3"))?;

    let missing = fixture("no-such-script.php");
    match ctx.php.run_file(&missing, &[]) {
        Err(PhpError::ScriptNotFound(p)) if p == missing => {}
        other => return Err(format!("expected ScriptNotFound, got {other:?}")),
    }

    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad = fixture("").join(OsStr::from_bytes(b"not-utf8-\xff.php"));
        match ctx.php.run_file(&bad, &[]) {
            Err(PhpError::NonUtf8Path(p)) if p == bad => {}
            other => return Err(format!("expected NonUtf8Path, got {other:?}")),
        }
    }
    Ok(())
}

/// `$_SERVER['SCRIPT_NAME']`, `SCRIPT_FILENAME` and `PHP_SELF` name the script, as the PHP CLI sets them (a phar's
/// stub runs only when it finds itself there).
fn case_run_file_script_name(ctx: &Ctx) -> Result<(), String> {
    let path = fixture("script_name.php");
    let (out, result) = capture(1, || ctx.php.run_file(&path, &[]));
    match result {
        Ok(0) => {}
        other => return Err(format!("expected Ok(0), got {other:?}")),
    }
    eq(&out?, "self|same|same")
}

/// `global()` and `set_global()` read and write `$GLOBALS`, through references, for the code that runs next.
fn case_globals_read_and_written_from_rust(ctx: &Ctx) -> Result<(), String> {
    if speedyspec_php::global("speedyspec_no_such_global").is_some() {
        return Err("a missing global reads as None".into());
    }
    ctx.php
        .eval("t", "$GLOBALS['speedyspec_g'] = ['a', 2];")
        .map_err(|e| e.to_string())?;
    let read = speedyspec_php::global("speedyspec_g");
    let expected = Value::Array(vec![
        (Key::Int(0), Value::Str("a".into())),
        (Key::Int(1), Value::Int(2)),
    ]);
    if read.as_ref() != Some(&expected) {
        return Err(format!("read {read:?}"));
    }
    speedyspec_php::set_global("speedyspec_g", &Value::Str("replaced".into()))
        .map_err(|e| e.to_string())?;
    speedyspec_php::set_global("speedyspec_new", &Value::Int(7)).map_err(|e| e.to_string())?;
    let seen = ctx
        .php
        .eval("t", "$r = &$GLOBALS['speedyspec_g']; return [$GLOBALS['speedyspec_g'], $GLOBALS['speedyspec_new']];")
        .map_err(|e| e.to_string())?;
    let expected = Value::Array(vec![
        (Key::Int(0), Value::Str("replaced".into())),
        (Key::Int(1), Value::Int(7)),
    ]);
    if seen != expected {
        return Err(format!("PHP saw {seen:?}"));
    }
    // Through a reference: the referenced variable changes too.
    speedyspec_php::set_global("speedyspec_g", &Value::Int(3)).map_err(|e| e.to_string())?;
    let via_ref = ctx.php.eval("t", "return $r;").map_err(|e| e.to_string())?;
    eq(&format!("{via_ref:?}"), "Int(3)")
}

fn case_fatal_is_caught(ctx: &Ctx) -> Result<(), String> {
    // Calling an undefined function throws Error in PHP 8.
    let (out, result) = capture(1, || ctx.php.require(&fixture("undefined_function.php")));
    eq(&out?, "")?;
    match result {
        Err(PhpError::Exception { class, message, .. })
            if class == "Error" && message.contains("speedyspec_fixture_undefined") => {}
        other => return Err(format!("undefined function: unexpected {other:?}")),
    }

    // Declaring a function twice is a compile-time engine fatal (a bailout).
    let (out, (err, result)) = capture(1, || {
        capture(2, || ctx.php.require(&fixture("redeclare.php")))
    });
    let (out, err) = (out?, err?);
    match result {
        Err(PhpError::Fatal {
            message,
            file,
            line,
        }) if message.contains("speedyspec_fixture_twice")
            && file.ends_with("redeclare.php")
            && line > 0 => {}
        other => return Err(format!("redeclare: unexpected {other:?}")),
    }
    // PHP reports it on stderr, never on stdout.
    eq(&out, "")?;
    if !err.contains("PHP Fatal error:") || !err.contains("speedyspec_fixture_twice") {
        return Err(format!(
            "PHP did not report the fatal error on stderr: {err:?}"
        ));
    }

    // The engine is still usable after the bailout.
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_fiber_in_callback(ctx: &Ctx) -> Result<(), String> {
    // A callback creates a fiber, which suspends back into the callback; the
    // callback resumes it to completion and the hook returns normally.
    eq(&require_out(ctx, "fiber_in_callback.php")?, "s,r1;done")
}

fn case_fiber_suspends_in_hook(ctx: &Ctx) -> Result<(), String> {
    // The firing pauses with the fiber and continues from the same callback,
    // then runs the remaining callbacks in order.
    eq(
        &require_out(ctx, "fiber_suspend_in_hook.php")?,
        "[aparked|main|b1c]|end",
    )
}

fn case_fiber_filter_suspends(ctx: &Ctx) -> Result<(), String> {
    // 1 + 1 = 2 is suspended with; resumed with 10: 2 * 10 + 100.
    eq(&require_out(ctx, "fiber_filter_suspends.php")?, "2,120")
}

fn case_fiber_parked_while_main_works(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "fiber_shared_park.php")?, "<xparked")?;
    // While fiber A is parked inside `fiber.shared`: fire it from PHP, add
    // and remove a callback, fire it from Rust, evaluate code.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "use SpeedySpec\\Hooks\\Hook;
                 Hook::run('fiber.shared');
                 echo '|';
                 $extra = static fn () => print('z');
                 Hook::add('fiber.shared', $extra);
                 Hook::run('fiber.shared');
                 echo Hook::remove('fiber.shared', $extra) ? '|R' : '|?';",
            )
        })?,
        "xy|xyz|R",
    )?;
    let (out, fired) = capture(1, || ctx.hooks.run("fiber.shared", &[]));
    fired.map_err(|e| e.to_string())?;
    eq(&out?, "xy")?;
    eq(&stdout_of(|| ctx.php.eval("t", "echo 2 + 2;"))?, "4")?;
    // Resumed, A finishes its own firing from the snapshot it started with.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "$GLOBALS['speedyspecParkedA']->resume('!');
                 echo $GLOBALS['speedyspecParkedA']->isTerminated() ? 'T' : 'S';",
            )
        })?,
        "!y>T",
    )
}

fn case_fiber_two_parked(ctx: &Ctx) -> Result<(), String> {
    // Parked in the order A, B; resumed B first.
    eq(
        &require_out(ctx, "fiber_two_parked.php")?,
        "A1B1|-B2B!+A2A!",
    )
}

fn case_fiber_dropped_while_parked(ctx: &Ctx) -> Result<(), String> {
    // PHP unwinds the destroyed fiber through the parked firing (finally
    // blocks run, the argument is released), and parking and dropping two
    // hundred more fibers does not grow memory.
    eq(
        &require_out(ctx, "fiber_dropped.php")?,
        "parked;unwound;fiber-finally;freed;dropped;0",
    )?;
    // A parked fiber whose last reference Rust holds is destroyed from the
    // bridge's own release queue, at the start of the next entry into PHP
    // (which settles what Rust let go of before running anything).
    let fiber = ctx
        .php
        .eval(
            "t",
            "return (static function (): Fiber
             {
                 $fiber = new Fiber(static function (): void
                 {
                     \\SpeedySpec\\Hooks\\Hook::run('fiber.drop', new stdClass());
                 });
                 $fiber->start();
                 return $fiber;
             })();",
        )
        .map_err(|e| e.to_string())?;
    if ctx.php.class_of(&fiber).as_deref() != Some("Fiber") {
        return Err(format!("expected a Fiber, got {fiber:?}"));
    }
    drop(fiber);
    eq(
        &stdout_of(|| ctx.php.eval("t", "echo 'next;';"))?,
        "unwound;next;",
    )?;

    // Nothing is left wedged: hooks, eval and a top-level exit() still work.
    let result = ctx
        .hooks
        .filter("php.double", Value::Int(21), &[])
        .map_err(|e| e.to_string())?;
    if result != Value::Int(42) {
        return Err(format!("firing from Rust gave {result:?}"));
    }
    match ctx.php.eval("t", "exit(5);") {
        Err(PhpError::Exit(5)) => {}
        other => return Err(format!("top-level exit: {other:?}")),
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_fiber_throw_into_parked(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "fiber_throw_into.php")?, "same")
}

fn case_fiber_frames_per_context(ctx: &Ctx) -> Result<(), String> {
    eq(
        &require_out(ctx, "fiber_frames_per_context.php")?,
        "from-main,from-fiber",
    )
}

fn case_fiber_frames_are_not_shared(ctx: &Ctx) -> Result<(), String> {
    // Fiber A is parked inside `ctx.park`, so its context has a firing frame.
    eq(&require_out(ctx, "fiber_ctx_frames.php")?, "")?;
    // Fired from Rust, the main context has no firing frame: the callback's
    // exception has no PHP caller to go back to and is only queued for
    // release. A frame stack shared between contexts would keep it in A's
    // frame instead, until A's firing ends.
    let (out, fired) = capture(1, || ctx.hooks.run("ctx.throw", &[]));
    eq(&out?, "")?;
    match fired {
        Err(err) if err.message.contains("noisy") => {}
        other => return Err(format!("ctx.throw: {other:?}")),
    }
    // Any native call drains the queue, before A is resumed.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "\\SpeedySpec\\Hooks\\Hook::ran('x'); echo 'mark;'; $GLOBALS['speedyspecCtxFiber']->resume();",
            )
        })?,
        "E-freed;mark;resumed;",
    )
}

fn case_fiber_exit_while_parked(ctx: &Ctx) -> Result<(), String> {
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "$GLOBALS['speedyspecExitFiber'] = new Fiber(static function (): void
                 {
                     \\SpeedySpec\\Hooks\\Hook::run('ctx.park');
                     echo 'fiber-done;';
                 });
                 $GLOBALS['speedyspecExitFiber']->start();
                 echo 'parked;';",
            )
        })?,
        "parked;",
    )?;
    // A top-level exit() is the main context's own: reported as Exit, not
    // parked for the fiber's native call.
    let (out, result) = capture(1, || ctx.php.eval("t", "echo 'exiting;'; exit(7);"));
    eq(&out?, "exiting;")?;
    match result {
        Err(PhpError::Exit(7)) => {}
        other => return Err(format!("expected Err(Exit(7)), got {other:?}")),
    }
    // The parked fiber is untouched and completes its firing.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "$GLOBALS['speedyspecExitFiber']->resume();
                 echo $GLOBALS['speedyspecExitFiber']->isTerminated() ? 'T' : 'S';",
            )
        })?,
        "resumed;fiber-done;T",
    )
}

fn case_fiber_destroyed_by_gc(ctx: &Ctx) -> Result<(), String> {
    // Leaving scope does not destroy the cycle; gc_collect_cycles() does,
    // unwinding the parked firing; 200 more cycles do not grow memory.
    eq(
        &require_out(ctx, "fiber_gc_cycle.php")?,
        "out-of-scope;unwound;collected;0",
    )?;
    let result = ctx
        .hooks
        .filter("php.double", Value::Int(21), &[])
        .map_err(|e| e.to_string())?;
    if result != Value::Int(42) {
        return Err(format!("firing from Rust gave {result:?}"));
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_fiber_abandoned_after_fatal(ctx: &Ctx) -> Result<(), String> {
    // Fibers parked inside a hook when a fatal error marks every object
    // destructed are later freed without being unwound: their records are
    // abandoned, until a new fiber reuses the address or `Php` is dropped.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "\\SpeedySpec\\Hooks\\Hook::add('ctx.abandon', static function (): void
                 {
                     try
                     {
                         Fiber::suspend();
                     }
                     finally
                     {
                         echo 'unwound?';
                     }
                 });
                 $GLOBALS['speedyspecAbandoned'] = [];
                 for ($i = 0; $i < 3; $i++)
                 {
                     $fiber = new Fiber(static function (): void
                     {
                         \\SpeedySpec\\Hooks\\Hook::run('ctx.abandon');
                     });
                     $fiber->start();
                     $GLOBALS['speedyspecAbandoned'][] = $fiber;
                 }
                 unset($fiber);",
            )
        })?,
        "",
    )?;
    let parked = speedyspec_php::fiber_records();
    let (_, result) = capture(1, || quiet(|| ctx.php.require(&fixture("redeclare.php"))));
    match result {
        Err(PhpError::Fatal { .. }) => {}
        other => return Err(format!("expected a fatal error, got {other:?}")),
    }
    // Freed, not unwound: no finally block runs, the records stay.
    eq(
        &stdout_of(|| {
            ctx.php
                .eval("t", "unset($GLOBALS['speedyspecAbandoned']); echo 'gone;';")
        })?,
        "gone;",
    )?;
    if speedyspec_php::fiber_records() != parked {
        return Err(format!(
            "records went from {parked} to {} although nothing was unwound",
            speedyspec_php::fiber_records()
        ));
    }
    // New fibers that park and finish inside a hook still work, and one that
    // lands on an abandoned fiber's address retires its stale record. (The
    // count cannot tell whether `on_fiber_init` dropped it or the first
    // switch into the new fiber consumed it; an abandoned record is inert
    // either way, see the fix report.)
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "$fiber = new Fiber(static function (): void
                 {
                     \\SpeedySpec\\Hooks\\Hook::run('ctx.park');
                 });
                 $fiber->start();
                 $fiber->resume();
                 unset($fiber);",
            )
        })?,
        "resumed;",
    )?;
    let after = speedyspec_php::fiber_records();
    // Whether the new fiber lands on an abandoned fiber's address depends on
    // the allocator; it must never add a record.
    if after > parked {
        return Err(format!(
            "a finished fiber left a record behind: {parked} -> {after}"
        ));
    }
    Ok(())
}

fn case_fiber_exit_in_callback(ctx: &Ctx) -> Result<(), String> {
    // exit() in a callback inside a fiber ends the whole script.
    let (out, result) = capture(1, || {
        ctx.php.require(&fixture("fiber_exit_in_callback.php"))
    });
    eq(&out?, "")?;
    match result {
        Err(PhpError::Exit(6)) => {}
        other => return Err(format!("expected Err(Exit(6)), got {other:?}")),
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_fiber_suspend_outside_fiber(ctx: &Ctx) -> Result<(), String> {
    // PHP's own rule, propagated as an ordinary exception: fired from PHP...
    eq(
        &require_out(ctx, "fiber_suspend_outside.php")?,
        "Cannot suspend outside of a fiber",
    )?;
    // ...and fired from Rust.
    match ctx.hooks.run("fiber.outside", &[]) {
        Err(err)
            if err.message.contains("FiberError")
                && err.message.contains("Cannot suspend outside of a fiber") =>
        {
            Ok(())
        }
        other => Err(format!("from Rust: {other:?}")),
    }
}

fn case_fatal_while_fiber_parked(ctx: &Ctx) -> Result<(), String> {
    // Park a fiber inside a hook, hit a fatal error at the top level, then
    // resume the fiber: it completes its firing.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "$GLOBALS['speedyspecParkedB'] = new Fiber(static function (): void
                 {
                     \\SpeedySpec\\Hooks\\Hook::run('fiber.shared');
                     echo 'B-done';
                 });
                 echo $GLOBALS['speedyspecParkedB']->start();",
            )
        })?,
        "xparked",
    )?;
    let (_, result) = capture(1, || quiet(|| ctx.php.require(&fixture("redeclare.php"))));
    match result {
        Err(PhpError::Fatal { .. }) => {}
        other => return Err(format!("expected a fatal error, got {other:?}")),
    }
    eq(
        &stdout_of(|| {
            ctx.php
                .eval("t", "$GLOBALS['speedyspecParkedB']->resume('+');")
        })?,
        "+yB-done",
    )?;
    if ctx.php.poisoned() {
        Ok(())
    } else {
        Err("not poisoned".to_owned())
    }
}

fn case_panic_inside_fiber(ctx: &Ctx) -> Result<(), String> {
    // A panicking Rust callback fired inside a fiber: PHP carries the bailout
    // out of the fiber into the resuming context, where the panic resumes.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (out, caught) = capture(1, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.php.require(&fixture("fiber_panic.php"))
        }))
    });
    std::panic::set_hook(hook);
    match caught {
        Err(payload) if matches!(payload.downcast_ref::<PanicMarker>(), Some(PanicMarker(7))) => {}
        Err(_) => return Err("a different panic came out".to_owned()),
        Ok(result) => return Err(format!("no panic came out: {result:?}")),
    }
    if out?.contains("not reached") {
        return Err("PHP kept running after the panic".to_owned());
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_fiber_parked_at_shutdown(ctx: &Ctx) -> Result<(), String> {
    // Leaves a fiber parked inside a hook; main() checks, after dropping
    // `Php`, that PHP unwound it through the parked firing.
    let seen = ctx.shutdown_seen.clone();
    add_rust(ctx, "fiber.shutdown.seen", move |_| {
        seen.set(true);
        Ok(Value::Null)
    })?;
    eq(&require_out(ctx, "fiber_parked_at_shutdown.php")?, "parked")?;
    if ctx.shutdown_seen.get() {
        return Err("the fiber was unwound too early".to_owned());
    }
    Ok(())
}

fn case_fiber_fires_hook(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "fiber_fires_hook.php")?, "2,11")
}

fn case_fiber_without_hooks(ctx: &Ctx) -> Result<(), String> {
    eq(&require_out(ctx, "fiber_without_hooks.php")?, "1,2,done:3")
}

fn case_execution_context_per_fiber(ctx: &Ctx) -> Result<(), String> {
    // A Rust callback sees which context fired it: the main one, then a fiber,
    // then the same fiber again after it was suspended and resumed.
    let seen: Rc<RefCell<Vec<usize>>> = Rc::default();
    let record = seen.clone();
    add_rust(ctx, "ctx.where", move |_| {
        record
            .borrow_mut()
            .push(speedyspec_php::execution_context());
        Ok(Value::Null)
    })?;
    stdout_of(|| {
        ctx.php.eval(
            "t",
            "\\SpeedySpec\\Hooks\\Hook::run('ctx.where');
             $fiber = new Fiber(static function (): void
             {
                 \\SpeedySpec\\Hooks\\Hook::run('ctx.where');
                 Fiber::suspend();
                 \\SpeedySpec\\Hooks\\Hook::run('ctx.where');
             });
             $fiber->start();
             \\SpeedySpec\\Hooks\\Hook::run('ctx.where');
             $fiber->resume();",
        )
    })?;
    let main = speedyspec_php::execution_context();
    match seen.borrow().as_slice() {
        [a, f1, b, f2] if *a == main && *b == main && f1 == f2 && *f1 != main && main != 0 => {
            Ok(())
        }
        other => Err(format!("contexts seen: {other:?}, main {main}")),
    }
}

fn case_not_poisoned_before_fatal(ctx: &Ctx) -> Result<(), String> {
    if ctx.php.poisoned() {
        Err("poisoned before any fatal error".to_owned())
    } else {
        Ok(())
    }
}

/// Code that exhausts the default 128M `memory_limit`.
const OOM: &str = "(function () { $a = []; while (true) { $a[] = str_repeat('x', 1000000); } })();";

fn is_oom(result: &Result<Value, PhpError>) -> bool {
    matches!(result, Err(PhpError::Fatal { message, .. }) if message.contains("Allowed memory size"))
}

fn case_oom_is_fatal(ctx: &Ctx) -> Result<(), String> {
    // This PHP reports memory exhaustion as a "delayed OOM": it unwinds with
    // exit status 255 and leaves EG(delayed_oom) set, which makes every
    // later zend_call_function fail. It must come back as a fatal error,
    // poison the engine and leave it usable.
    let (out, (err, result)) = capture(1, || capture(2, || ctx.php.eval("oom", OOM)));
    eq(&out?, "")?;
    if !is_oom(&result) {
        return Err(format!(
            "top level: expected a memory fatal, got {result:?}"
        ));
    }
    if !err?.contains("Allowed memory size") {
        return Err("PHP did not report the memory fatal on stderr".to_owned());
    }
    if !ctx.php.poisoned() {
        return Err("not poisoned after memory exhaustion".to_owned());
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")?;

    // PHP callbacks still run, fired from PHP and from Rust.
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "\\SpeedySpec\\Hooks\\Hook::add('oom.after', static fn (int $v): int => $v + 1);
                 echo \\SpeedySpec\\Hooks\\Hook::filter('oom.after', 1);",
            )
        })?,
        "2",
    )?;
    match ctx.hooks.filter("oom.after", Value::Int(5), &[]) {
        Ok(Value::Int(6)) => {}
        other => return Err(format!("firing from Rust after OOM: {other:?}")),
    }

    // In a callback fired from PHP it abandons the PHP caller as a fatal
    // error, not as exit(255).
    let (out, result) = capture(1, || quiet(|| ctx.php.require(&fixture("nested_oom.php"))));
    if out?.contains("not reached") {
        return Err("PHP kept running after memory exhaustion in a callback".to_owned());
    }
    if !is_oom(&result) {
        return Err(format!("from PHP: expected a memory fatal, got {result:?}"));
    }
    // Fired from Rust it is that callback's error.
    match quiet(|| ctx.hooks.run("nested.oom", &[])) {
        Err(err)
            if err.kind == HookErrorKind::Callback
                && err.message.contains("Allowed memory size") => {}
        other => return Err(format!("from Rust: unexpected {other:?}")),
    }
    // A value from Rust that crosses the limit while it is converted: PHP
    // handles the queued error at the start of the call it was meant for.
    let big = Value::Str("x".repeat(200_000_000));
    let called = quiet(|| ctx.php.call(&Value::Str("strlen".into()), &[big]));
    if !is_oom(&called) {
        return Err(format!(
            "argument from Rust: expected a memory fatal, got {called:?}"
        ));
    }
    // Top-level code whose last statement crosses the limit.
    let tail = quiet(|| {
        ctx.php.eval(
            "t",
            "$GLOBALS['speedyspecBig'] = str_repeat('x', 200000000);",
        )
    });
    let _ = ctx.php.eval("t", "unset($GLOBALS['speedyspecBig']);");
    if !is_oom(&tail) {
        return Err(format!(
            "last statement: expected a memory fatal, got {tail:?}"
        ));
    }
    // Php::call reports it the same way.
    let closure = ctx
        .php
        .eval(
            "t",
            &format!("return static function (): void {{ {OOM} }};"),
        )
        .map_err(|e| e.to_string())?;
    let called = quiet(|| ctx.php.call(&closure, &[]));
    if !is_oom(&called) {
        return Err(format!(
            "Php::call: expected a memory fatal, got {called:?}"
        ));
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_poisoned_after_fatal(ctx: &Ctx) -> Result<(), String> {
    if ctx.php.poisoned() {
        Ok(())
    } else {
        Err("not poisoned after a fatal error".to_owned())
    }
}

/// Runs `code` with output on stdout; returns (stdout, stderr, result).
fn diagnose(ctx: &Ctx, code: &str) -> Result<(String, String, Result<Value, PhpError>), String> {
    ctx.php.redirect_output(Output::Stdout);
    let (out, (err, result)) = capture(1, || capture(2, || ctx.php.eval("diag", code)));
    Ok((out?, err?, result))
}

fn case_warning_goes_to_stderr(ctx: &Ctx) -> Result<(), String> {
    let (out, err, result) = diagnose(ctx, "$a = []; $v = $a['missing']; echo 'x';")?;
    result.map_err(|e| e.to_string())?;
    eq(&out, "x")?;
    if err.contains("PHP Warning:") && err.contains("Undefined array key \"missing\"") {
        Ok(())
    } else {
        Err(format!("stderr: {err:?}"))
    }
}

fn case_deprecation_goes_to_stderr(ctx: &Ctx) -> Result<(), String> {
    let (out, err, result) = diagnose(
        ctx,
        "trigger_error('speedyspec old api', E_USER_DEPRECATED); echo 'x';",
    )?;
    result.map_err(|e| e.to_string())?;
    eq(&out, "x")?;
    if err.contains("PHP Deprecated:") && err.contains("speedyspec old api") {
        Ok(())
    } else {
        Err(format!("stderr: {err:?}"))
    }
}

fn case_fatal_goes_to_stderr(ctx: &Ctx) -> Result<(), String> {
    let (out, err, result) = diagnose(
        ctx,
        "echo 'x'; function speedyspec_diag_twice(): void {} function speedyspec_diag_twice(): void {}",
    )?;
    eq(&out, "")?;
    match result {
        Err(PhpError::Fatal { message, line, .. })
            if message.contains("speedyspec_diag_twice") && line > 0 => {}
        other => return Err(format!("expected a fatal error, got {other:?}")),
    }
    if err.contains("PHP Fatal error:") && err.contains("speedyspec_diag_twice") {
        Ok(())
    } else {
        Err(format!("stderr: {err:?}"))
    }
}

fn case_uncaught_in_run_file_not_on_stdout(ctx: &Ctx) -> Result<(), String> {
    ctx.php.redirect_output(Output::Stdout);
    let (out, (err, result)) = capture(1, || {
        capture(2, || ctx.php.run_file(&fixture("uncaught.php"), &[]))
    });
    // Only the script's own output; the exception comes back as an error.
    eq(&out?, "before;")?;
    err?;
    match result {
        Err(PhpError::Exception { class, message, .. })
            if class == "RuntimeException" && message == "uncaught here" =>
        {
            Ok(())
        }
        other => Err(format!("expected the exception, got {other:?}")),
    }
}

fn case_fatal_inside_callback(ctx: &Ctx) -> Result<(), String> {
    // A fatal error in a callback fired from PHP abandons the PHP caller too,
    // after the Rust frames in between have returned normally.
    let (out, result) = capture(1, || {
        quiet(|| ctx.php.require(&fixture("nested_fatal.php")))
    });
    let out = out?;
    match result {
        Err(PhpError::Fatal { message, .. }) if message.contains("speedyspec_fixture_twice") => {}
        other => return Err(format!("from PHP: unexpected {other:?}")),
    }
    if out.contains("not reached") {
        return Err("PHP kept running after the fatal error".to_owned());
    }

    // A fatal error in a destructor that runs when Rust releases its last
    // reference, at the end of a native call, unwinds that call's PHP caller.
    let (out, result) = capture(1, || quiet(|| ctx.php.require(&fixture("drain_fatal.php"))));
    let out = out?;
    match result {
        Err(PhpError::Fatal { message, .. }) if message.contains("speedyspec_fixture_twice") => {}
        other => return Err(format!("from a destructor: unexpected {other:?}")),
    }
    if out.contains("not reached") {
        return Err("PHP kept running after the fatal error in a destructor".to_owned());
    }

    // Fired from Rust, the same fatal error is that callback's error.
    let (_, result) = capture(1, || quiet(|| ctx.hooks.run("nested.fatal", &[])));
    match result {
        Err(err)
            if err.kind == HookErrorKind::Callback
                && err.message.contains("speedyspec_fixture_twice") => {}
        other => return Err(format!("from Rust: unexpected {other:?}")),
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_exit_inside_callback(ctx: &Ctx) -> Result<(), String> {
    // exit() in a callback ends the PHP script that fired the hook, skipping
    // its finally block as exit() does.
    let (out, result) = capture(1, || ctx.php.require(&fixture("nested_exit.php")));
    eq(&out?, "")?;
    match result {
        Err(PhpError::Exit(4)) => {}
        other => return Err(format!("from PHP: expected Err(Exit(4)), got {other:?}")),
    }
    match ctx.hooks.run("nested.exit", &[]) {
        Err(err) if err.kind == HookErrorKind::Callback && err.message.contains("exit(4)") => {}
        other => return Err(format!("from Rust: unexpected {other:?}")),
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

fn case_panic_propagates(ctx: &Ctx) -> Result<(), String> {
    // A panicking Rust callback fired from PHP: the panic comes out of
    // require() as the same panic, not as a PHP error.
    add_rust(ctx, "rust.panics", |_| {
        std::panic::panic_any(PanicMarker(7));
    })?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (out, caught) = capture(1, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.php.require(&fixture("nested_panic.php"))
        }))
    });
    std::panic::set_hook(hook);
    match caught {
        Err(payload) => match payload.downcast_ref::<PanicMarker>() {
            Some(PanicMarker(7)) => {}
            _ => return Err("a different panic came out".to_owned()),
        },
        Ok(result) => return Err(format!("no panic came out: {result:?}")),
    }
    if out?.contains("not reached") {
        return Err("PHP kept running after the panic".to_owned());
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'alive';"))?, "alive")
}

struct PanicMarker(u8);

// ---------------------------------------------------------------------------
// Conversion, boot options and output

fn case_array_round_trip(ctx: &Ctx) -> Result<(), String> {
    let seen: Rc<RefCell<Option<Value>>> = Rc::default();
    let record = seen.clone();
    add_rust(ctx, "array.identity", move |args| {
        let value = args.first().cloned().unwrap_or(Value::Null);
        *record.borrow_mut() = Some(value.clone());
        Ok(value)
    })?;
    eq(&require_out(ctx, "array_round_trip.php")?, "equal")?;

    let expected = Value::Array(vec![
        (Key::Str("a".into()), Value::Int(1)),
        (Key::Int(5), Value::Str("x".into())),
        (
            Key::Str("n".into()),
            Value::Array(vec![
                (Key::Int(0), Value::Bool(true)),
                (Key::Int(1), Value::Null),
            ]),
        ),
    ]);
    match seen.borrow().as_ref() {
        Some(v) if *v == expected => Ok(()),
        other => Err(format!("Rust saw {other:?}")),
    }
}

fn case_preload_is_available(ctx: &Ctx) -> Result<(), String> {
    eq(
        &stdout_of(|| ctx.php.eval("t", "echo class_exists('PreT');"))?,
        "1",
    )
}

fn case_construct_builds_an_object(ctx: &Ctx) -> Result<(), String> {
    ctx.php
        .eval(
            "t",
            "final class ConstructP { public function __construct(public string $a, public array $b) {} }",
        )
        .map_err(|e| e.to_string())?;
    let pairs = Value::Array(vec![(Key::Str("k".to_owned()), Value::Str("v".to_owned()))]);
    let object = ctx
        .php
        .construct("ConstructP", &[Value::Str("x".to_owned()), pairs.clone()])
        .map_err(|e| e.to_string())?;
    eq(
        &format!("{:?}", ctx.php.class_of(&object)),
        "Some(\"ConstructP\")",
    )?;
    let properties = ctx.php.properties_of(&object).ok_or("not an object")?;
    if properties
        != vec![
            ("a".to_owned(), Value::Str("x".to_owned())),
            ("b".to_owned(), pairs),
        ]
    {
        return Err(format!("unexpected properties: {properties:?}"));
    }
    Ok(())
}

fn case_construct_unknown_class(ctx: &Ctx) -> Result<(), String> {
    match ctx.php.construct("NopeNoSuchClass", &[]) {
        Err(PhpError::Exception { message, .. }) if message.contains("NopeNoSuchClass") => {}
        other => {
            return Err(format!(
                "expected an exception naming the class, got {other:?}"
            ));
        }
    }
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'still';"))?, "still")
}

fn case_construct_wrong_args(ctx: &Ctx) -> Result<(), String> {
    match ctx.php.construct("ConstructP", &[]) {
        Err(PhpError::Exception { class, .. }) if class == "ArgumentCountError" => Ok(()),
        other => Err(format!("expected ArgumentCountError, got {other:?}")),
    }
}

fn case_output_redirect(ctx: &Ctx) -> Result<(), String> {
    ctx.php.redirect_output(Output::Stderr);
    let (stdout, (stderr, result)) = capture(1, || capture(2, || ctx.php.eval("t", "echo 'x';")));
    ctx.php.redirect_output(Output::Stdout);
    result.map_err(|e| e.to_string())?;
    eq(&stdout?, "")?;
    eq(&stderr?, "x")?;
    eq(&stdout_of(|| ctx.php.eval("t", "echo 'y';"))?, "y")
}

fn case_backend_error_is_exception(ctx: &Ctx) -> Result<(), String> {
    ctx.hooks.backend_down.set(true);
    let out = require_out(ctx, "backend_error.php");
    ctx.hooks.backend_down.set(false);
    // The message names the hook (and the owner, when there is one).
    eq(&out?, "backend.down: down")?;
    // A Rust callback's error, fired from PHP, names its owner too.
    ctx.hooks
        .add(
            "rust.fails",
            Rc::new(|_: &[Value]| Err(CallbackError("nope".into()))),
            AddOptions {
                owner: Some("acme/rust".into()),
                ..AddOptions::default()
            },
        )
        .map_err(|e| e.to_string())?;
    eq(
        &stdout_of(|| {
            ctx.php.eval(
                "t",
                "try
                 {
                     \\SpeedySpec\\Hooks\\Hook::run('rust.fails');
                 }
                 catch (\\SpeedySpec\\Hooks\\HookException $e)
                 {
                     echo $e->getMessage();
                 }",
            )
        })?,
        "rust.fails: nope (registered by acme/rust)",
    )
}

fn case_call_and_inspect(ctx: &Ctx) -> Result<(), String> {
    let php = &ctx.php;
    let upper = php
        .call(&Value::Str("strtoupper".into()), &[Value::Str("ab".into())])
        .map_err(|e| e.to_string())?;
    if upper != Value::Str("AB".into()) {
        return Err(format!("strtoupper: {upper:?}"));
    }

    let closure = php
        .eval("t", "return static fn (int $x): int => $x + 1;")
        .map_err(|e| e.to_string())?;
    if php.class_of(&closure).as_deref() != Some("Closure") {
        return Err(format!("class_of(closure) = {:?}", php.class_of(&closure)));
    }
    let two = php
        .call(&closure, &[Value::Int(1)])
        .map_err(|e| e.to_string())?;
    if two != Value::Int(2) {
        return Err(format!("closure returned {two:?}"));
    }

    let object = php
        .eval(
            "t",
            "return new class { public $a = 1; protected $b = 2; private $p = 3; public int $unset; public $c = 'z'; };",
        )
        .map_err(|e| e.to_string())?;
    let props = php.properties_of(&object);
    let expected = vec![
        ("a".to_owned(), Value::Int(1)),
        ("c".to_owned(), Value::Str("z".into())),
    ];
    if props.as_ref() != Some(&expected) {
        return Err(format!("properties_of = {props:?}"));
    }
    if php.class_of(&Value::Int(1)).is_some() || php.properties_of(&Value::Int(1)).is_some() {
        return Err("class_of/properties_of answered for a non-object".to_owned());
    }

    // An exception thrown by a callable called from Rust comes back as an error.
    let thrower = php
        .eval(
            "t",
            "return static function (): never { throw new LogicException('nope'); };",
        )
        .map_err(|e| e.to_string())?;
    match php.call(&thrower, &[]) {
        Err(PhpError::Exception { class, message, .. })
            if class == "LogicException" && message == "nope" =>
        {
            Ok(())
        }
        other => Err(format!("thrower: {other:?}")),
    }
}

fn case_stray_destructor_exception(ctx: &Ctx) -> Result<(), String> {
    // An object whose destructor throws, released by Rust between calls: the
    // exception belongs to no call. It is logged and cleared, and the next
    // call, inspection and eval are not blamed for it.
    let thrower = || {
        ctx.php.eval(
            "t",
            "return new class
             {
                 public function __destruct()
                 {
                     throw new RuntimeException('from a destructor');
                 }
             };",
        )
    };
    let discarded = |err: Result<String, String>| -> Result<(), String> {
        let err = err?;
        if err.contains("speedyspec: discarded an uncaught RuntimeException (from a destructor)")
            && !err.contains("PHP Fatal error")
        {
            Ok(())
        } else {
            Err(format!("stderr: {err:?}"))
        }
    };

    drop(thrower().map_err(|e| e.to_string())?);
    let (err, upper) = capture(2, || {
        ctx.php
            .call(&Value::Str("strtoupper".into()), &[Value::Str("ab".into())])
    });
    match upper {
        Ok(Value::Str(s)) if s == "AB" => {}
        other => return Err(format!("call after a stray exception: {other:?}")),
    }
    discarded(err)?;

    let object = ctx
        .php
        .eval("t", "return new class { public $a = 1; };")
        .map_err(|e| e.to_string())?;
    drop(thrower().map_err(|e| e.to_string())?);
    let (err, props) = capture(2, || ctx.php.properties_of(&object));
    if props != Some(vec![("a".to_owned(), Value::Int(1))]) {
        return Err(format!("properties_of after a stray exception: {props:?}"));
    }
    discarded(err)?;

    drop(thrower().map_err(|e| e.to_string())?);
    let (err, out) = capture(2, || stdout_of(|| ctx.php.eval("t", "echo 'clean';")));
    eq(&out?, "clean")?;
    discarded(err)?;

    // A PHP callback fired from Rust is not blamed either: it returns its own
    // value, and the destructor's exception is logged, not returned.
    ctx.php
        .eval(
            "t",
            "\\SpeedySpec\\Hooks\\Hook::add('stray.filter', static fn (int $v): int => $v + 1);",
        )
        .map_err(|e| e.to_string())?;
    drop(thrower().map_err(|e| e.to_string())?);
    let (err, filtered) = capture(2, || ctx.hooks.filter("stray.filter", Value::Int(1), &[]));
    match filtered {
        Ok(Value::Int(2)) => {}
        other => {
            return Err(format!(
                "filter from Rust after a stray exception: {other:?}"
            ));
        }
    }
    discarded(err)
}

fn case_refcounts_balance(ctx: &Ctx) -> Result<(), String> {
    add_rust(ctx, "refcount.rust", |args| {
        Ok(args.first().cloned().unwrap_or(Value::Null))
    })?;
    // The object is freed exactly when PHP drops its last reference, so no
    // reference taken while it travelled through the hooks is left behind;
    // and a thousand more firings do not grow memory.
    eq(&require_out(ctx, "refcounts.php")?, "before;freed;after;0")?;

    // Firing from Rust with an object does not grow memory either.
    let object = ctx
        .php
        .eval("t", "return new stdClass();")
        .map_err(|e| e.to_string())?;
    let usage = || -> Result<i64, String> {
        match ctx.php.eval("t", "return memory_get_usage();") {
            Ok(Value::Int(n)) => Ok(n),
            other => Err(format!("memory_get_usage: {other:?}")),
        }
    };
    // A new handle to the same PHP object is a different Rc, so compare PHP's
    // own identity.
    let id = |v: &Value| {
        ctx.php
            .call(&Value::Str("spl_object_id".into()), std::slice::from_ref(v))
            .map_err(|e| e.to_string())
    };
    let object_id = id(&object)?;
    let fire = || -> Result<(), String> {
        let v = ctx
            .hooks
            .filter(
                "refcount.php",
                object.clone(),
                &[
                    Value::Array(vec![(Key::Int(0), object.clone())]),
                    Value::Null,
                ],
            )
            .map_err(|e| e.to_string())?;
        if id(&v)? == object_id {
            Ok(())
        } else {
            Err(format!("a different object came back: {v:?}"))
        }
    };
    for _ in 0..10 {
        fire()?;
    }
    let before = usage()?;
    for _ in 0..1000 {
        fire()?;
    }
    let grown = usage()? - before;
    if grown != 0 {
        return Err(format!(
            "memory grew by {grown} bytes over 1000 firings from Rust"
        ));
    }
    Ok(())
}

/// After `Php` is dropped, PHP callbacks still in the registry must not run
/// (or be released): the engine that owned them is gone.
fn after_shutdown(hooks: &TestHooks) -> Result<(), String> {
    match hooks.filter("php.double", Value::Int(1), &[]) {
        Err(err) if err.kind == HookErrorKind::Callback => Ok(()),
        other => Err(format!("expected a callback error, got {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Helpers

/// The hook backend for the whole run: [`MemoryHooks`], except that `filter`
/// fails with a backend error while `backend_down` is set.
#[derive(Default)]
struct TestHooks {
    memory: MemoryHooks,
    backend_down: Cell<bool>,
}

impl Hooks for TestHooks {
    fn add(
        &self,
        hook: &str,
        callback: Rc<Callback>,
        options: AddOptions,
    ) -> Result<CallbackId, HookError> {
        self.memory.add(hook, callback, options)
    }
    fn remove(&self, hook: &str, id: CallbackId) -> Result<bool, HookError> {
        self.memory.remove(hook, id)
    }
    fn remove_keyed(&self, hook: &str, key: &str, priority: i64) -> Result<bool, HookError> {
        self.memory.remove_keyed(hook, key, priority)
    }
    fn has(&self, hook: &str) -> Result<bool, HookError> {
        self.memory.has(hook)
    }
    fn has_keyed(&self, hook: &str, key: &str) -> Result<bool, HookError> {
        self.memory.has_keyed(hook, key)
    }
    fn run(&self, hook: &str, args: &[Value]) -> Result<(), HookError> {
        self.memory.run(hook, args)
    }
    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Result<Value, HookError> {
        if self.backend_down.get() {
            return Err(HookError {
                kind: HookErrorKind::Backend,
                hook: hook.to_owned(),
                owner: None,
                message: "down".to_owned(),
            });
        }
        self.memory.filter(hook, value, args)
    }
    fn ran(&self, hook: &str) -> Result<u64, HookError> {
        self.memory.ran(hook)
    }
}

fn add_rust(
    ctx: &Ctx,
    hook: &str,
    f: impl Fn(&[Value]) -> Result<Value, CallbackError> + 'static,
) -> Result<(), String> {
    ctx.hooks
        .add(hook, Rc::new(f), AddOptions::default())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn eq(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected:?}, got {actual:?}"))
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Requires a fixture and returns what it printed; any error fails the case.
fn require_out(ctx: &Ctx, name: &str) -> Result<String, String> {
    stdout_of(|| ctx.php.require(&fixture(name)).map(|_| ()))
}

/// Runs `f`, which must succeed, and returns what it printed to stdout.
fn stdout_of<T>(f: impl FnOnce() -> Result<T, PhpError>) -> Result<String, String> {
    let (out, result) = capture(1, f);
    match result {
        Ok(_) => out,
        Err(err) => Err(format!("{err} (output: {:?})", out.unwrap_or_default())),
    }
}

/// Runs `f` with file descriptor `fd` (1 or 2) pointed at a temporary file and
/// returns what was written to it. PHP writes through the process's stdout and
/// stderr, so this sees exactly what a user of the CLI would see.
fn capture<T>(fd: i32, f: impl FnOnce() -> T) -> (Result<String, String>, T) {
    let path = std::env::temp_dir().join(format!(
        "speedyspec-php-test-{}-{fd}.out",
        std::process::id()
    ));
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path);
    let _ = std::fs::remove_file(&path);
    let mut file = match file {
        Ok(file) => file,
        Err(e) => return (Err(format!("open {}: {e}", path.display())), f()),
    };

    flush(fd);
    // SAFETY: plain descriptor juggling on fds this function owns or restores.
    let saved = unsafe { libc::dup(fd) };
    if saved < 0 || unsafe { libc::dup2(file.as_raw_fd(), fd) } < 0 {
        return (Err("could not redirect output".to_owned()), f());
    }

    let result = f();

    flush(fd);
    // SAFETY: restores the descriptor saved above and closes the copy.
    unsafe {
        libc::dup2(saved, fd);
        libc::close(saved);
    }

    let mut out = String::new();
    let read = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.read_to_string(&mut out))
        .map(|_| out)
        .map_err(|e| e.to_string());
    (read, result)
}

/// Runs `f` with stderr captured and discarded: for cases where PHP
/// legitimately reports a diagnostic that the case does not check.
fn quiet<T>(f: impl FnOnce() -> T) -> T {
    capture(2, f).1
}

fn flush(fd: i32) {
    if fd == 1 {
        let _ = std::io::stdout().flush();
    } else {
        let _ = std::io::stderr().flush();
    }
}
