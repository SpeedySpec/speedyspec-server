//! WordPress's sodium functions in embedded PHP: the facade over the native functions, replaying every case
//! `compat/crypto/record.php` recorded from WordPress 7.1.5, with WordPress's own `WP_Error` loaded.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each check in order.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_crypto_php::CryptoArea;
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

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
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    let compat = format!("{}/../../compat/crypto/", env!("CARGO_MANIFEST_DIR"));
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container, Rc::new(MemoryHookStore::new()));
    let crypto = CryptoArea::new(hooks.api()).expect("libsodium initializes");
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(crypto)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let r = php.eval(
        "speedyspec://test/no-wp-error",
        "return verify_file_signature('/nonexistent/x.zip', '0', 'x.zip');",
    );
    check(
        "no_wp_error_class_returns_false",
        matches!(r, Ok(HookValue::Bool(false))),
        &r,
    );

    // WordPress's WP_Error, whose add() fires wp_error_added through the native Hook API.
    let load = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); error_reporting(E_ALL & ~E_DEPRECATED);
         require ABSPATH . WPINC . '/class-wp-error.php';",
        format!("{}/", release.display())
    );
    php.eval("speedyspec://test/load", &load)
        .expect("WP_Error loads");

    for name in [
        "wp_fast_hash",
        "wp_verify_fast_hash",
        "wp_trusted_keys",
        "verify_file_signature",
    ] {
        let r = php.eval(
            "speedyspec://test/facade",
            &format!("return (new ReflectionFunction('{name}'))->getFileName();"),
        );
        check(
            &format!("{name}_is_the_facade"),
            matches!(&r, Ok(HookValue::Str(s)) if s == "speedyspec://wp-crypto"),
            &r,
        );
    }

    let r = php.eval(
        "speedyspec://test/signatures",
        "$f = static fn ($n) => array_map(static fn ($p) => [$p->getName(), $p->isOptional() ? var_export($p->getDefaultValue(), true) : null, (string) $p->getType()], (new ReflectionFunction($n))->getParameters());
         return $f('wp_fast_hash') === [['message', null, 'string']]
             && $f('wp_verify_fast_hash') === [['message', null, 'string'], ['hash', null, 'string']]
             && $f('wp_trusted_keys') === []
             && $f('verify_file_signature') === [['filename', null, ''], ['signatures', null, ''], ['filename_for_errors', 'false', '']]
             && (string) (new ReflectionFunction('wp_fast_hash'))->getReturnType() === 'string'
             && (string) (new ReflectionFunction('wp_verify_fast_hash'))->getReturnType() === 'bool';",
    );
    check("signatures_match_wordpress", is_true(&r), &r);

    let replay = format!(
        r#"$compat = {compat:?};
        $cases = json_decode(file_get_contents($compat . 'cases.json'), true);
        $fail = [];
        foreach ($cases['fast_hash'] as $c) {{
            $got = wp_fast_hash(base64_decode($c['message_b64']));
            if ($got !== $c['hash']) {{ $fail[] = "fast_hash: $got"; }}
        }}
        foreach ($cases['verify'] as $c) {{
            $got = wp_verify_fast_hash(base64_decode($c['message_b64']), $c['hash']);
            $want = 'phpass' === $c['name'] ? false : $c['result'];
            if ($got !== $want) {{ $fail[] = "verify/{{$c['name']}}"; }}
        }}
        foreach ($cases['signature'] as $c) {{
            remove_all_filters('wp_trusted_keys');
            $keys = $c['filter'];
            add_filter('wp_trusted_keys', static fn () => $keys);
            $got = @verify_file_signature($compat . 'files/' . $c['file'], $c['signatures'], $c['filename_for_errors']);
            $want = $c['result'];
            if (is_array($want)) {{
                if (is_array($want['data']) && isset($want['data']['hash'])) {{
                    $want['data']['php'] = PHP_VERSION;
                    $want['data']['sodium'] = SODIUM_LIBRARY_VERSION;
                }}
                $ok = $got instanceof WP_Error && $got->get_error_code() === $want['code']
                    && $got->get_error_message() === $want['message'] && $got->get_error_data() === $want['data'];
            }} else {{
                $ok = $got === $want;
            }}
            if (!$ok) {{ $fail[] = "signature/{{$c['name']}}: " . var_export($got, true); }}
        }}
        remove_all_filters('wp_trusted_keys');
        if (wp_trusted_keys() !== $cases['trusted_keys']) {{ $fail[] = 'trusted_keys'; }}
        return implode("\n", $fail);"#
    );
    let r = php.eval("speedyspec://test/replay", &replay);
    check(
        "replays_wordpress",
        matches!(&r, Ok(HookValue::Str(s)) if s.is_empty()),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/did-filter",
        "remove_all_filters('wp_trusted_keys'); $before = did_filter('wp_trusted_keys'); wp_trusted_keys();
         return did_filter('wp_trusted_keys') === $before + 1;",
    );
    check("no_filter_still_counts", is_true(&r), &r);

    let r = php.eval(
        "speedyspec://test/error-added",
        "$seen = null; add_action('wp_error_added', static function ($code) use (&$seen) { $seen = $code; });
         verify_file_signature('/nonexistent/x.zip', '', 'x.zip');
         return $seen === 'signature_verification_no_signature';",
    );
    check("wp_error_added_fires", is_true(&r), &r);

    let r = php.eval(
        "speedyspec://test/type-error",
        "try { wp_fast_hash([]); return 'no error'; } catch (TypeError $e) { return $e->getMessage(); }",
    );
    check(
        "fast_hash_type_error",
        matches!(&r, Ok(HookValue::Str(s)) if s.starts_with("wp_fast_hash(): Argument #1 ($message) must be of type string")),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/filter-object",
        "remove_all_filters('wp_trusted_keys'); add_filter('wp_trusted_keys', static fn () => [['nested']]);
         try { verify_file_signature('/nonexistent/x.zip', base64_encode(str_repeat(chr(0), 64)), 'x'); return 'no error'; }
         catch (TypeError $e) { return $e->getMessage(); } finally { remove_all_filters('wp_trusted_keys'); }",
    );
    check(
        "array_key_is_base64_decodes_type_error",
        matches!(&r, Ok(HookValue::Str(s)) if s == "base64_decode(): Argument #1 ($string) must be of type string, array given"),
        &r,
    );
    // The recorded valid signature and key for files/hello.txt.
    let fixture = format!(
        r#"$compat = {compat:?};
        $cases = json_decode(file_get_contents($compat . 'cases.json'), true);
        $valid = array_values(array_filter($cases['signature'], static fn ($c) => 'valid' === $c['name']))[0];
        $GLOBALS['speedyspec_sig'] = $valid['signatures'];
        $GLOBALS['speedyspec_key'] = $valid['filter'][0];
        $GLOBALS['speedyspec_file'] = $compat . 'files/hello.txt';
        return true;"#
    );
    php.eval("speedyspec://test/fixture", &fixture)
        .expect("the fixture loads");

    let r = php.eval(
        "speedyspec://test/trusted-keys-untouched",
        "remove_all_filters('wp_trusted_keys'); $value = [['nested'], null, 5, new stdClass()];
         add_filter('wp_trusted_keys', static fn () => $value);
         $got = wp_trusted_keys(); remove_all_filters('wp_trusted_keys');
         return $got == $value && count($got) === 4 && $got[3] instanceof stdClass;",
    );
    check(
        "trusted_keys_returns_the_filtered_value_untouched",
        is_true(&r),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/key-after-match",
        "remove_all_filters('wp_trusted_keys'); add_filter('wp_trusted_keys', static fn () => [$GLOBALS['speedyspec_key'], ['x']]);
         $got = verify_file_signature($GLOBALS['speedyspec_file'], $GLOBALS['speedyspec_sig'], 'f'); remove_all_filters('wp_trusted_keys');
         return $got;",
    );
    check("a_key_after_the_match_is_never_read", is_true(&r), &r);

    let r = php.eval(
        "speedyspec://test/key-short-signature",
        "remove_all_filters('wp_trusted_keys'); add_filter('wp_trusted_keys', static fn () => [['x']]);
         $got = verify_file_signature($GLOBALS['speedyspec_file'], 'QUJD', 'f'); remove_all_filters('wp_trusted_keys');
         return $got instanceof WP_Error ? $got->get_error_code() . ':' . $got->get_error_data()['skipped_sig'] : var_export($got, true);",
    );
    check(
        "a_key_behind_short_signatures_is_never_read",
        matches!(&r, Ok(HookValue::Str(s)) if s == "signature_verification_failed:1"),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/signature-after-match",
        "remove_all_filters('wp_trusted_keys'); add_filter('wp_trusted_keys', static fn () => [$GLOBALS['speedyspec_key']]);
         $got = verify_file_signature($GLOBALS['speedyspec_file'], [$GLOBALS['speedyspec_sig'], ['x']], 'f'); remove_all_filters('wp_trusted_keys');
         return $got;",
    );
    check("a_signature_after_the_match_is_never_read", is_true(&r), &r);

    let r = php.eval(
        "speedyspec://test/message-hooks",
        "// WordPress's __() fires gettext; l10n.php isn't loaded here, so this stands in for it.
         if (!function_exists('__')) { function __($text, $domain = 'default') { return apply_filters('gettext', $text, $text, $domain); } }
         $log = [];
         add_filter('gettext', static function ($t) use (&$log) { $log[] = 'gettext'; return $t; });
         add_filter('esc_html', static function ($t) use (&$log) { $log[] = 'esc_html'; return $t; });
         add_filter('wp_trusted_keys', static function ($k) use (&$log) { $log[] = 'keys'; return [$GLOBALS['speedyspec_key']]; });
         $runs = [];
         verify_file_signature($GLOBALS['speedyspec_file'], $GLOBALS['speedyspec_sig'], 'f'); $runs[] = implode(',', $log); $log = [];
         verify_file_signature($GLOBALS['speedyspec_file'], '', 'f'); $runs[] = implode(',', $log); $log = [];
         verify_file_signature($GLOBALS['speedyspec_file'], 'QUJD', 'f'); $runs[] = implode(',', $log);
         remove_all_filters('gettext'); remove_all_filters('esc_html'); remove_all_filters('wp_trusted_keys');
         return implode('|', $runs);",
    );
    check(
        "message_hooks_fire_only_with_their_error_and_in_wordpress_order",
        matches!(&r, Ok(HookValue::Str(s)) if s == "keys|gettext,esc_html|keys,gettext,esc_html"),
        &r,
    );
    println!("wp-crypto php: all checks passed");
}
