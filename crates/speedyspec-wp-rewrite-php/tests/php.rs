//! The rewrite API from PHP, over memory options. PHP boots once per process (`harness = false`).

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_rewrite::memory::MemoryRewriteOptions;
use speedyspec_wp_rewrite_php::RewriteArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn eval(php: &Php, code: &str) -> Result<Value, PhpError> {
    php.eval("case", code)
}

fn yes(php: &Php, name: &str, code: &str) {
    let r = eval(php, code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

/// The WordPress functions and the `$wp` object the rewrite API calls (WordPress itself is not loaded).
const WORDPRESS: &str = r#"
function home_url() { return 'https://example.test'; }
function site_url() { return 'https://example.test'; }
function is_multisite() { return false; }
function is_main_site() { return true; }
function get_post_types($args = array()) { return array(); }
function is_post_type_hierarchical($type) { return false; }
class WP_Stub { public array $public_query_vars = array(); public function add_query_var($qv) { $this->public_query_vars[] = $qv; } }
$GLOBALS['wp'] = new WP_Stub();
$GLOBALS['wp_rewrite'] = new WP_Rewrite();
return true;
"#;

fn main() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let api = hooks.api();
    let options = Rc::new(MemoryRewriteOptions::new());
    options.set("permalink_structure", "/%postname%/");
    let rewrite = RewriteArea::new(container.clone(), api.clone(), options.clone());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container.clone())),
        Rc::new(hooks),
        Rc::new(rewrite),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let ready = eval(&php, WORDPRESS);
    check(
        "wordpress_stubs_load",
        matches!(ready, Ok(Value::Bool(true))),
        &ready,
    );

    yes(
        &php,
        "functions_and_constants_exist",
        r#"foreach (['add_rewrite_rule', 'add_rewrite_tag', 'remove_rewrite_tag', 'add_permastruct', 'remove_permastruct',
            'add_feed', 'flush_rewrite_rules', 'add_rewrite_endpoint', '_wp_filter_taxonomy_base'] as $f) {
            if (!function_exists($f)) { return $f; }
        }
        return EP_ALL === 8191 && EP_ALL_ARCHIVES === 3644 && EP_PAGES === 4096
            && class_exists('WP_Rewrite') && class_exists('WP_MatchesMapRegex');"#,
    );
    yes(
        &php,
        "init_reads_permalink_structure",
        r#"global $wp_rewrite; return $wp_rewrite->permalink_structure === '/%postname%/'
            && $wp_rewrite->front === '/' && $wp_rewrite->use_verbose_page_rules === true
            && $wp_rewrite->using_permalinks() === true && !isset($wp_rewrite->author_structure);"#,
    );
    yes(
        &php,
        "property_writes_are_seen",
        r#"global $wp_rewrite;
        $wp_rewrite->author_base = 'writer';
        $wp_rewrite->extra_rules_top['x/?$'] = 'index.php?x=1';
        $rules = $wp_rewrite->rewrite_rules();
        return array_key_first($rules) === 'x/?$' && isset($rules['writer/([^/]+)/?$'])
            && $wp_rewrite->get_author_permastruct() === '/writer/%author%';"#,
    );
    yes(
        &php,
        "add_rewrite_rule_reaches_extra_rules",
        r#"global $wp_rewrite;
        add_rewrite_rule('top/?$', 'index.php?top=1', 'top');
        add_rewrite_rule('arr/([^/]+)/?$', array('a' => '1', 'b' => '$matches[1]'));
        return $wp_rewrite->extra_rules_top['top/?$'] === 'index.php?top=1'
            && $wp_rewrite->extra_rules['arr/([^/]+)/?$'] === 'index.php?a=1&b=$matches[1]';"#,
    );
    yes(
        &php,
        "rewrite_tags_and_permastructs",
        r#"global $wp_rewrite, $wp;
        add_rewrite_tag('%book%', '([^/]+)');
        add_permastruct('book', 'book/%book%', array('ep_mask' => EP_PERMALINK, 'paged' => false));
        $ok = in_array('%book%', $wp_rewrite->rewritecode, true) && in_array('book', $wp->public_query_vars, true)
            && $wp_rewrite->extra_permastructs['book']['struct'] === '/book/%book%'
            && $wp_rewrite->extra_permastructs['book']['paged'] === false;
        remove_permastruct('book');
        remove_rewrite_tag('%book%');
        return $ok && !isset($wp_rewrite->extra_permastructs['book']) && !in_array('%book%', $wp_rewrite->rewritecode, true);"#,
    );
    yes(
        &php,
        "matches_map_regex_apply",
        r#"$m = new WP_MatchesMapRegex('p=$matches[2]', ['', 'x', 'y z']);
        return WP_MatchesMapRegex::apply('a=$matches[1]', ['', 'b c']) === 'a=b+c' && $m->output === 'p=y+z';"#,
    );
    yes(
        &php,
        "add_feed_registers_action",
        r#"global $wp_rewrite;
        function my_feed() {}
        return add_feed('custom', 'my_feed') === 'do_feed_custom' && has_action('do_feed_custom', 'my_feed') === 10
            && in_array('custom', $wp_rewrite->feeds, true);"#,
    );
    yes(
        &php,
        "add_rewrite_endpoint_adds_query_var",
        r#"global $wp_rewrite, $wp;
        add_rewrite_endpoint('json', EP_ALL);
        add_rewrite_endpoint('quiet', EP_PAGES, false);
        $last = end($wp_rewrite->endpoints);
        return in_array('json', $wp->public_query_vars, true) && !in_array('quiet', $wp->public_query_vars, true)
            && $last === array(EP_PAGES, 'quiet', false);"#,
    );
    yes(
        &php,
        "generate_rewrite_rules_action_gets_the_object",
        r#"global $wp_rewrite;
        add_action('generate_rewrite_rules', function ($r) { $r->rules = array('plugin$' => 'index.php?plugin=1') + $r->rules; });
        $rules = $wp_rewrite->rewrite_rules();
        return array_key_first($rules) === 'plugin$';"#,
    );
    yes(
        &php,
        "generate_rewrite_rules_method",
        r#"global $wp_rewrite;
        $wp_rewrite->matches = 'matches';
        $rules = $wp_rewrite->generate_rewrite_rules('/tag/%author%', EP_NONE, false, false);
        return $rules === array('tag/([^/]+)/?$' => 'index.php?author_name=$matches[1]');"#,
    );
    yes(
        &php,
        "filter_taxonomy_base",
        r#"return _wp_filter_taxonomy_base('/index.php/topics/') === 'topics' && _wp_filter_taxonomy_base('') === '';"#,
    );
    let flushed = eval(
        &php,
        "do_action('wp_loaded'); flush_rewrite_rules(false); return true;",
    );
    check(
        "flush_writes_option",
        flushed.is_ok() && options.stored_rules().is_some_and(|r| !r.is_empty()),
        &flushed,
    );
}
