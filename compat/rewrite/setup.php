<?php

/**
 * Sets up the rewrite scenarios the same way on both sides: WordPress's rewrite code (the recorder) or the port
 *  (the replay), over the same stubbed WordPress around them.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's hooks, `class-wp-rewrite.php`, `class-wp-matchesmapregex.php`,
 *  `rewrite.php` and `class-wp.php`. The port's side loads none of them (the hooks and rewrite API are native), and
 *  only `class-wp.php`, with `class-wp.patch` applied, so `WP::parse_request` resolves through the Rust router.
 *
 *  Everything else the rewrite code calls is stubbed identically on both sides: options live in an array, post
 *  types and taxonomies are empty, pages are a fixed list. No database is needed.
 *
 *  There is no `declare(strict_types=1)`: the stubs accept WordPress's loose types, as WordPress's functions do.
 */

foreach (['ABSPATH' => SPEEDYSPEC_COMPAT_WP_ROOT, 'WPINC' => 'wp-includes', 'WP_DEBUG' => false] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
    require ABSPATH . WPINC . '/class-wp-rewrite.php';
    require ABSPATH . WPINC . '/class-wp-matchesmapregex.php';
    require ABSPATH . WPINC . '/rewrite.php';
    require ABSPATH . WPINC . '/class-wp.php';
}
else
{
    eval('?>' . speedyspec_compat_patched(ABSPATH . WPINC . '/class-wp.php', __DIR__ . '/class-wp.patch'));
    // rewrite.php's functions that stay PHP until posts are ported, as WordPress has them.
    eval(speedyspec_compat_function(ABSPATH . WPINC . '/rewrite.php', 'wp_resolve_numeric_slug_conflicts'));
}
// query.php's switch for random content redirects, which `WP_Rewrite::random_rewrite_rules()` reads, as WordPress has
// it on both sides: it stays PHP until queries are ported.
eval(speedyspec_compat_function(ABSPATH . WPINC . '/query.php', 'wp_is_random_content_redirect_enabled'));

/**
 * Extracts one top-level function's source from a WordPress file.
 *
 * @return string
 *   The function's declaration.
 */
function speedyspec_compat_function(string $file, string $name)
{
    $source = file_get_contents($file);
    $start = strpos($source, "function $name(");
    if (false === $start)
    {
        throw new RuntimeException("$file declares no $name()");
    }
    $end = strpos($source, "\n}\n", $start);
    return substr($source, $start, $end - $start + 3);
}

/**
 * Applies a one-hunk unified diff to a file's source.
 *
 * @return string
 *   The patched source.
 */
function speedyspec_compat_patched(string $file, string $patch)
{
    $source = file_get_contents($file);
    $old = array();
    $new = array();
    foreach (explode("\n", file_get_contents($patch)) as $line)
    {
        if (str_starts_with($line, '---') || str_starts_with($line, '+++') || str_starts_with($line, '@@') || '' === $line)
        {
            continue;
        }
        $text = substr($line, 1);
        if ('-' === $line[0])
        {
            $old[] = $text;
        }
        elseif ('+' === $line[0])
        {
            $new[] = $text;
        }
        else
        {
            $old[] = $text;
            $new[] = $text;
        }
    }
    $old = implode("\n", $old);
    if (!str_contains($source, $old))
    {
        throw new RuntimeException("$patch does not apply to $file");
    }
    return str_replace($old, implode("\n", $new), $source);
}

/**
 * The options both sides read and write.
 */
function get_option($option, $default_value = false)
{
    return array_key_exists($option, $GLOBALS['speedyspec_compat_options']) ? $GLOBALS['speedyspec_compat_options'][$option] : $default_value;
}

function update_option($option, $value, $autoload = null)
{
    $GLOBALS['speedyspec_compat_options'][$option] = $value;
    return true;
}

function add_option($option, $value = '', $deprecated = '', $autoload = null)
{
    $GLOBALS['speedyspec_compat_options'][$option] = $value;
    return true;
}

function delete_option($option)
{
    unset($GLOBALS['speedyspec_compat_options'][$option]);
    return true;
}

function home_url($path = '', $scheme = null)
{
    return $GLOBALS['speedyspec_compat_home'] . ($path ? '/' . ltrim($path, '/') : '');
}

function site_url($path = '', $scheme = null)
{
    return $GLOBALS['speedyspec_compat_site'] . ($path ? '/' . ltrim($path, '/') : '');
}

function is_multisite()
{
    return false;
}

function is_main_site($site_id = null, $network_id = null)
{
    return true;
}

function get_post_types($args = array(), $output = 'names', $operator = 'and')
{
    return array();
}

function is_post_type_hierarchical($post_type)
{
    return false;
}

function is_post_type_viewable($post_type)
{
    return false;
}

function get_taxonomies($args = array(), $output = 'names', $operator = 'and')
{
    return array();
}

function is_admin()
{
    return false;
}

function __($text, $domain = 'default')
{
    return $text;
}

function _x($text, $context, $domain = 'default')
{
    return $text;
}

/**
 * The site language is English, so `WP_Rewrite::get_random_base()` switches to no other locale.
 */
function get_locale()
{
    return 'en_US';
}

function switch_to_locale($locale)
{
    return false;
}

function restore_previous_locale()
{
    return false;
}

function wp_die($message = '', $title = '', $args = array())
{
    throw new RuntimeException(is_string($message) ? $message : 'wp_die');
}

/**
 * The pages that exist: `about`, `parent` and `parent/child`, published.
 */
function get_page_by_path($page_path, $output = OBJECT, $post_type = 'page')
{
    return in_array($page_path, array('about', 'parent', 'parent/child'), true)
        ? (object) array('ID' => 2, 'post_name' => basename($page_path), 'post_status' => 'publish')
        : null;
}

function get_post_status_object($post_status)
{
    return (object) array('public' => true, 'protected' => false, 'private' => false, 'exclude_from_search' => false);
}

/**
 * `add_query_arg()` for the one form the rewrite code uses: an array of variables onto a URL with none.
 */
function add_query_arg(...$args)
{
    $query = array();
    foreach ($args[0] as $key => $value)
    {
        if (false !== $value && null !== $value)
        {
            $query[] = $key . '=' . $value;
        }
    }
    return $args[1] . '?' . implode('&', $query);
}

function wp_parse_args($args, $defaults = array())
{
    if (is_object($args))
    {
        $parsed_args = get_object_vars($args);
    }
    elseif (is_array($args))
    {
        $parsed_args =& $args;
    }
    else
    {
        parse_str((string) $args, $parsed_args);
    }
    return is_array($defaults) && $defaults ? array_merge($defaults, $parsed_args) : $parsed_args;
}

function trailingslashit($value)
{
    return rtrim($value, '/\\') . '/';
}

function esc_attr($text)
{
    return htmlspecialchars((string) $text, ENT_QUOTES, 'UTF-8', false);
}

if (!defined('OBJECT'))
{
    define('OBJECT', 'OBJECT');
}

/**
 * Sets the site up for a scenario: the permalink structure and other options, and the home and site URLs, then
 *  WordPress's `WP_Rewrite` and `WP` objects.
 */
function speedyspec_compat_configure($permalink_structure, $options = array(), $home = 'https://example.test', $site = null)
{
    $GLOBALS['speedyspec_compat_options'] = array_merge(
        array('permalink_structure' => $permalink_structure, 'rewrite_rules' => '', 'category_base' => '', 'tag_base' => ''),
        $options
    );
    $GLOBALS['speedyspec_compat_home'] = $home;
    $GLOBALS['speedyspec_compat_site'] = $site ?? $home;
    $GLOBALS['wp_rewrite'] = new WP_Rewrite();
    $GLOBALS['wp'] = new WP();
    return true;
}

/**
 * Registers the category, tag and post format taxonomies' rewrite tags and permastructs, as register_taxonomy()
 *  does for them.
 */
function speedyspec_compat_taxonomies()
{
    global $wp_rewrite;
    $category = get_option('category_base') ?: 'category';
    $tag = get_option('tag_base') ?: 'tag';
    add_rewrite_tag('%category%', '(.+?)', 'category_name=');
    add_rewrite_tag('%post_tag%', '([^/]+)', 'tag=');
    add_rewrite_tag('%post_format%', '([^/]+)', 'post_format=');
    add_permastruct('category', "$category/%category%", array('with_front' => true, 'ep_mask' => EP_CATEGORIES));
    add_permastruct('post_tag', "$tag/%post_tag%", array('with_front' => true, 'ep_mask' => EP_TAGS));
    add_permastruct('post_format', 'type/%post_format%', array('with_front' => true));
    return true;
}

/**
 * Resolves a request as WordPress's front controller would, and reports what `WP::parse_request` set.
 *
 * @return array
 *   The request, the matched rule and query, the query variables and whether permalink matching ran.
 */
function speedyspec_compat_request($request_uri, $path_info = null)
{
    $_SERVER['REQUEST_URI'] = $request_uri;
    $_SERVER['PHP_SELF'] = '/index.php';
    unset($_SERVER['PATH_INFO']);
    if (null !== $path_info)
    {
        $_SERVER['PATH_INFO'] = $path_info;
        $_SERVER['PHP_SELF'] = '/index.php' . $path_info;
    }
    $query = parse_url($request_uri, PHP_URL_QUERY);
    $_GET = array();
    if ($query)
    {
        parse_str($query, $_GET);
    }
    $_POST = array();
    $GLOBALS['wp'] = new WP();
    $GLOBALS['wp']->parse_request();
    return array(
        'request' => $GLOBALS['wp']->request,
        'matched_rule' => $GLOBALS['wp']->matched_rule,
        'matched_query' => $GLOBALS['wp']->matched_query,
        'query_vars' => $GLOBALS['wp']->query_vars,
        'did_permalink' => $GLOBALS['wp']->did_permalink,
    );
}

/**
 * Prepares each scenario once the driver is loaded: a plain site, `wp_loaded` done (so flushes save at once), and
 *  the state each step reports: `$wp_rewrite`'s public properties without the rules, and a digest of the stored rules
 *  (steps that matter return the rules themselves).
 */
function speedyspec_compat_area_setup(): void
{
    speedyspec_compat_configure('');
    do_action('wp_loaded');
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$state = static function (): array
    {
        $props = get_object_vars($GLOBALS['wp_rewrite']);
        unset($props['rules']);
        ksort($props);
        $stored = get_option('rewrite_rules');
        return array(
            'wp_rewrite' => $props,
            'rewrite_rules' => is_array($stored) ? array('count' => count($stored), 'md5' => md5(serialize($stored))) : $stored,
        );
    };
}
