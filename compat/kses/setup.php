<?php

declare(strict_types=1);

/**
 * Sets up the kses scenarios the same way on both sides: WordPress's own load, compat, UTF-8, formatting, functions
 * and kses files, and the helpers the scenarios name.
 *
 * `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 * loaded. The recorder also loads WordPress's hooks (`plugin.php`); the port's side does not (its hooks are native),
 * and its release copy's kses.php and functions.php have the ported functions removed, so kses.php runs only for its
 * globals. No database is used: the few options these files read come from `pre_option`.
 */

define('ABSPATH', SPEEDYSPEC_COMPAT_WP_ROOT);
define('WPINC', 'wp-includes');
define('WP_CONTENT_DIR', sys_get_temp_dir() . '/speedyspec-compat-kses-content');
define('WP_CONTENT_URL', 'http://example.org/wp-content');
// wp_initial_constants()'s default: _deprecated_function(), which wp_kses_html_error() calls, reads it.
define('WP_DEBUG', false);

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
}

$GLOBALS['speedyspec_kses_options'] = array(
    'blog_charset'                  => 'UTF-8',
    'siteurl'                       => 'http://example.org',
    'home'                          => 'http://example.org',
    'upload_path'                   => '',
    'upload_url_path'               => '',
    'uploads_use_yearmonth_folders' => '',
);

/**
 * The options the loaded files read, as a fresh install has them.
 */
function speedyspec_kses_option($pre, $option)
{
    return $GLOBALS['speedyspec_kses_options'][$option] ?? '';
}

/**
 * `wp_load_alloptions()` without a database.
 */
function speedyspec_kses_alloptions()
{
    return $GLOBALS['speedyspec_kses_options'];
}

add_filter('pre_option', 'speedyspec_kses_option', 10, 2);
add_filter('pre_wp_load_alloptions', 'speedyspec_kses_alloptions');

/**
 * `current_user_can()` for the scenarios: `$speedyspec_kses_can` lists the capabilities the current user has.
 */
function current_user_can($capability, ...$args)
{
    return in_array($capability, $GLOBALS['speedyspec_kses_can'] ?? array(), true);
}

require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/class-wp-token-map.php';
foreach (
    array(
        'html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span',
        'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder',
        'class-wp-html-tag-processor',
    ) as $speedyspec_file
)
{
    require ABSPATH . WPINC . "/html-api/{$speedyspec_file}.php";
}
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/kses.php';

// WordPress 7.2 moved wp_kses_html_error() from kses.php to deprecated.php. Define that copy alone, on both sides, as
// the kses testkit's oracle does: the rest of deprecated.php is not part of this area.
if (!function_exists('wp_kses_html_error'))
{
    preg_match(
        '/^function wp_kses_html_error\(.*?^}\n/ms',
        (string) file_get_contents(ABSPATH . WPINC . '/deprecated.php'),
        $speedyspec_function
    ) || throw new RuntimeException('wp_kses_html_error() is not in deprecated.php');
    eval($speedyspec_function[0]);
}

// Filters the scenarios add by name: each changes a result, and a recording fixture beside it counts the runs.

/** Adds `x-tag` (with `data-x`) to any allowlist. */
function speedyspec_kses_add_x_tag($tags)
{
    $tags = is_array($tags) ? $tags : array();
    $tags['x-tag'] = array('data-x' => true);
    return $tags;
}

/** Allows `input` in an allowlist, so the post context adds `form`. */
function speedyspec_kses_add_input($tags)
{
    $tags = is_array($tags) ? $tags : array();
    $tags['input'] = array('type' => true, 'name' => true);
    return $tags;
}

/** Treats `data-href` as a URL attribute. */
function speedyspec_kses_uri_data_href($uris)
{
    $uris[] = 'data-href';
    return $uris;
}

/** Allows the `cursor` property. */
function speedyspec_kses_style_cursor($properties)
{
    $properties[] = 'cursor';
    return $properties;
}

/** Narrows the protocols to https. */
function speedyspec_kses_only_https($protocols)
{
    return array('https');
}

/** Allows any CSS, forbidden characters included. */
function speedyspec_kses_allow_css($allow, $css)
{
    return true;
}

/** Refuses any CSS. */
function speedyspec_kses_refuse_css($allow, $css)
{
    return false;
}

/** A value callback: digits only. */
function speedyspec_kses_digits($value)
{
    return 1 === preg_match('/^[0-9]+$/', (string) $value);
}

/**
 * Once the driver is loaded: a large array a hook receives (a whole allowlist, run once per tag) is reported as its
 * size and a digest of its contents, so the fixture stays small while every run is still compared.
 *
 * Each step reports how many times every filter and action has run (`$wp_filters`, `$wp_actions`) in place of the
 * whole hook globals: reading `$wp_filter`'s `WP_Hook` objects from the port's proxy can hang (speedyspec-php copies
 * an object's hash-chain link into the array it builds, which loops on a key collision; kses_init_filters()'s five
 * hooks collide). The counts still compare every hook run on both sides.
 */
function speedyspec_compat_area_setup(): void
{
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$state = static function (): array
    {
        $counts = static function ($global): array
        {
            $out = array();
            foreach ($GLOBALS[$global] ?? array() as $hook => $count)
            {
                $out[] = array($hook, $count);
            }
            sort($out);
            return $out;
        };
        return array('filters' => $counts('wp_filters'), 'actions' => $counts('wp_actions'));
    };
    SpeedySpecCompat::$arguments = static function (array $args): array
    {
        foreach ($args as $key => $arg)
        {
            if (is_array($arg) && count($arg) > 16)
            {
                $args[$key] = array('array of' => count($arg), 'md5' => 'md5:' . md5(var_export($arg, true)));
            }
        }
        return $args;
    };
}
