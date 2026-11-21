//! The native kses functions from PHP, beside the release's own formatting.php and functions.php. PHP boots once per
//! process (`harness = false`); each group of cases runs in a fresh request.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::{KsesArea, prepare_release};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

fn yes(name: &str, code: &str) {
    let r = eval(code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

/// Starts a request with the release's files loaded; `before` runs first (a constant kses.php reads, say).
fn request(php: &Php, root: &std::path::Path, before: &str) {
    php.restart_request().expect("a new request");
    let code = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); {before}
        function current_user_can($capability, ...$args) {{ return !empty($GLOBALS['test_can']); }}
        function wp_installing($is = null) {{ return false; }}
        add_filter('pre_wp_load_alloptions', function () {{ return ['blog_charset' => 'UTF-8']; }});
        require ABSPATH . WPINC . '/compat.php';
        require ABSPATH . WPINC . '/utf8.php';
        require ABSPATH . WPINC . '/formatting.php';
        require ABSPATH . WPINC . '/functions.php';
        require ABSPATH . WPINC . '/kses.php';
        return true;",
        format!("{}/", root.display())
    );
    let loaded = eval(&code);
    check(
        "request loads the release",
        matches!(loaded, Ok(Value::Bool(true))),
        loaded,
    );
}

fn main() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();

    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let api = kses.api();
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container.clone())),
        Rc::new(hooks),
        Rc::new(kses),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    request(&php, &root, "");
    yes(
        "functions_are_native",
        r#"foreach (['wp_kses', 'wp_kses_post', 'wp_allowed_protocols', 'safecss_filter_attr'] as $f) {
            if ((new ReflectionFunction($f))->getFileName() !== 'speedyspec://wp-kses') { return $f; }
        }
        return true;"#,
    );
    for (i, post) in speedyspec_wp_kses_testkit::posts().iter().enumerate() {
        speedyspec_php::set_global("test_post", &Value::Bytes(post.clone())).unwrap();
        let from_php = eval("return wp_kses_post($GLOBALS['test_post']);");
        let ours = api.post(post).unwrap();
        let same = match &from_php {
            Ok(Value::Str(s)) => s.as_bytes() == ours,
            Ok(Value::Bytes(b)) => *b == ours,
            _ => false,
        };
        check(
            &format!("wp_kses_post_from_php_equals_rust[{i}]"),
            same,
            &from_php,
        );
    }
    yes(
        "global_write_takes_effect",
        r#"$before = wp_kses_post('<x-tag>a</x-tag>');
        $GLOBALS['allowedposttags']['x-tag'] = array();
        $after = wp_kses_post('<x-tag>a</x-tag>');
        $GLOBALS['allowedposttags']['x-tag']['data-n'] = true;
        $nested = wp_kses_post('<x-tag data-n="1">a</x-tag>');
        return $before === 'a' && $after === '<x-tag>a</x-tag>' && $nested === '<x-tag data-n="1">a</x-tag>';"#,
    );
    yes(
        "value_callback_calls_php",
        r#"function test_ok($v) { return 'ok' === $v; }
        $GLOBALS['allowedposttags']['span']['data-v'] = array('value_callback' => 'test_ok');
        return wp_kses_post('<span data-v="ok">a</span><span data-v="no">b</span>')
            === '<span data-v="ok">a</span><span>b</span>';"#,
    );
    yes(
        "post_deep_maps_arrays",
        r#"$o = new stdClass; $o->p = '<b onclick="x">c</b>';
        $r = wp_kses_post_deep(array('a' => '<script>x</script>b', 'n' => array('<i>d</i>'), 'o' => $o));
        return $r['a'] === 'xb' && $r['n'] === array('<i>d</i>') && $r['o']->p === '<b>c</b>';"#,
    );
    yes(
        "allowed_html_returns_the_php_array",
        r#"$f = function ($tags, $context) { $tags['x-y'] = array('data-z' => array('value_callback' => 'strlen')); return $tags; };
        add_filter('wp_kses_allowed_html', $f, 10, 2);
        $data = wp_kses_allowed_html('data');
        remove_filter('wp_kses_allowed_html', $f, 10);
        return $data['x-y']['data-z']['value_callback'] === 'strlen' && isset($data['a']['href'])
            && wp_kses_allowed_html('strip') === array();"#,
    );
    yes(
        "attr_check_writes_its_references",
        r#"$n = 'onclick'; $v = 'x'; $w = 'onclick="x"';
        $denied = wp_kses_attr_check($n, $v, $w, 'n', 'a', wp_kses_allowed_html('post'));
        $n2 = 'href'; $v2 = 'http://a/'; $w2 = 'href="http://a/"';
        $kept = wp_kses_attr_check($n2, $v2, $w2, 'n', 'a', wp_kses_allowed_html('post'));
        return false === $denied && '' === $n && '' === $v && '' === $w
            && true === $kept && 'href' === $n2 && 'http://a/' === $v2;"#,
    );
    yes(
        "exception_in_a_filter_is_rethrown",
        r#"$f = function () { throw new RuntimeException('boom'); };
        add_filter('pre_kses', $f);
        try { wp_kses_post('x'); $r = 'no exception'; } catch (RuntimeException $e) { $r = $e->getMessage(); }
        remove_filter('pre_kses', $f);
        return 'boom' === $r;"#,
    );
    yes(
        "lexical_functions_match_wordpress",
        r#"return wp_kses_no_null("a\x01b\\\\0c") === 'abc'
            && wp_kses_no_null("a\\\\0", array('slash_zero' => 'keep')) === "a\\\\0"
            && wp_kses_normalize_entities('&amp; &bogus; &#65;') === '&amp; &amp;bogus; &#065;'
            && wp_kses_named_entities(array('&nbsp;', 'nbsp')) === '&nbsp;'
            && wp_kses_named_entities(array('&zz;', 'zz')) === '&amp;zz;'
            && wp_kses_xml_named_entities(array('&nbsp;', 'nbsp')) === "\u{a0}"
            && wp_kses_normalize_entities2(array('&#0065;', '0065')) === '&#065;'
            && wp_kses_normalize_entities3(array('&#x0041;', '0041')) === '&#x41;'
            && _wp_kses_decode_entities_chr(array('&#65;', '65')) === 'A'
            && _wp_kses_decode_entities_chr_hexdec(array('&#x41;', '41')) === 'A'
            && valid_unicode(65) && ! valid_unicode(0xFFFE)
            && wp_kses_decode_entities('&#65;&#x42;') === 'AB'
            && wp_kses_bad_protocol('javascript:x', array('http')) === 'x'
            && wp_kses_hair_parse('a="b" c') === array('a="b" ', 'c')
            && wp_kses_attr_parse('<a b>') === array('<a ', 'b', '>')
            && wp_kses_array_lc(array('A' => array('HREF' => true))) === array('a' => array('href' => true))
            && wp_kses_check_attr_val('abc', 'n', 'maxlen', 2) === false
            && wp_kses_hair('1 b="c"', array('http'))[1]['name'] === 1
            && wp_kses_version() === '0.2.2'
            && in_array('https', wp_allowed_protocols(), true)
            && in_array('href', wp_kses_uri_attributes(), true)
            && safecss_filter_attr('color: red; behavior: x') === 'color: red';"#,
    );
    yes(
        "init_filters_register_by_name",
        r#"kses_init_filters();
        $ok = has_filter('title_save_pre', 'wp_filter_kses') === 10 && has_filter('content_save_pre', 'wp_filter_post_kses') === 10
            && has_filter('content_save_pre', 'wp_filter_global_styles_post') === 9
            && apply_filters('title_save_pre', '<b onclick=\"x\">t</b>') === '<b>t</b>';
        kses_remove_filters();
        return $ok && false === has_filter('title_save_pre', 'wp_filter_kses');"#,
    );

    request(&php, &root, "");
    yes(
        "write_through_a_reference_takes_effect",
        r#"$r = &$GLOBALS['allowedposttags']['a'];
        $before = wp_kses_post('<a title="t">a</a>');
        unset($r['title']);
        $after = wp_kses_post('<a title="t">a</a>');
        $r['data-z'] = true;
        $added = wp_kses_post('<a data-z="1">a</a>');
        unset($r);
        return $before === '<a title="t">a</a>' && $after === '<a>a</a>' && $added === '<a data-z="1">a</a>' ? true : [$before, $after, $added];"#,
    );

    request(&php, &root, "");
    yes(
        "a_destructor_running_kses_while_the_old_allowlist_is_freed",
        r#"final class SpeedySpecKsesDestructs { public function __destruct() { $GLOBALS['speedyspec_seen'] = wp_kses_post('<i>d</i>'); } }
        $o = new SpeedySpecKsesDestructs();
        $GLOBALS['allowedposttags']['span']['data-d'] = array('value_callback' => function ($v) use ($o) { return true; });
        unset($o);
        $first = wp_kses_post('<span data-d="1">a</span>');
        $GLOBALS['allowedposttags'] = array('b' => array(), 'i' => array());
        $second = wp_kses_post('<b>y</b><span>s</span>');
        $third = wp_kses_post('<i>z</i>');
        return $first === '<span data-d="1">a</span>' && $second === '<b>y</b>s' && $third === '<i>z</i>'
            && ($GLOBALS['speedyspec_seen'] ?? null) === '<i>d</i>';"#,
    );

    request(
        &php,
        &root,
        "define('CUSTOM_TAGS', true); $GLOBALS['allowedposttags'] = array('b' => array(), 'input' => array());
        $GLOBALS['allowedtags'] = array('i' => array()); $GLOBALS['allowedentitynames'] = array('amp');
        $GLOBALS['allowedxmlentitynames'] = array('amp');",
    );
    yes(
        "custom_tags_defined_uses_user_globals",
        r#"return wp_kses_post('<b>x</b><i>y</i><form>z</form>&nbsp;') === '<b>x</b>yz&amp;nbsp;'
            && wp_kses_data('<b>x</b><i>y</i>') === 'x<i>y</i>'
            && ! isset(wp_kses_allowed_html('post')['form']);"#,
    );
    println!("all kses PHP cases passed");
}
