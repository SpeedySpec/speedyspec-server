<?php
/**
 * Records WordPress's post type and post status rules into the fixtures the Rust posts port is tested against.
 *
 * Usage: php record.php <wordpress-root> <out-dir>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`) against the WordPress 7.1.5 root
 * that `speedyspec_compat::wordpress_release("7.1.5")` unpacks. It records these sections into `<out-dir>/cases.json`:
 *
 * - `labels`: `get_post_type_labels()` over a plain object built from `object` (the input, recorded before the
 *   call, since the function rewrites `labels` in place).
 * - `caps`: `get_post_type_capabilities()` over a plain object with `capability_type`, `map_meta_cap` and
 *   `capabilities`; `meta_caps` is `$post_type_meta_caps` afterwards, starting empty for each case.
 * - `set_props`: `get_object_vars( new WP_Post_Type( $post_type, $args ) )`, under the permalink structure named by
 *   `permalinks`.
 * - `post_types`, `statuses`, `lists`: named scenarios. State is reset at the start of each scenario and carries
 *   over from call to call within one. Each call records its arguments, its result, the `_doing_it_wrong()`
 *   messages it raised (captured through the `doing_it_wrong_run` action), the actions it added and removed (a diff
 *   of `$wp_filter` before and after) and the `register_taxonomy_for_object_type()` calls it made. Each scenario
 *   ends with `$wp_post_types`, `$wp_post_statuses`, `$_wp_post_type_features` and `$post_type_meta_caps`.
 * - `status_labels`: `get_post_statuses()`, `get_page_statuses()` and `_wp_privacy_statuses()`.
 * - `initial`: `create_initial_post_types()` from a clean state, once without permalinks or attachment pages and
 *   once with both, with the globals, actions and taxonomies it left and the results of `queries` run after it.
 *
 * Value encoding. Every PHP value is written as `{"t": <type>, "v": <value>}`, so key order and bytes survive JSON:
 *
 * - `{"t":"null"}` (no `v`), `{"t":"bool","v":true}`, `{"t":"int","v":10}`, `{"t":"float","v":1.5}`;
 * - `{"t":"str","v":"<lowercase hex of the bytes>"}`;
 * - `{"t":"array","v":[[key, value], ...]}` in PHP's order, where a key is `{"t":"int","v":N}` or
 *   `{"t":"str","v":"<hex>"}` and a value is encoded the same way, recursively;
 * - `{"t":"object","class":"<class>","v":[[key, value], ...]}`, the pairs in `get_object_vars()` order, keys and
 *   values encoded as an array's are;
 * - `{"t":"wp_error","code":"<code>","message":"<message>"}`, the first code and its first message in plain text.
 *
 * Plain JSON fields: scenario and case names, `permalinks` (the permalink structure), `attachment_pages` (the
 * option's value). In `actions` and `actions_removed` each entry is `[hook, callback, priority, accepted_args]`:
 * hook and callback are encoded values (callbacks are named functions, so strings), priority and accepted_args plain
 * ints. `taxonomies` entries are `[taxonomy, object_type]` in plain text. `doing_it_wrong` entries are `{function,
 * message, version}` in plain text, the message being the one `__()` returns.
 *
 * Loaded from WordPress: version.php, compat-utf8.php and utf8.php (wp_is_valid_utf8), plugin.php, load.php
 * (is_admin, wp_installing), class-wp-error.php, functions.php (wp_parse_args, _doing_it_wrong,
 * wp_filter_object_list; it requires option.php), formatting.php (sanitize_key, sanitize_title_with_dashes),
 * class-wp-list-util.php, class-wp-post-type.php, post.php.
 * `is_admin()` is WordPress's own and false here.
 *
 * Stubbed, because the loaded files call them and they live in files not loaded here:
 * - `__()` and `_x()` (l10n.php) return their text; `_n_noop()` is copied from l10n.php;
 * - `EP_PERMALINK` (rewrite.php) is 1;
 * - `add_rewrite_tag()`, `add_rewrite_rule()`, `add_permastruct()`, `remove_rewrite_tag()` and
 *   `remove_permastruct()` (rewrite.php) do nothing;
 * - `register_taxonomy_for_object_type()` (taxonomy.php) appends `[taxonomy, object_type]` to the call's
 *   `taxonomies` and returns true; `get_object_taxonomies()` returns an empty array;
 * - `$wp` is an object whose `add_query_var()` and `remove_query_var()` do nothing;
 * - `$wp_rewrite` is an object with `front` `/`, `root` empty, the five default `feeds`, `pagination_base` `page`
 *   and no `extra_rules_top`.
 * Options come through `pre_option_permalink_structure` (`''` or `'/%postname%/'`) and
 * `pre_option_wp_attachment_pages_enabled` (`'0'` or `'1'`), hooked again after each reset and left out of the
 * action diff, as `doing_it_wrong_run` is. `_doing_it_wrong()` is WordPress's own, with WP_DEBUG off.
 */

declare(strict_types=1);

[$_, $root, $out] = $argv + [null, null, null];
if (!$root || !$out) {
    fwrite(STDERR, "usage: php record.php <wordpress-root> <out-dir>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
define('WP_DEBUG', false);
define('EP_PERMALINK', 1);
error_reporting(E_ALL);

/** Warnings PHP raised; the run fails if any are left. */
$warnings = [];
set_error_handler(static function (int $no, string $message) use (&$warnings, $root): bool {
    $warnings[] = str_replace($root, '<files>/', $message);
    return true;
});

// Stubs for what the loaded files call outside them.
function __($text, $domain = 'default')
{
    return $text;
}

function _x($text, $context, $domain = 'default')
{
    return $text;
}

function _n_noop($singular, $plural, $domain = null)
{
    return array(
        0          => $singular,
        1          => $plural,
        'singular' => $singular,
        'plural'   => $plural,
        'context'  => null,
        'domain'   => $domain,
    );
}

function add_rewrite_tag($tag, $regex, $query = '')
{
}

function add_rewrite_rule($regex, $query, $after = 'bottom')
{
}

function add_permastruct($name, $struct, $args = array())
{
}

function remove_rewrite_tag($tag)
{
}

function remove_permastruct($name)
{
}

function register_taxonomy_for_object_type($taxonomy, $object_type)
{
    $GLOBALS['recorder_taxonomies'][] = [$taxonomy, $object_type];
    return true;
}

function get_object_taxonomies($object_type, $output = 'names')
{
    return [];
}

/** The query-variable registry `WP_Post_Type` talks to. */
final class SpeedySpec_Recorder_WP
{
    public function add_query_var($qv)
    {
    }

    public function remove_query_var($name)
    {
    }
}

$wp = new SpeedySpec_Recorder_WP();
$wp_rewrite = (object) [
    'front'           => '/',
    'root'            => '',
    'feeds'           => ['feed', 'rdf', 'rss', 'rss2', 'atom'],
    'pagination_base' => 'page',
    'extra_rules_top' => [],
];

require ABSPATH . WPINC . '/version.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/plugin.php';
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/class-wp-list-util.php';
require ABSPATH . WPINC . '/class-wp-post-type.php';
require ABSPATH . WPINC . '/post.php';

/** A named meta box callback. */
function speedyspec_meta_boxes($post)
{
}

/** Encodes a PHP value as `{"t", "v"}`. */
function enc($value): array
{
    $pairs = static fn (array $items) => array_map(
        static fn ($k, $v) => [enc($k), enc($v)],
        array_keys($items),
        array_values($items)
    );
    return match (true) {
        null === $value     => ['t' => 'null'],
        is_bool($value)     => ['t' => 'bool', 'v' => $value],
        is_int($value)      => ['t' => 'int', 'v' => $value],
        is_float($value)    => ['t' => 'float', 'v' => $value],
        is_string($value)   => ['t' => 'str', 'v' => bin2hex($value)],
        is_array($value)    => ['t' => 'array', 'v' => $pairs($value)],
        $value instanceof WP_Error => [
            't'       => 'wp_error',
            'code'    => (string) $value->get_error_code(),
            'message' => $value->get_error_message(),
        ],
        is_object($value)   => ['t' => 'object', 'class' => get_class($value), 'v' => $pairs(get_object_vars($value))],
        default => throw new RuntimeException('cannot encode ' . get_debug_type($value)),
    };
}

/** The `_doing_it_wrong()` calls of the current call. */
$doing_it_wrong = [];

/** The `register_taxonomy_for_object_type()` calls of the current call. */
$recorder_taxonomies = [];

/** The permalink structure and attachment pages option the current scenario runs under. */
$recorder_permalinks = '';
$recorder_attachment_pages = '0';

function record_doing_it_wrong($function_name, $message, $version): void
{
    $GLOBALS['doing_it_wrong'][] = ['function' => $function_name, 'message' => $message, 'version' => $version];
}

function recorder_permalink_structure()
{
    return $GLOBALS['recorder_permalinks'];
}

function recorder_attachment_pages()
{
    return $GLOBALS['recorder_attachment_pages'];
}

/** Hooks the recorder keeps for itself and leaves out of the action diff. */
const RECORDER_HOOKS = [
    'doing_it_wrong_run',
    'pre_option_permalink_structure',
    'pre_option_wp_attachment_pages_enabled',
];

/** Empties the hooks and the post type and status globals, then sets the options a scenario runs under. */
function reset_state(string $permalinks = '', string $attachment_pages = '0'): void
{
    $GLOBALS['wp_filter'] = [];
    $GLOBALS['wp_post_types'] = [];
    $GLOBALS['wp_post_statuses'] = [];
    $GLOBALS['_wp_post_type_features'] = [];
    $GLOBALS['post_type_meta_caps'] = [];
    $GLOBALS['wp_rewrite']->extra_rules_top = [];
    $GLOBALS['recorder_permalinks'] = $permalinks;
    $GLOBALS['recorder_attachment_pages'] = $attachment_pages;
    add_action('doing_it_wrong_run', 'record_doing_it_wrong', 10, 3);
    add_filter('pre_option_permalink_structure', 'recorder_permalink_structure');
    add_filter('pre_option_wp_attachment_pages_enabled', 'recorder_attachment_pages');
}

/** Every hooked callback as `"hook|priority|id" => [hook, callback, priority, accepted_args]`. */
function action_snapshot(): array
{
    $snapshot = [];
    foreach ($GLOBALS['wp_filter'] as $hook => $wp_hook) {
        if (in_array($hook, RECORDER_HOOKS, true)) {
            continue;
        }
        foreach ($wp_hook->callbacks as $priority => $callbacks) {
            foreach ($callbacks as $id => $callback) {
                $snapshot["$hook|$priority|$id"] = [
                    enc((string) $hook),
                    enc($callback['function']),
                    (int) $priority,
                    (int) $callback['accepted_args'],
                ];
            }
        }
    }
    return $snapshot;
}

/** Fails the run on any warning so far. */
function check_warnings(string $where): void
{
    if ($GLOBALS['warnings']) {
        throw new RuntimeException("$where raised: " . implode('; ', $GLOBALS['warnings']));
    }
}

/** Calls `$fn(...$args)`, recording its result, `_doing_it_wrong()` messages, action and taxonomy changes. */
function call(string $fn, array $args): array
{
    global $doing_it_wrong, $recorder_taxonomies;
    $doing_it_wrong = [];
    $recorder_taxonomies = [];
    $encoded_args = enc($args);
    $before = action_snapshot();
    $result = $fn(...$args);
    $after = action_snapshot();
    check_warnings($fn);
    return [
        'fn'              => $fn,
        'args'            => $encoded_args,
        'result'          => enc($result),
        'doing_it_wrong'  => $doing_it_wrong,
        'actions'         => array_values(array_diff_key($after, $before)),
        'actions_removed' => array_values(array_diff_key($before, $after)),
        'taxonomies'      => $recorder_taxonomies,
    ];
}

/** The post type and status globals as a scenario leaves them. */
function globals_snapshot(): array
{
    return [
        'wp_post_types'    => enc($GLOBALS['wp_post_types']),
        'wp_post_statuses' => enc($GLOBALS['wp_post_statuses']),
        'features'         => enc($GLOBALS['_wp_post_type_features']),
        'meta_caps'        => enc($GLOBALS['post_type_meta_caps']),
    ];
}

/** Runs a scenario's calls from a clean state. */
function scenario(string $name, array $calls, string $permalinks = ''): array
{
    reset_state($permalinks);
    $recorded = [];
    foreach ($calls as [$fn, $args]) {
        $recorded[] = call($fn, $args);
    }
    return ['name' => $name, 'permalinks' => $permalinks, 'calls' => $recorded] + globals_snapshot();
}

$cases = [
    'wordpress'     => $GLOBALS['wp_version'],
    'php'           => PHP_VERSION,
    'labels'        => [],
    'caps'          => [],
    'set_props'     => [],
    'post_types'    => [],
    'statuses'      => [],
    'status_labels' => [],
    'lists'         => [],
    'initial'       => [],
];
if ('7.1.5' !== $cases['wordpress']) {
    fwrite(STDERR, "expected WordPress 7.1.5, found {$cases['wordpress']}\n");
    exit(1);
}

// get_post_type_labels().
$labels = [
    'no-labels'          => ['labels' => []],
    'label-only'         => ['label' => 'Books', 'labels' => []],
    'name-only'          => ['labels' => ['name' => 'Books']],
    'singular-only'      => ['labels' => ['singular_name' => 'Book']],
    'name-and-singular'  => ['labels' => ['name' => 'Books', 'singular_name' => 'Book']],
    'template-name'      => ['labels' => ['singular_name' => 'Book', 'template_name' => 'Book template']],
    'hierarchical'       => ['labels' => [], 'hierarchical' => true],
    'labels-string'      => ['labels' => 'Books'],
    'extra-key'          => ['labels' => ['name' => 'Books', 'custom' => 'Custom']],
    'empty-name-label'   => ['label' => 'L', 'labels' => ['name' => '']],
    'name-admin-bar'     => ['labels' => ['name_admin_bar' => 'Book bar']],
    'singular-null-name' => ['labels' => ['name' => 'Books', 'singular_name' => null]],
];
reset_state();
foreach ($labels as $name => $input) {
    $input = ['name' => 'book'] + $input + ['hierarchical' => false];
    $object = (object) $input;
    $encoded = enc($object);
    $result = get_post_type_labels($object);
    check_warnings("labels/$name");
    $cases['labels'][] = ['name' => $name, 'object' => $encoded, 'result' => enc($result)];
}

// get_post_type_capabilities().
$caps = [
    'post-map'          => ['post', true, []],
    'page-map'          => ['page', true, []],
    'book-no-map'       => ['book', false, []],
    'book-map'          => ['book', true, []],
    'pair-map'          => [['story', 'stories'], true, []],
    'create-posts'      => ['post', true, ['create_posts' => 'upload_files']],
    'edit-post-extra'   => ['post', true, ['edit_post' => 'edit_thing', 'extra' => 'x']],
    'create-posts-null' => ['post', false, ['create_posts' => null]],
    'int-type'          => [5, true, []],
];
foreach ($caps as $name => [$capability_type, $map_meta_cap, $capabilities]) {
    reset_state();
    $result = get_post_type_capabilities((object) [
        'capability_type' => $capability_type,
        'map_meta_cap'    => $map_meta_cap,
        'capabilities'    => $capabilities,
    ]);
    check_warnings("caps/$name");
    $cases['caps'][] = [
        'name'            => $name,
        'capability_type' => enc($capability_type),
        'map_meta_cap'    => enc($map_meta_cap),
        'capabilities'    => enc($capabilities),
        'result'          => enc($result),
        'meta_caps'       => enc($GLOBALS['post_type_meta_caps']),
    ];
}

// WP_Post_Type::set_props().
$set_props = [
    ['defaults', '', []],
    ['public', '', ['public' => true]],
    ['public-no-ui', '', ['public' => true, 'show_ui' => false]],
    ['ui-in-tools', '', ['show_ui' => true, 'show_in_menu' => 'tools.php']],
    ['rest', '', ['show_in_rest' => true]],
    ['rest-namespace', '', ['show_in_rest' => true, 'rest_namespace' => 'my/v1']],
    ['capability-book', '', ['capability_type' => 'book', 'map_meta_cap' => true]],
    ['capabilities-given', '', ['capability_type' => 'post', 'capabilities' => ['edit_post' => 'edit_thing']]],
    ['query-var-string', '', ['query_var' => 'My Books!']],
    ['query-var-false', '', ['query_var' => false]],
    ['unknown-args', '', ['foo' => 'bar', 'public' => true, 'zap' => 'z']],
    ['edit-link-no-ui', '', ['_edit_link' => 'thing.php?id=%d', 'show_ui' => false]],
    ['label', '', ['label' => 'Books']],
    ['rewrite-true', '/%postname%/', ['rewrite' => true]],
    [
        'rewrite-feeds-no-archive',
        '/%postname%/',
        ['rewrite' => ['slug' => 'b', 'feeds' => true], 'has_archive' => false],
    ],
    ['has-archive', '/%postname%/', ['has_archive' => true]],
    ['archive-no-front', '/%postname%/', ['has_archive' => 'books', 'rewrite' => ['with_front' => false]]],
    ['epmask', '/%postname%/', ['permalink_epmask' => 2]],
    ['rewrite-string', '/%postname%/', ['rewrite' => 'x']],
    ['rewrite-false', '/%postname%/', ['rewrite' => false]],
    ['rewrite-true-no-permalinks', '', ['rewrite' => true]],
];
foreach ($set_props as [$name, $permalinks, $args]) {
    reset_state($permalinks);
    $object = new WP_Post_Type('book', $args);
    check_warnings("set_props/$name");
    $cases['set_props'][] = [
        'name'       => $name,
        'post_type'  => 'book',
        'args'       => enc($args),
        'permalinks' => $permalinks,
        'object'     => enc(get_object_vars($object)),
    ];
}

// register_post_type() and the feature API.
$post_types = [
    'name-rules' => [
        ['register_post_type', ['Book Type!']],
        ['register_post_type', ['']],
        ['register_post_type', ['0']],
        ['register_post_type', [str_repeat('a', 20)]],
        ['register_post_type', [str_repeat('a', 21)]],
        ['register_post_type', ['123']],
    ],
    'supports-list' => [
        ['register_post_type', ['book', ['supports' => ['title', 'editor' => ['notes' => true], 'thumbnail']]]],
    ],
    'supports-false' => [
        ['register_post_type', ['book', ['supports' => false]]],
    ],
    'supports-empty' => [
        ['register_post_type', ['book', ['supports' => []]]],
    ],
    'supports-editor-only' => [
        ['register_post_type', ['book', ['supports' => ['editor']]]],
    ],
    'supports-autosave-first' => [
        ['add_post_type_support', ['book', 'autosave']],
        ['register_post_type', ['book', ['supports' => ['editor']]]],
    ],
    'meta-box-cb' => [
        ['register_post_type', ['book', ['register_meta_box_cb' => 'speedyspec_meta_boxes']]],
    ],
    'taxonomies' => [
        ['register_post_type', ['book', ['taxonomies' => ['category', 'post_tag']]]],
    ],
    're-register' => [
        ['register_post_type', ['book', [
            'supports'        => ['title', 'comments'],
            'capability_type' => 'book',
            'map_meta_cap'    => true,
        ]]],
        ['register_post_type', ['movie']],
        ['register_post_type', ['book', [
            'supports'        => ['editor'],
            'capability_type' => 'tome',
            'map_meta_cap'    => true,
        ]]],
    ],
    'unregister' => [
        ['register_post_type', ['book', ['register_meta_box_cb' => 'speedyspec_meta_boxes', 'map_meta_cap' => true]]],
        ['unregister_post_type', ['book']],
        ['unregister_post_type', ['missing']],
        ['register_post_type', ['core', ['_builtin' => true]]],
        ['unregister_post_type', ['core']],
    ],
    'supports-api' => [
        ['add_post_type_support', ['book', ['a', 'b']]],
        ['add_post_type_support', ['book', 'c', 'x', 'y']],
        ['post_type_supports', ['book', 'a']],
        ['post_type_supports', ['book', 'c']],
        ['post_type_supports', ['book', 'z']],
        ['get_all_post_type_supports', ['book']],
        ['get_all_post_type_supports', ['none']],
        ['remove_post_type_support', ['book', 'a']],
        ['remove_post_type_support', ['book', 'b']],
        ['remove_post_type_support', ['book', 'c']],
        ['get_all_post_type_supports', ['book']],
    ],
];
foreach ($post_types as $name => $calls) {
    $cases['post_types'][] = scenario($name, $calls);
}

// register_post_status().
$statuses = [
    'defaults' => [['register_post_status', ['archived']]],
    'public' => [['register_post_status', ['archived', ['public' => true]]]],
    'protected' => [['register_post_status', ['archived', ['protected' => true]]]],
    'private' => [['register_post_status', ['archived', ['private' => true]]]],
    'internal-false' => [['register_post_status', ['archived', ['internal' => false]]]],
    'label' => [['register_post_status', ['archived', ['label' => 'Archived']]]],
    'label-count' => [['register_post_status', ['archived', [
        'label_count' => _n_noop('Archived (%s)', 'Archived (%s)'),
    ]]]],
    'exclude-internal' => [
        ['register_post_status', ['archived', ['exclude_from_search' => false, 'internal' => true]]],
    ],
    'admin-all-list' => [['register_post_status', ['archived', ['show_in_admin_all_list' => false]]]],
    'odd-name' => [['register_post_status', ['Odd Status!']]],
    'empty-name' => [['register_post_status', ['']]],
    'unknown-arg' => [['register_post_status', ['archived', ['foo' => 'bar']]]],
    're-register' => [
        ['register_post_status', ['first', ['public' => true]]],
        ['register_post_status', ['second']],
        ['register_post_status', ['third']],
        ['register_post_status', ['first', ['private' => true]]],
    ],
    'get-object' => [
        ['register_post_status', ['archived']],
        ['get_post_status_object', ['archived']],
        ['get_post_status_object', ['missing']],
    ],
];
foreach ($statuses as $name => $calls) {
    $cases['statuses'][] = scenario($name, $calls);
}

reset_state();
$cases['status_labels'] = [
    'post'    => enc(get_post_statuses()),
    'page'    => enc(get_page_statuses()),
    'privacy' => enc(_wp_privacy_statuses()),
];
check_warnings('status_labels');

// get_post_types(), get_post_stati(), get_post_types_by_support() and the predicates.
$type_names = ['book', 'secret', 'guide', 'one', 'yes', 'fifth', 'missing'];
$status_names = ['open', 'held', 'gone', 'seen', 'missing'];
$lists = [
    ['register_post_type', ['book', ['public' => true]]],
    ['register_post_type', ['secret', ['show_ui' => true]]],
    ['register_post_type', ['guide', ['hierarchical' => true, 'show_in_menu' => 'tools.php']]],
    ['register_post_type', ['one', ['public' => 1]]],
    ['register_post_type', ['yes', ['public' => 'yes']]],
    ['register_post_type', ['fifth', ['menu_position' => 5]]],
    ['register_post_status', ['open', ['public' => true]]],
    ['register_post_status', ['held', ['protected' => true]]],
    ['register_post_status', ['gone', ['internal' => true]]],
    ['register_post_status', ['seen', ['publicly_queryable' => true, 'protected' => false]]],
    ['add_post_type_support', ['book', ['a', 'c']]],
    ['add_post_type_support', ['guide', 'a']],
    ['add_post_type_support', ['secret', 'c']],
];
$type_queries = [
    [],
    ['public' => true],
    ['public' => 1],
    ['public' => '1'],
    ['public' => false],
    ['_builtin' => false],
    ['show_in_menu' => 'tools.php'],
    ['menu_position' => null],
    ['menu_position' => '5'],
    ['name' => 'book'],
];
foreach ($type_queries as $query) {
    foreach (['names', 'objects'] as $output) {
        $lists[] = ['get_post_types', [$query, $output, 'and']];
    }
}
foreach (['and', 'or', 'not', 'Or', 'bogus'] as $operator) {
    foreach (['names', 'objects'] as $output) {
        $lists[] = ['get_post_types', [['public' => true, 'hierarchical' => true], $output, $operator]];
    }
}
foreach (['names', 'objects'] as $output) {
    $lists[] = ['get_post_types', [[], $output, 'bogus']];
}
$lists[] = ['get_post_stati', [['public' => true]]];
$lists[] = ['get_post_stati', [['internal' => true]]];
$lists[] = ['get_post_stati', [['protected' => true, 'private' => true], 'names', 'or']];
$lists[] = ['get_post_stati', [['public' => true], 'objects']];
$lists[] = ['get_post_types_by_support', ['a']];
$lists[] = ['get_post_types_by_support', [['a', 'c']]];
$lists[] = ['get_post_types_by_support', [['a', 'c'], 'or']];
$lists[] = ['get_post_types_by_support', [['a', 'a']]];
$lists[] = ['get_post_types_by_support', ['a', 'not']];
$lists[] = ['get_post_types_by_support', ['a', 'xor']];
foreach ($type_names as $type) {
    $lists[] = ['post_type_exists', [$type]];
    $lists[] = ['is_post_type_hierarchical', [$type]];
    $lists[] = ['is_post_type_viewable', [$type]];
}
foreach ($status_names as $status) {
    $lists[] = ['is_post_status_viewable', [$status]];
}
$cases['lists'][] = scenario('lists', $lists);

// create_initial_post_types().
foreach ([['plain', '', '0'], ['pretty', '/%postname%/', '1']] as [$name, $permalinks, $attachment_pages]) {
    reset_state($permalinks, $attachment_pages);
    $recorder_taxonomies = [];
    create_initial_post_types();
    check_warnings("initial/$name");
    $initial = [
        'name'             => $name,
        'permalinks'       => $permalinks,
        'attachment_pages' => $attachment_pages,
    ] + globals_snapshot() + [
        'actions'    => array_values(action_snapshot()),
        'taxonomies' => $recorder_taxonomies,
    ];
    $queries = [
        ['get_post_types', [['public' => true]]],
        ['get_post_types', [['show_in_rest' => true], 'objects']],
        ['get_post_types', [['_builtin' => false]]],
        ['get_post_stati', [['show_in_admin_status_list' => true]]],
        ['get_post_stati', [['internal' => true]]],
        ['get_post_types_by_support', ['editor']],
    ];
    foreach (array_keys($GLOBALS['wp_post_types']) as $type) {
        $queries[] = ['is_post_type_viewable', [(string) $type]];
    }
    foreach (array_keys($GLOBALS['wp_post_statuses']) as $status) {
        $queries[] = ['is_post_status_viewable', [(string) $status]];
    }
    $initial['queries'] = array_map(static fn ($q) => call($q[0], $q[1]), $queries);
    $cases['initial'][] = $initial;
}

if ($warnings) {
    fwrite(STDERR, "warnings:\n" . implode("\n", $warnings) . "\n");
    exit(1);
}

file_put_contents(
    rtrim($out, '/') . '/cases.json',
    json_encode($cases, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n"
);
$calls = static fn (array $scenarios) => array_sum(array_map(static fn ($s) => count($s['calls']), $scenarios));
printf(
    "WordPress %s, PHP %s: %d labels, %d caps, %d set_props, %d post_types scenarios (%d calls), "
    . "%d statuses scenarios (%d calls), %d status_labels, %d lists scenarios (%d calls), %d initial (%d queries)\n",
    $cases['wordpress'],
    $cases['php'],
    count($cases['labels']),
    count($cases['caps']),
    count($cases['set_props']),
    count($cases['post_types']),
    $calls($cases['post_types']),
    count($cases['statuses']),
    $calls($cases['statuses']),
    count($cases['status_labels']),
    count($cases['lists']),
    $calls($cases['lists']),
    count($cases['initial']),
    array_sum(array_map(static fn ($i) => count($i['queries']), $cases['initial']))
);
