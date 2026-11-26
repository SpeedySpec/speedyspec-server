<?php
/**
 * Records WordPress's post entity, date, slug and hierarchy rules into the fixtures the Rust posts port is tested
 * against.
 *
 * Usage: php record.php <wordpress-root> <out-dir> [<expected-wordpress-version>]
 *
 * The recording stops unless the release's `$wp_version` is the expected version: the third argument, else
 * `SPEEDYSPEC_COMPAT_WORDPRESS`, else the repository's pin, `compat/wordpress.json`'s `wordpress`.
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`) against the pinned WordPress root
 * that `speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())` unpacks.
 * It is apart from `compat/posts/record.php` because it loads `class-wp-post.php` behind a stub `$wpdb`, which that
 * recording never needs; the two recordings stay independent. It records these sections into
 * `<out-dir>/cases.json`:
 *
 * - `rows`: `{name, row, cached}`. `row` is a `wp_posts` row as mysqli returns it (every column a string, in table
 *   order); `cached` is the value `WP_Post::get_instance()` handed `wp_cache_set()` after reading it.
 * - `cache_reads`: `{name, cached, queried, post}`. `get_post( 3 )` with `cached` as the `posts` cache entry for 3;
 *   `queried` says whether the `find` statement ran; `post` is `get_object_vars()` of the result, or null.
 * - `dates`: `{name, post_date, post_date_gmt, timezone, result, clock}`. `wp_resolve_post_date()`; `timezone` is
 *   `wp_timezone_string()`; `clock` holds the answers of the time functions while the call ran: `now_local` and
 *   `now_gmt` (`current_time( 'mysql' )` and `current_time( 'mysql', true )`, read before and after the call, which
 *   is retried until both reads agree), and `date_from_gmt` and `gmt_from_date`, `[[in, out]]` pairs of
 *   `get_date_from_gmt()` and `get_gmt_from_date()` for the inputs the case gives.
 * - `slugs` and `hierarchy`: scenarios `{name, permalinks, post_types, rows, calls}`. `post_types` are `[name,
 *   hierarchical]`; `rows` are the scenario's `wp_posts` rows; each call is `{fn, args, result, statements}`, where
 *   `statements` are the `$wpdb` reads it made, in order, each `[label, [args]]` with the prepared arguments.
 * - `pages`: `{name, pages, calls}`. `get_page_hierarchy()` and `get_page_children()` over `WP_Post` objects built
 *   from `pages` (rows, sanitized as `get_instance()` does), with no database; each call is `{fn, args, result}`,
 *   `args` without the page list.
 * - `extended`: `{content, result}`, `get_extended()`.
 *
 * Value encoding. Every PHP value is written as `{"t": <type>, "v": <value>}`, so key order and bytes survive JSON:
 *
 * - `{"t":"null"}` (no `v`), `{"t":"bool","v":true}`, `{"t":"int","v":10}`, `{"t":"float","v":1.5}`;
 * - `{"t":"str","v":"<lowercase hex of the bytes>"}`;
 * - `{"t":"array","v":[[key, value], ...]}` in PHP's order, where a key is `{"t":"int","v":N}` or
 *   `{"t":"str","v":"<hex>"}` and a value is encoded the same way, recursively;
 * - `{"t":"object","class":"<class>","v":[[key, value], ...]}`, the pairs in `get_object_vars()` order, keys and
 *   values encoded as an array's are.
 *
 * Encoded: `row`, `cached`, `post`, `post_date`, `post_date_gmt`, `result`, `args`, the statement arguments, the
 * `date_from_gmt` and `gmt_from_date` pairs, `pages` and `content`. Plain JSON: names, `queried`, `timezone`,
 * `now_local`, `now_gmt`, `permalinks`, `post_types`, statement labels and `fn`.
 *
 * Loaded from WordPress: version.php, compat-utf8.php and utf8.php, plugin.php, load.php, class-wp-error.php,
 * functions.php (current_time, wp_checkdate, wp_find_hierarchy_loop; it requires option.php), formatting.php
 * (wp_truncate_slug, get_date_from_gmt), class-wp-post.php, post.php.
 *
 * Stubbed, because the loaded files call them and they live in files not loaded here:
 * - `__()` and `_x()` (l10n.php) return their text;
 * - `OBJECT`, `ARRAY_A` and `ARRAY_N` (class-wpdb.php) are their names, as there;
 * - `wp_cache_get()` (cache.php) returns the current `cache_reads` case's entry, and `false` everywhere else, so
 *   every other read reaches `$wpdb` as uncached WordPress does; `wp_cache_set()` and `wp_cache_add()` keep the
 *   value they get (the last one, encoded) and return true;
 * - `$wpdb` is a {@link RecorderWpdb} with `posts` `wp_posts`. Its `prepare()` returns the query and its arguments
 *   (`%d` cast to int, `%s` to string) unformatted. Its `get_row()` and `get_var()` answer exactly four templates
 *   from the scenario's rows, comparing bytes, first match in row order: `find` (`SELECT * … WHERE ID = %d LIMIT
 *   1`, a `stdClass` of strings) and `wp_unique_post_slug()`'s `attachment_slug`, `hierarchical_slug` and
 *   `flat_slug` (the matching row's `post_name`). Any other query stops the run;
 * - `$wp_rewrite` is an object with the five default `feeds` and `pagination_base` `page`;
 * - `$wp_post_types` holds `stdClass` objects with `name` and `hierarchical`, from the scenario's `post_types`.
 * Options come through `pre_option_permalink_structure`, `pre_option_timezone_string` and `pre_option_gmt_offset`.
 */

declare(strict_types=1);

[$_, $root, $out, $expected] = $argv + [null, null, null, null];
if (!$root || !$out) {
    fwrite(STDERR, "usage: php record.php <wordpress-root> <out-dir> [<expected-wordpress-version>]\n");
    exit(2);
}
if (!$expected) {
    $expected = getenv('SPEEDYSPEC_COMPAT_WORDPRESS')
        ?: json_decode((string) file_get_contents(__DIR__ . '/../wordpress.json'), true)['wordpress'];
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
define('WP_DEBUG', false);
define('OBJECT', 'OBJECT');
define('ARRAY_A', 'ARRAY_A');
define('ARRAY_N', 'ARRAY_N');
error_reporting(E_ALL);
date_default_timezone_set('UTC');

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

/** The `posts` cache entry the current `cache_reads` case stubs; `false` outside them. */
$recorder_cached = false;

/** The last value `wp_cache_set()` or `wp_cache_add()` got, encoded. */
$recorder_cache_set = null;

function wp_cache_get($key, $group = '', $force = false, &$found = null)
{
    $found = false !== $GLOBALS['recorder_cached'];
    return $GLOBALS['recorder_cached'];
}

function wp_cache_set($key, $data, $group = '', $expire = 0)
{
    $GLOBALS['recorder_cache_set'] = enc($data);
    return true;
}

function wp_cache_add($key, $data, $group = '', $expire = 0)
{
    $GLOBALS['recorder_cache_set'] = enc($data);
    return true;
}

/**
 * Answers the `wp_posts` reads `post.php` makes from a scenario's rows.
 *
 * The four templates are the statements `PostRepository` mirrors, so each read is logged under that method's name.
 */
final class RecorderWpdb
{
    public string $posts = 'wp_posts';

    /** The rows reads answer from, each a column => string array in table order. */
    public array $rows = [];

    /** The current call's statements, `[label, [args]]`. */
    public array $statements = [];

    public function prepare($query, ...$args)
    {
        preg_match_all('/%[sd]/', $query, $placeholders);
        if (count($placeholders[0]) !== count($args)) {
            throw new RuntimeException("placeholders and arguments differ: $query");
        }
        foreach ($placeholders[0] as $i => $placeholder) {
            $args[$i] = '%d' === $placeholder ? (int) $args[$i] : (string) $args[$i];
        }
        return [$query, $args];
    }

    public function get_row($prepared)
    {
        [$label, $row] = $this->answer($prepared);
        if ('find' !== $label) {
            throw new RuntimeException("get_row() of $label");
        }
        return null === $row ? null : (object) $row;
    }

    public function get_var($prepared)
    {
        [$label, $row] = $this->answer($prepared);
        if ('find' === $label) {
            throw new RuntimeException('get_var() of find');
        }
        return null === $row ? null : $row['post_name'];
    }

    /** Logs the statement and finds its first matching row. */
    private function answer($prepared): array
    {
        if (!is_array($prepared)) {
            throw new RuntimeException('unprepared query: ' . var_export($prepared, true));
        }
        [$query, $args] = $prepared;
        $label = match ($query) {
            "SELECT * FROM wp_posts WHERE ID = %d LIMIT 1" => 'find',
            "SELECT post_name FROM wp_posts WHERE post_name = %s AND ID != %d LIMIT 1" => 'attachment_slug',
            "SELECT post_name FROM wp_posts WHERE post_name = %s AND post_type IN ( %s, 'attachment' ) "
                . "AND ID != %d AND post_parent = %d LIMIT 1" => 'hierarchical_slug',
            "SELECT post_name FROM wp_posts WHERE post_name = %s AND post_type = %s AND ID != %d LIMIT 1"
                => 'flat_slug',
            default => throw new RuntimeException("unexpected query: $query"),
        };
        $this->statements[] = [$label, array_map('enc', $args)];
        $match = match ($label) {
            'find' => static fn (array $r): bool => $r['ID'] === (string) $args[0],
            'attachment_slug' => static fn (array $r): bool => $r['post_name'] === $args[0]
                && $r['ID'] !== (string) $args[1],
            'hierarchical_slug' => static fn (array $r): bool => $r['post_name'] === $args[0]
                && ($r['post_type'] === $args[1] || 'attachment' === $r['post_type'])
                && $r['ID'] !== (string) $args[2]
                && $r['post_parent'] === (string) $args[3],
            'flat_slug' => static fn (array $r): bool => $r['post_name'] === $args[0]
                && $r['post_type'] === $args[1]
                && $r['ID'] !== (string) $args[2],
        };
        foreach ($this->rows as $row) {
            if ($match($row)) {
                return [$label, $row];
            }
        }
        return [$label, null];
    }
}

$wpdb = new RecorderWpdb();
$wp_rewrite = (object) [
    'feeds'           => ['feed', 'rdf', 'rss', 'rss2', 'atom'],
    'pagination_base' => 'page',
];
$wp_post_types = [];

require ABSPATH . WPINC . '/version.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/plugin.php';
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/class-wp-post.php';
require ABSPATH . WPINC . '/post.php';

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
        is_object($value)   => ['t' => 'object', 'class' => get_class($value), 'v' => $pairs(get_object_vars($value))],
        default => throw new RuntimeException('cannot encode ' . get_debug_type($value)),
    };
}

/** The options the current case runs under. */
$recorder_options = [
    'permalink_structure' => '',
    'timezone_string'     => 'America/Chicago',
    'gmt_offset'          => '0',
];

function recorder_permalink_structure()
{
    return $GLOBALS['recorder_options']['permalink_structure'];
}

function recorder_timezone_string()
{
    return $GLOBALS['recorder_options']['timezone_string'];
}

function recorder_gmt_offset()
{
    return $GLOBALS['recorder_options']['gmt_offset'];
}

add_filter('pre_option_permalink_structure', 'recorder_permalink_structure');
add_filter('pre_option_timezone_string', 'recorder_timezone_string');
add_filter('pre_option_gmt_offset', 'recorder_gmt_offset');

/** Fails the run on any warning so far. */
function check_warnings(string $where): void
{
    if ($GLOBALS['warnings']) {
        throw new RuntimeException("$where raised: " . implode('; ', $GLOBALS['warnings']));
    }
}

/** The 23 `wp_posts` columns in table order. */
const COLUMNS = [
    'ID', 'post_author', 'post_date', 'post_date_gmt', 'post_content', 'post_title', 'post_excerpt', 'post_status',
    'comment_status', 'ping_status', 'post_password', 'post_name', 'to_ping', 'pinged', 'post_modified',
    'post_modified_gmt', 'post_content_filtered', 'post_parent', 'guid', 'menu_order', 'post_type',
    'post_mime_type', 'comment_count',
];

/** A full `wp_posts` row of strings, from a few columns over plain defaults. */
function row(int $id, array $columns = []): array
{
    $defaults = [
        'ID'                    => (string) $id,
        'post_author'           => '1',
        'post_date'             => '2024-03-01 06:00:00',
        'post_date_gmt'         => '2024-03-01 12:00:00',
        'post_content'          => '',
        'post_title'            => '',
        'post_excerpt'          => '',
        'post_status'           => 'publish',
        'comment_status'        => 'open',
        'ping_status'           => 'open',
        'post_password'         => '',
        'post_name'             => "post-$id",
        'to_ping'               => '',
        'pinged'                => '',
        'post_modified'         => '2024-03-01 06:00:00',
        'post_modified_gmt'     => '2024-03-01 12:00:00',
        'post_content_filtered' => '',
        'post_parent'           => '0',
        'guid'                  => "http://example.org/?p=$id",
        'menu_order'            => '0',
        'post_type'             => 'post',
        'post_mime_type'        => '',
        'comment_count'         => '0',
    ];
    $row = [];
    foreach (COLUMNS as $column) {
        $row[$column] = (string) ($columns[$column] ?? $defaults[$column]);
    }
    return $row;
}

/** Sets the rows `$wpdb` answers from and the registered post types. */
function use_scenario(array $rows, array $post_types): void
{
    $GLOBALS['wpdb']->rows = $rows;
    $GLOBALS['wp_post_types'] = [];
    foreach ($post_types as [$name, $hierarchical]) {
        $GLOBALS['wp_post_types'][$name] = (object) ['name' => $name, 'hierarchical' => $hierarchical];
    }
}

/** Calls `$fn(...$args)`, recording its result and the statements it ran. */
function call(string $fn, array $args): array
{
    $GLOBALS['wpdb']->statements = [];
    $encoded_args = enc($args);
    $result = $fn(...$args);
    check_warnings($fn);
    return [
        'fn'         => $fn,
        'args'       => $encoded_args,
        'result'     => enc($result),
        'statements' => $GLOBALS['wpdb']->statements,
    ];
}

/** Runs a scenario's calls over its rows, post types and permalink structure. */
function scenario(string $name, string $permalinks, array $post_types, array $rows, array $calls): array
{
    use_scenario($rows, $post_types);
    $GLOBALS['recorder_options']['permalink_structure'] = $permalinks;
    $recorded = [];
    foreach ($calls as [$fn, $args]) {
        $recorded[] = call($fn, $args);
    }
    return [
        'name'       => $name,
        'permalinks' => $permalinks,
        'post_types' => $post_types,
        'rows'       => array_map('enc', $rows),
        'calls'      => $recorded,
    ];
}

$cases = [
    'wordpress'   => $GLOBALS['wp_version'],
    'php'         => PHP_VERSION,
    'rows'        => [],
    'cache_reads' => [],
    'dates'       => [],
    'slugs'       => [],
    'hierarchy'   => [],
    'pages'       => [],
    'extended'    => [],
];
if ($expected !== $cases['wordpress']) {
    fwrite(STDERR, "expected WordPress {$expected}, found {$cases['wordpress']}\n");
    exit(1);
}

const POST_TYPES = [
    ['post', false],
    ['page', true],
    ['attachment', false],
    ['revision', false],
    ['nav_menu_item', false],
    ['user_request', false],
    ['book', true],
];

// The row a get_instance() miss reads and what it caches.
$rows = [
    'every-column' => row(1, [
        'post_author'           => '5',
        'post_content'          => "Hello\nworld",
        'post_title'            => 'Hello',
        'post_excerpt'          => 'Short',
        'comment_status'        => 'closed',
        'ping_status'           => 'closed',
        'post_password'         => 'secret',
        'post_name'             => 'hello',
        'to_ping'               => 'http://example.com/',
        'pinged'                => 'http://example.net/',
        'post_content_filtered' => 'filtered',
        'post_parent'           => '2',
        'menu_order'            => '4',
        'post_type'             => 'page',
        'post_mime_type'        => 'text/plain',
        'comment_count'         => '1',
    ]),
    'zero-date' => row(2, [
        'post_date'         => '0000-00-00 00:00:00',
        'post_date_gmt'     => '0000-00-00 00:00:00',
        'post_modified'     => '0000-00-00 00:00:00',
        'post_modified_gmt' => '0000-00-00 00:00:00',
        'post_status'       => 'draft',
    ]),
    'bytes-and-counts' => row(3, [
        'post_title'    => "Caf\xc3\xa9 \xff\xfe",
        'post_name'     => '%e2%82%ac-caf%c3%a9',
        'comment_count' => '7',
        'menu_order'    => '-3',
    ]),
];
foreach ($rows as $name => $row) {
    use_scenario([$row], POST_TYPES);
    $recorder_cache_set = null;
    WP_Post::get_instance((int) $row['ID']);
    check_warnings("rows/$name");
    if (null === $recorder_cache_set) {
        throw new RuntimeException("rows/$name cached nothing");
    }
    $cases['rows'][] = ['name' => $name, 'row' => enc($row), 'cached' => $recorder_cache_set];
}

// get_post( 3 ) over posts cache entries of each shape.
$row3 = row(3, ['post_title' => 'From the database', 'post_parent' => '1', 'menu_order' => '2']);
$raw = static function (array $columns): stdClass {
    $object = (object) $columns;
    $object->ID = (int) $object->ID;
    $object->post_parent = (int) $object->post_parent;
    $object->menu_order = (int) $object->menu_order;
    $object->filter = 'raw';
    return $object;
};
$cached_row = row(3, ['post_title' => 'From the cache', 'post_parent' => '1', 'menu_order' => '2']);
$without_title = $raw($cached_row);
unset($without_title->post_title);
$without_id = $raw($cached_row);
unset($without_id->ID);
$display = (object) $cached_row;
$display->filter = 'display';
$cache_reads = [
    'raw-stdclass'   => $raw($cached_row),
    'no-filter'      => (object) $cached_row,
    'display-filter' => $display,
    'wp-post'        => new WP_Post($raw($cached_row)),
    'missing-title'  => $without_title,
    'array'          => (array) $raw($cached_row),
    'no-id'          => $without_id,
    'false'          => false,
];
foreach ($cache_reads as $name => $cached) {
    use_scenario([$row3], POST_TYPES);
    $encoded = enc($cached);
    $wpdb->statements = [];
    $recorder_cached = $cached;
    $post = get_post(3);
    $recorder_cached = false;
    check_warnings("cache_reads/$name");
    $cases['cache_reads'][] = [
        'name'    => $name,
        'cached'  => $encoded,
        'queried' => in_array('find', array_column($wpdb->statements, 0), true),
        'post'    => null === $post ? enc(null) : enc(get_object_vars($post)),
    ];
}

// wp_resolve_post_date().
$dates = [
    ['empty', '', ''],
    ['zero-string', '0', ''],
    ['zero-date-from-gmt', '0000-00-00 00:00:00', '2024-03-01 12:00:00'],
    ['empty-from-gmt', '', '2024-03-01 12:00:00'],
    ['empty-from-gmt-offset', '', '2024-03-01 12:00:00', '', '5.5'],
    ['garbage-gmt', '', 'garbage'],
    ['leap-day', '2024-02-29 10:00:00', ''],
    ['not-leap-day', '2023-02-29 10:00:00', ''],
    ['unanchored-end', '2024-1-5 garbage', ''],
    ['two-digit-year', '24-01-05', ''],
    ['month-13', '2024-13-01', ''],
    ['year-zero', '0000-01-01 00:00:00', ''],
    ['feb-30', '2024-02-30', ''],
    ['leading-space', ' 2024-01-01', ''],
    ['letters', 'abc', ''],
];
$is_empty_date = static fn (string $date): bool => empty($date) || '0000-00-00 00:00:00' === $date;
foreach ($dates as $case) {
    [$name, $post_date, $post_date_gmt] = $case;
    $recorder_options['timezone_string'] = $case[3] ?? 'America/Chicago';
    $recorder_options['gmt_offset'] = $case[4] ?? '0';
    do {
        $now_local = current_time('mysql');
        $now_gmt = current_time('mysql', true);
        $result = wp_resolve_post_date($post_date, $post_date_gmt);
    } while ($now_local !== current_time('mysql') || $now_gmt !== current_time('mysql', true));
    $date_from_gmt = [];
    if (!$is_empty_date($post_date_gmt)) {
        $date_from_gmt[] = [enc($post_date_gmt), enc(get_date_from_gmt($post_date_gmt))];
    }
    $gmt_from_date = [];
    foreach (array_unique([$post_date, is_string($result) ? $result : '']) as $local) {
        if (!$is_empty_date($local)) {
            $gmt_from_date[] = [enc($local), enc(get_gmt_from_date($local))];
        }
    }
    check_warnings("dates/$name");
    $cases['dates'][] = [
        'name'          => $name,
        'post_date'     => enc($post_date),
        'post_date_gmt' => enc($post_date_gmt),
        'timezone'      => wp_timezone_string(),
        'result'        => enc($result),
        'clock'         => [
            'now_local'     => $now_local,
            'now_gmt'       => $now_gmt,
            'date_from_gmt' => $date_from_gmt,
            'gmt_from_date' => $gmt_from_date,
        ],
    ];
}
$recorder_options['timezone_string'] = 'America/Chicago';
$recorder_options['gmt_offset'] = '0';

// wp_unique_post_slug().
$slug = static fn (string $slug, int $id = 0, string $type = 'post', int $parent = 0, string $status = 'publish')
    => ['wp_unique_post_slug', [$slug, $id, $status, $type, $parent]];
$long_a = str_repeat('a', 200);
$long_b = str_repeat('b', 200);
$long_b_taken = [row(60, ['post_name' => $long_b])];
foreach (range(2, 9) as $n) {
    $long_b_taken[] = row(60 + $n, ['post_name' => str_repeat('b', 198) . "-$n"]);
}
$slugs = [
    ['skips', '', [row(1, ['post_name' => 'hello'])], [
        $slug('hello', 0, 'post', 0, 'draft'),
        $slug('hello', 0, 'post', 0, 'pending'),
        $slug('hello', 0, 'post', 0, 'auto-draft'),
        $slug('hello', 0, 'revision', 0, 'inherit'),
        $slug('hello', 0, 'user_request'),
    ]],
    ['flat', '', array_merge([
        row(10, ['post_name' => 'taken']),
        row(11, ['post_name' => 'twice']),
        row(12, ['post_name' => 'twice-2']),
        row(13, ['post_name' => 'page-only', 'post_type' => 'page']),
        row(14, ['post_name' => 'mine']),
        row(15, ['post_name' => '0']),
        row(16, ['post_name' => '']),
        row(17, ['post_name' => 'abc-']),
        row(18, ['post_name' => $long_a]),
    ], $long_b_taken), [
        $slug('free'),
        $slug('taken'),
        $slug('twice'),
        $slug('page-only'),
        $slug('mine', 14),
        $slug('feed'),
        $slug('rss2'),
        $slug('embed'),
        $slug('0'),
        $slug(''),
        $slug('abc-'),
        $slug($long_a),
        $slug($long_b),
    ]],
    ['dates-postname', '/%postname%/', [], [
        $slug('2024'),
        $slug('0'),
        $slug("5\n"),
        $slug('2024', 0, 'book'),
        $slug('2024', 0, 'revision'),
    ]],
    ['dates-year', '/%year%/%postname%/', [], [
        $slug('12'),
        $slug('13'),
    ]],
    ['dates-monthnum', '/%year%/%monthnum%/%postname%/', [], [
        $slug('31'),
        $slug('32'),
    ]],
    ['dates-own-name', '/%postname%/', [row(9, ['post_name' => '2024'])], [
        $slug('2024', 9),
    ]],
    ['hierarchical', '', [
        row(20, ['post_name' => 'top', 'post_type' => 'page']),
        row(21, ['post_name' => 'child', 'post_type' => 'page', 'post_parent' => '20']),
        row(22, ['post_name' => 'pic', 'post_type' => 'attachment', 'post_parent' => '20']),
        row(23, ['post_name' => 'news']),
    ], [
        $slug('fresh', 0, 'page'),
        $slug('child', 0, 'page', 20),
        $slug('child', 0, 'page', 0),
        $slug('pic', 0, 'page', 20),
        $slug('news', 0, 'page'),
        $slug('2', 0, 'page'),
        $slug('page2', 0, 'page'),
        $slug('page', 0, 'page'),
        $slug("2\n", 0, 'page'),
    ]],
    ['attachment', '', [row(30, ['post_name' => 'photo', 'post_type' => 'page'])], [
        $slug('photo', 0, 'attachment'),
        $slug('embed', 0, 'attachment'),
        $slug('free', 0, 'attachment'),
    ]],
];
foreach ($slugs as [$name, $permalinks, $scenario_rows, $calls]) {
    $cases['slugs'][] = scenario($name, $permalinks, POST_TYPES, $scenario_rows, $calls);
}

// Parents, ancestors, page URIs and loops.
$page = static fn (int $id, int $parent, string $name)
    => row($id, ['post_type' => 'page', 'post_parent' => (string) $parent, 'post_name' => $name]);
$hierarchy_rows = [
    $page(1, 0, 'a'),
    $page(2, 1, 'b'),
    $page(3, 2, 'c'),
    $page(4, 3, ''),
    $page(5, 4, '%e2%82%ac'),
    $page(6, 6, 'self'),
    $page(7, 8, 'x'),
    $page(8, 7, 'y'),
    $page(9, 7, 'z'),
];
$loop = static fn (int $start, ?int $parent) => ['wp_find_hierarchy_loop', ['wp_get_post_parent_id', $start, $parent]];
$cases['hierarchy'][] = scenario('hierarchy', '', POST_TYPES, $hierarchy_rows, [
    ['wp_get_post_parent_id', [3]],
    ['wp_get_post_parent_id', [1]],
    ['wp_get_post_parent_id', [99]],
    ['get_post_ancestors', [5]],
    ['get_post_ancestors', [1]],
    ['get_post_ancestors', [6]],
    ['get_post_ancestors', [9]],
    ['get_page_uri', [5]],
    ['get_page_uri', [3]],
    ['get_page_uri', [1]],
    ['get_page_uri', [99]],
    ['wp_check_post_hierarchy_for_loops', [0, 3]],
    ['wp_check_post_hierarchy_for_loops', [2, 0]],
    ['wp_check_post_hierarchy_for_loops', [3, 3]],
    ['wp_check_post_hierarchy_for_loops', [5, 2]],
    ['wp_check_post_hierarchy_for_loops', [2, 5]],
    ['wp_check_post_hierarchy_for_loops', [1, 3]],
    $loop(3, 1),
    $loop(2, 5),
    $loop(10, 7),
    $loop(9, null),
]);

// get_page_hierarchy() and get_page_children(), with no database.
$pages = [
    'tree' => [[$page(5, 1, 'e'), $page(3, 0, 'c'), $page(6, 2, 'f'), $page(1, 0, 'a'), $page(4, 3, 'd'),
        $page(2, 1, 'b')], [0, 1, 2]],
    'orphans' => [[$page(1, 0, 'a'), $page(2, 50, 'b'), $page(3, 50, 'c'), $page(4, 2, 'd')], [0, 50]],
    'duplicate-id' => [[$page(1, 0, 'first'), $page(2, 1, 'b'), $page(1, 0, 'second')], [0, 1]],
    'empty' => [[], [0]],
];
use_scenario([], POST_TYPES);
$wpdb->statements = [];
foreach ($pages as $name => [$page_rows, $starts]) {
    $calls = [];
    foreach ($starts as $start) {
        $objects = array_map(static fn (array $r) => new WP_Post(sanitize_post((object) $r, 'raw')), $page_rows);
        $calls[] = [
            'fn'     => 'get_page_hierarchy',
            'args'   => enc([$start]),
            'result' => enc(get_page_hierarchy($objects, $start)),
        ];
        $calls[] = [
            'fn'     => 'get_page_children',
            'args'   => enc([$start]),
            'result' => enc(get_page_children($start, $objects)),
        ];
        check_warnings("pages/$name");
    }
    if ($wpdb->statements) {
        throw new RuntimeException("pages/$name read the database");
    }
    $cases['pages'][] = ['name' => $name, 'pages' => array_map('enc', $page_rows), 'calls' => $calls];
}

// get_extended().
$extended = [
    'no marker',
    'a<!--more-->b',
    '<!--more Read on-->',
    'one<!--more-->two<!--more-->three',
    "main<!--more\nacross-->rest",
    "  \t main  <!--more  text  -->  rest \n ",
    "line one\nline two<!--more-->after",
    'abc   ',
    "abc  \n ",
    '',
];
foreach ($extended as $content) {
    $result = get_extended($content);
    check_warnings('extended');
    $cases['extended'][] = ['content' => enc($content), 'result' => enc($result)];
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
    "WordPress %s, PHP %s: %d rows, %d cache_reads, %d dates, %d slugs scenarios (%d calls), "
    . "%d hierarchy scenarios (%d calls), %d pages (%d calls), %d extended\n",
    $cases['wordpress'],
    $cases['php'],
    count($cases['rows']),
    count($cases['cache_reads']),
    count($cases['dates']),
    count($cases['slugs']),
    $calls($cases['slugs']),
    count($cases['hierarchy']),
    $calls($cases['hierarchy']),
    count($cases['pages']),
    $calls($cases['pages']),
    count($cases['extended'])
);
