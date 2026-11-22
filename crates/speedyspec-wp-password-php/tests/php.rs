//! WordPress's password functions in embedded PHP: the facade over the native functions and the container ports,
//! with libsodium and bcrypt behind them, replaying what `compat/password/record.php` recorded from WordPress 7.1.5.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each check in order.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_password_contracts::{PASSWORD_HASHER, PasswordHasher};
use speedyspec_wp_password_php::PasswordArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn is_true(r: &Result<HookValue, PhpError>) -> bool {
    matches!(r, Ok(HookValue::Bool(true)))
}

fn main() {
    let compat = format!("{}/../../compat/password/", env!("CARGO_MANIFEST_DIR"));
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let sodium = Rc::new(Sodium::init().expect("libsodium initializes"));
    let password = PasswordArea::new(hooks.api(), container.clone(), sodium);
    password.bind_defaults();
    let container_area = PasswordArea::ports(ContainerArea::new("test", container.clone()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(password),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let eval = |label: &str, code: &str| php.eval(&format!("speedyspec://test/{label}"), code);

    for name in [
        "wp_hash_password",
        "wp_check_password",
        "wp_password_needs_rehash",
        "wp_generate_password",
        "wp_rand",
    ] {
        let r = eval(
            "facade",
            &format!("return (new ReflectionFunction('{name}'))->getFileName();"),
        );
        check(
            &format!("{name}_is_the_facade"),
            matches!(&r, Ok(HookValue::Str(s)) if s == "speedyspec://wp-password"),
            &r,
        );
    }

    let r = eval(
        "signatures",
        "$f = static fn ($n) => array_map(static fn ($p) => [$p->getName(), $p->isOptional() ? var_export($p->getDefaultValue(), true) : null], (new ReflectionFunction($n))->getParameters());
         return $f('wp_hash_password') === [['password', null]]
             && $f('wp_check_password') === [['password', null], ['hash', null], ['user_id', \"''\"]]
             && $f('wp_password_needs_rehash') === [['hash', null], ['user_id', \"''\"]]
             && $f('wp_generate_password') === [['length', '12'], ['special_chars', 'true'], ['extra_special_chars', 'false']]
             && $f('wp_rand') === [['min', 'NULL'], ['max', 'NULL']];",
    );
    check("signatures_match_wordpress", is_true(&r), &r);

    let r = eval(
        "round-trip",
        "$h = wp_hash_password(' pw ');
         return str_starts_with($h, '$argon2id$v=19$m=65536,t=4,p=1$') && 97 === strlen($h)
             && password_verify('pw', $h) && wp_check_password('pw', $h) && !wp_check_password('px', $h)
             && !wp_check_password(' pw ', $h) && !wp_password_needs_rehash($h);",
    );
    check("round_trip_with_php", is_true(&r), &r);

    let r = eval(
        "binary",
        "$p = \"\\xff\\x00pw\";
         $a = wp_hash_password($p);
         add_filter('wp_hash_password_algorithm', static fn () => PASSWORD_BCRYPT);
         $b = wp_hash_password($p);
         remove_all_filters('wp_hash_password_algorithm');
         return wp_check_password($p, $a) && !wp_check_password(\"\\xff\", $a)
             && str_starts_with($b, '$wp$2y$') && wp_check_password($p, $b) && !wp_check_password(\"\\xff\\x00px\", $b);",
    );
    check("binary_password_round_trips", is_true(&r), &r);

    // Every recorded check, through the global function, libsodium and bcrypt.
    let r = eval(
        "replay-check",
        &format!(
            r#"$cases = json_decode(file_get_contents({compat:?} . 'cases.json'), true);
            $pw = static fn ($p) => is_array($p) ? str_repeat(hex2bin($p['repeat']), $p['times']) : hex2bin($p);
            $divergent = ['md5', 'phpass', '2x', 'md5-crypt', 'sha256-crypt', 'sha512-crypt'];
            $fail = [];
            foreach ($cases['check'] as $c) {{
                $want = in_array($c['format'], $divergent, true) ? false : $c['result'];
                if (wp_check_password($pw($c['password']), $c['hash']) !== $want) {{ $fail[] = $c['name']; }}
            }}
            foreach ($cases['argon2_php'] as $c) {{
                if (wp_check_password($pw($c['password']), $c['hash']) !== $c['verify_right']) {{ $fail[] = $c['name']; }}
            }}
            return $fail;"#
        ),
    );
    check(
        "recorded_checks_replay",
        matches!(&r, Ok(HookValue::Array(a)) if a.is_empty()),
        &r,
    );

    let r = eval(
        "override",
        "$c = \\SpeedySpec\\Container\\Container::getInstance();
         $c->bind(\\SpeedySpec\\WpPassword\\PasswordHasher::class, fn () => new class implements \\SpeedySpec\\WpPassword\\PasswordHasher { public function hash($password) { return \"X:$password\"; } });
         $c->bind(\\SpeedySpec\\WpPassword\\PasswordChecker::class, fn () => new class implements \\SpeedySpec\\WpPassword\\PasswordChecker { public function check($password, $hash, $user_id) { return \"C:$password:$hash:$user_id\"; } });
         $c->bind(\\SpeedySpec\\WpPassword\\RehashPolicy::class, fn () => new class implements \\SpeedySpec\\WpPassword\\RehashPolicy { public function needs_rehash($hash, $user_id) { return \"R:$hash\"; } });
         $c->bind(\\SpeedySpec\\WpPassword\\PasswordGenerator::class, fn () => new class implements \\SpeedySpec\\WpPassword\\PasswordGenerator { public function generate($length, $special, $extra) { return \"G:$length\"; } });
         $c->bind(\\SpeedySpec\\WpPassword\\RandomNumber::class, fn () => new class implements \\SpeedySpec\\WpPassword\\RandomNumber { public function rand($min, $max) { return 42; } });
         return [wp_hash_password('a'), wp_check_password('p', 'h', 7), wp_password_needs_rehash('h'), wp_generate_password(3), wp_rand(1, 2)];",
    );
    let want = |items: Vec<HookValue>| {
        HookValue::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| (speedyspec_hooks::Key::Int(i as i64), v))
                .collect(),
        )
    };
    check(
        "php_override_replaces_default",
        r.as_ref().ok()
            == Some(&want(vec![
                HookValue::Str("X:a".into()),
                HookValue::Str("C:p:h:7".into()),
                HookValue::Str("R:h".into()),
                HookValue::Str("G:3".into()),
                HookValue::Int(42),
            ])),
        &r,
    );
    let from_rust = container
        .make_port::<dyn PasswordHasher>(PASSWORD_HASHER)
        .map(|hasher| hasher.hash(b"b"));
    check(
        "rust_callers_get_the_php_override",
        matches!(&from_rust, Ok(Ok(HookValue::Str(s))) if s == "X:b"),
        from_rust
            .as_ref()
            .map(|r| r.as_ref().map(|_| ()))
            .map_err(|e| e.to_string()),
    );
    php.restart_request().expect("a new request");
    let r = eval(
        "override-ends",
        "return str_starts_with(wp_hash_password('a'), '$argon2id$');",
    );
    check("override_ends_with_the_request", is_true(&r), &r);

    let r = eval(
        "generator-random",
        "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpPassword\\RandomNumber::class, fn () => new class implements \\SpeedySpec\\WpPassword\\RandomNumber { public function rand($min, $max) { return 1; } });
         return wp_generate_password(4, false, false);",
    );
    check(
        "generator_draws_through_bound_random_number",
        matches!(&r, Ok(HookValue::Str(s)) if s == "bbbb"),
        &r,
    );
    php.restart_request().expect("a new request");

    let r = eval(
        "application",
        "$h = \\SpeedySpec\\WpPassword\\application_password_hash('secret');
         $ok = str_starts_with($h, '$generic$') && \\SpeedySpec\\WpPassword\\application_password_check('secret', $h)
             && !\\SpeedySpec\\WpPassword\\application_password_check('wrong', $h)
             && 'abcd efgh ij' === \\SpeedySpec\\WpPassword\\chunk_password('abcdefghij');
         \\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpPassword\\PasswordChecker::class, fn () => new class implements \\SpeedySpec\\WpPassword\\PasswordChecker { public function check($password, $hash, $user_id) { return 'yes'; } });
         return $ok && true === \\SpeedySpec\\WpPassword\\application_password_check('any', '$2y$10$whatever');",
    );
    check("application_check_uses_bound_checker", is_true(&r), &r);
    php.restart_request().expect("a new request");

    let r = eval(
        "throwing-override",
        "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpPassword\\PasswordHasher::class, fn () => new class implements \\SpeedySpec\\WpPassword\\PasswordHasher { public function hash($password) { throw new RuntimeException('boom'); } });
         try { wp_hash_password('a'); return 'no exception'; } catch (RuntimeException $e) { return $e->getMessage(); }",
    );
    check(
        "throwing_override_surfaces_as_php_exception",
        matches!(&r, Ok(HookValue::Str(s)) if s == "boom"),
        &r,
    );
    php.restart_request().expect("a new request");

    let r = eval(
        "throwing-filter",
        "add_filter('check_password', static function () { throw new LogicException('filtered'); });
         try { wp_check_password('a', str_repeat('x', 40)); return 'no exception'; } catch (LogicException $e) { return $e->getMessage(); } finally { remove_all_filters('check_password'); }",
    );
    check(
        "filter_exception_is_rethrown",
        matches!(&r, Ok(HookValue::Str(s)) if s == "filtered"),
        &r,
    );

    let r = eval(
        "wp-hasher",
        "$log = [];
         add_filter('wp_hash_password_algorithm', static function ($a) use (&$log) { $log[] = 'algorithm'; return $a; });
         add_filter('password_needs_rehash', static function ($n) use (&$log) { $log[] = 'needs'; return $n; });
         $GLOBALS['wp_hasher'] = new class { public function HashPassword($p) { return \"H:$p\"; } public function CheckPassword($p, $h) { return 'checked'; } };
         $r = [wp_hash_password(\" a \\n\"), wp_check_password('a', str_repeat('x', 60)), wp_check_password('a', 'short'), wp_password_needs_rehash('$P$x'), $log];
         unset($GLOBALS['wp_hasher']);
         remove_all_filters('wp_hash_password_algorithm'); remove_all_filters('password_needs_rehash');
         return $r === ['H:a', 'checked', false, false, []];",
    );
    check("wp_hasher_honoured", is_true(&r), &r);

    let r = eval(
        "filter-order",
        "$log = [];
         foreach (['wp_hash_password_algorithm', 'wp_hash_password_options', 'check_password', 'password_needs_rehash', 'random_password'] as $hook) {
             add_filter($hook, static function (...$args) use (&$log, $hook) { $log[] = [$hook, array_slice($args, 1)]; return $args[0]; }, 10, 5);
         }
         $h = wp_hash_password('pw');
         wp_check_password('pw', $h, 3);
         wp_password_needs_rehash($h, '4');
         wp_generate_password(2, false, true);
         foreach (['wp_hash_password_algorithm', 'wp_hash_password_options', 'check_password', 'password_needs_rehash', 'random_password'] as $hook) { remove_all_filters($hook); }
         return $log === [
             ['wp_hash_password_algorithm', []],
             ['wp_hash_password_options', ['argon2id']],
             ['check_password', ['pw', $h, 3]],
             ['wp_hash_password_algorithm', []],
             ['wp_hash_password_options', ['argon2id']],
             ['password_needs_rehash', [$h, '4']],
             ['random_password', [2, false, true]],
         ] ? true : $log;",
    );
    check("filters_fire_in_wordpress_order", is_true(&r), &r);

    let r = eval(
        "errors",
        "add_filter('wp_hash_password_algorithm', static fn () => PASSWORD_BCRYPT);
         add_filter('wp_hash_password_options', static fn () => ['cost' => 3]);
         try { wp_hash_password('a'); $r = 'no exception'; } catch (ValueError $e) { $r = $e->getMessage(); }
         remove_all_filters('wp_hash_password_options');
         $warnings = [];
         set_error_handler(static function ($no, $message) use (&$warnings) { $warnings[] = $message; return true; });
         add_filter('wp_hash_password_options', static fn () => ['salt' => 'x', 'cost' => 4]);
         $h = wp_hash_password('a');
         restore_error_handler();
         remove_all_filters('wp_hash_password_options'); remove_all_filters('wp_hash_password_algorithm');
         return [$r, $warnings, str_starts_with($h, '$wp$2y$04$')];",
    );
    check(
        "php_errors_match",
        r.as_ref().ok()
            == Some(&want(vec![
                HookValue::Str("Invalid bcrypt cost parameter specified: 3".into()),
                want(vec![HookValue::Str("password_hash(): The \"salt\" option has been ignored, since providing a custom salt is no longer supported".into())]),
                HookValue::Bool(true),
            ])),
        &r,
    );

    let r = eval(
        "rand",
        "$ok = 0 === wp_rand(0, 0) && 5 === wp_rand(5, 5);
         for ($i = 0; $i < 200; $i++) {
             $a = wp_rand('3', '7'); $b = wp_rand(10, 1); $c = wp_rand(); $d = wp_rand(-5, -1);
             $ok = $ok && $a >= 3 && $a <= 7 && $b >= 1 && $b <= 10 && $c >= 0 && $c <= 4294967295 && $d >= 1 && $d <= 5;
         }
         $p = wp_generate_password(64, true, true);
         return $ok && 64 === strlen($p) && is_int(wp_rand());",
    );
    check("rand_and_generate_follow_wordpress", is_true(&r), &r);

    let r = eval(
        "coercion",
        "try { wp_hash_password([]); return 'no exception'; } catch (TypeError $e) { return 'TypeError'; }",
    );
    check(
        "array_password_is_a_type_error",
        matches!(&r, Ok(HookValue::Str(s)) if s == "TypeError"),
        &r,
    );

    println!("wp-password php: all checks passed");
}
