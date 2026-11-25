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
function get_post_types($args = array(), $output = 'names') {
    if (empty($args['randomable'])) { return array(); }
    $book = (object) array('name' => 'book', 'has_archive' => 'books', 'rewrite' => array('slug' => 'book'));
    return array('book' => 'objects' === $output ? $book : 'book');
}
function is_post_type_hierarchical($type) { return false; }
function get_page_by_path($path) {
    return 'about' === $path ? (object) array('ID' => 2, 'post_status' => 'publish') : null;
}
function get_taxonomies($args = array(), $output = 'names') {
    return array('genre' => (object) array('name' => 'genre', 'rewrite' => array('slug' => 'genre'), 'object_type' => array('book')));
}
define('OBJECT', 'OBJECT');
function wp_is_random_content_redirect_enabled() { return (bool) apply_filters('wp_enable_random_content_redirect', true); }
function get_locale() { return 'de_DE'; }
function switch_to_locale($locale) { $GLOBALS['switched'][] = $locale; return true; }
function restore_previous_locale() { $GLOBALS['switched'][] = 'restored'; return 'en_US'; }
function _x($text, $context) { return 'random' === $text ? 'zufall' : $text; }
function get_post_status_object($status) {
    return (object) array('public' => true, 'protected' => false, 'private' => false, 'exclude_from_search' => false);
}
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
            'add_feed', 'flush_rewrite_rules', 'add_rewrite_endpoint', '_wp_filter_taxonomy_base',
            'wp_flush_rewrite_rules_on_site_language_change'] as $f) {
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
        "random_base_is_translated_in_the_site_language",
        r#"global $wp_rewrite;
        $GLOBALS['switched'] = array();
        return $wp_rewrite->random_base === '' && $wp_rewrite->get_random_base() === 'zufall'
            && $GLOBALS['switched'] === array('de_DE', 'restored')
            && $wp_rewrite->post_permalinks_share_random_base() === true;"#,
    );
    yes(
        &php,
        "random_rewrite_rules_cover_authors_taxonomies_and_post_types",
        r#"global $wp_rewrite;
        add_rewrite_tag('%genre%', '([^/]+)', 'genre=');
        add_permastruct('genre', 'genre/%genre%');
        $rules = $wp_rewrite->random_rewrite_rules();
        $all = $wp_rewrite->rewrite_rules();
        remove_permastruct('genre');
        remove_rewrite_tag('%genre%');
        return $rules === array(
                'zufall/?$' => 'index.php?random=1',
                'zufall/writer/([^/]+)/?$' => 'index.php?author_name=$1&random=1',
                'zufall/genre/([^/]+)/?$' => 'index.php?genre=$1&random=1',
                'zufall/books/?$' => 'index.php?post_type=book&random=1',
            )
            && array_search('zufall/?$', array_keys($all), true) < array_search('feed/(feed|rdf|rss|rss2|atom)/?$', array_keys($all), true);"#,
    );
    yes(
        &php,
        "set_random_base_stores_and_reinitialises",
        r#"global $wp_rewrite;
        $wp_rewrite->set_random_base('surprise');
        $ok = $wp_rewrite->random_base === 'surprise' && $wp_rewrite->get_random_base() === 'surprise'
            && isset($wp_rewrite->random_rewrite_rules()['surprise/?$']);
        $wp_rewrite->set_random_base('');
        return $ok && $wp_rewrite->random_base === '';"#,
    );
    yes(
        &php,
        "random_content_rules_follow_their_filters",
        r#"global $wp_rewrite;
        $no = static function () { return false; };
        add_filter('wp_enable_random_content_redirect', $no);
        $off = $wp_rewrite->random_rewrite_rules();
        remove_filter('wp_enable_random_content_redirect', $no);
        $drop = static function ($rules) { unset($rules['zufall/?$']); return $rules; };
        add_filter('random_rewrite_rules', $drop);
        $all = $wp_rewrite->rewrite_rules();
        remove_filter('random_rewrite_rules', $drop);
        return $off === array() && !isset($all['zufall/?$']) && isset($all['zufall/writer/([^/]+)/?$']);"#,
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
    yes(
        &php,
        "plain_permalinks_do_not_404",
        r#"return speedyspec_rewrite_match(array(), 'anything', 'anything') === null;"#,
    );
    yes(
        &php,
        "match_sets_matched_rule_and_query",
        r#"$rules = array(
            '(.?.+?)(?:/([0-9]+))?/?$' => 'index.php?pagename=$matches[1]&page=$matches[2]',
            '([^/]+)(?:/([0-9]+))?/?$' => 'index.php?name=$matches[1]&page=$matches[2]',
        );
        $m = speedyspec_rewrite_match($rules, 'hello-world', 'hello-world');
        return $m['matched_rule'] === '([^/]+)(?:/([0-9]+))?/?$' && $m['matched_query'] === 'name=hello-world&page='
            && $m['perma_query_vars'] === array('name' => 'hello-world', 'page' => '');"#,
    );
    yes(
        &php,
        "verbose_page_guard_in_php",
        r#"$rules = array(
            '(.?.+?)(?:/([0-9]+))?/?$' => 'index.php?pagename=$matches[1]&page=$matches[2]',
            '([^/]+)(?:/([0-9]+))?/?$' => 'index.php?name=$matches[1]&page=$matches[2]',
        );
        $m = speedyspec_rewrite_match($rules, 'about', 'about');
        return $m['matched_query'] === 'pagename=about&page=';"#,
    );
    yes(
        &php,
        "no_match_is_null_fields",
        r#"$m = speedyspec_rewrite_match(array('only$' => 'index.php?only=1'), 'else', 'else');
        return $m === array('matched_rule' => null, 'matched_query' => null, 'perma_query_vars' => null);"#,
    );
    yes(
        &php,
        "empty_path_matches_dollar",
        r#"$m = speedyspec_rewrite_match(array('(.*)' => 'index.php?all=1', '$' => 'index.php?home=1'), '', '');
        return $m['matched_rule'] === '$' && $m['perma_query_vars'] === array('home' => '1');"#,
    );
    yes(
        &php,
        "needs_php_rule_still_matches",
        r#"$rules = array('x$' => 'index.php?x=1', '(?|(a)|(b))/?$' => 'index.php?p=$matches[1]', '.*' => 'index.php?any=1');
        $m = speedyspec_rewrite_match($rules, 'b', 'b');
        return $m['matched_rule'] === '(?|(a)|(b))/?$' && $m['matched_query'] === 'p=b';"#,
    );
    yes(
        &php,
        "non_utf8_rule_keys_fall_back_to_php",
        r#"$rules = array("caf\xE9/?$" => 'index.php?latin=1', '([^/]+)/?$' => 'index.php?name=$matches[1]');
        $threw = false;
        try { speedyspec_rewrite_match($rules, 'caf%E9', 'caf%E9'); } catch (\Throwable $e) { $threw = true; }
        $m = speedyspec_rewrite_match_php($rules, 'caf%E9', 'caf%E9');
        return $threw && $m['matched_rule'] === "caf\xE9/?$" && $m['matched_query'] === 'latin=1';"#,
    );
    yes(
        &php,
        "read_only_calls_write_nothing_back",
        r#"class CountingRewrite extends WP_Rewrite {
            public $assigns = 0;
            public function __speedyspec_assign($vars, $unset) { $this->assigns++; parent::__speedyspec_assign($vars, $unset); }
        }
        $r = new CountingRewrite();
        $r->assigns = 0;
        $r->get_author_permastruct();
        $r->get_author_permastruct();
        $r->get_extra_permastruct('category');
        $r->using_permalinks();
        $r->using_index_permalinks();
        $r->using_mod_rewrite_permalinks();
        $r->preg_index(2);
        return $r->assigns === 1;"#,
    );
    yes(
        &php,
        "methods_do_not_read_the_rules",
        r#"$r = new WP_Rewrite();
        $r->rules = array("caf\xE9/?$" => 'index.php?x=1');
        return $r->get_feed_permastruct() === 'feed/%feed%' && $r->rules === array("caf\xE9/?$" => 'index.php?x=1');"#,
    );
    // WordPress keeps the caller's values as given (after array_intersect_key and wp_parse_args), and later writes
    // of the state keep them.
    yes(
        &php,
        "permastruct_and_endpoint_values_keep_their_php_types",
        r#"$r = new WP_Rewrite();
        $r->init();
        $r->add_permastruct('typed', 'typed/%typed%', array('with_front' => 'yes', 'paged' => 0, 'bogus' => 1));
        $r->add_endpoint('blank', '3', '');
        $permastruct = array(
            'with_front' => 'yes', 'ep_mask' => EP_NONE, 'paged' => 0, 'feed' => true,
            'forcomments' => false, 'walk_dirs' => true, 'endpoints' => true, 'struct' => $r->front . 'typed/%typed%',
        );
        $endpoint = array('3', 'blank', '');
        $added = $r->extra_permastructs['typed'] === $permastruct && end($r->endpoints) === $endpoint;
        $r->add_permastruct('other', 'other/%other%');
        $r->add_endpoint('more', EP_ALL);
        return $added && $r->extra_permastructs['typed'] === $permastruct && $r->endpoints[0] === $endpoint
            && $r->extra_permastructs['other']['with_front'] === true;"#,
    );
    // A huge integer key must not make the port allocate up to it.
    yes(
        &php,
        "matches_map_takes_huge_keys",
        r#"return WP_MatchesMapRegex::apply('a=$matches[1]&b=$matches[9223372036854775807]', [1 => 'x', PHP_INT_MAX => 'y'])
            === 'a=x&b=y';"#,
    );
    // A PHP callback's exception reaches the caller unchanged, as it does in WordPress.
    yes(
        &php,
        "callback_exception_is_rethrown_unchanged",
        r#"$thrown = new DomainException('boom');
        $throw = function () use ($thrown) { throw $thrown; };
        add_filter('rewrite_rules_array', $throw);
        $r = new WP_Rewrite();
        $r->init();
        try {
            $r->rewrite_rules();
            $caught = null;
        } catch (Throwable $e) {
            $caught = $e;
        }
        remove_filter('rewrite_rules_array', $throw);
        return $caught === $thrown;"#,
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
