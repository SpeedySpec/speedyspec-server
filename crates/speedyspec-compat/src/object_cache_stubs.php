declare(strict_types=1);

/**
 * Declares the WordPress functions the object cache calls, identically for WordPress's own cache and the port.
 *
 *  Notices are recorded as invocations, as the driver records `_deprecated_hook`.
 */

if (!function_exists('is_multisite'))
{
    function is_multisite()
    {
        return (bool) ($GLOBALS['speedyspec_compat_multisite'] ?? false);
    }
}

if (!function_exists('get_current_blog_id'))
{
    function get_current_blog_id()
    {
        return (int) ($GLOBALS['speedyspec_compat_blog_id'] ?? 1);
    }
}

if (!function_exists('__'))
{
    function __($text, $domain = 'default')
    {
        return $text;
    }
}

if (!function_exists('_doing_it_wrong'))
{
    function _doing_it_wrong($function_name, $message, $version)
    {
        SpeedySpecCompat::$invocations[] = [
            'fixture' => '_doing_it_wrong',
            'args' => SpeedySpecCompat::normalise([$function_name, $message, $version]),
        ];
    }
}

if (!function_exists('_deprecated_function'))
{
    function _deprecated_function($function_name, $version, $replacement = '')
    {
        SpeedySpecCompat::$invocations[] = [
            'fixture' => '_deprecated_function',
            'args' => SpeedySpecCompat::normalise([$function_name, $version, $replacement]),
        ];
    }
}

if (!function_exists('wp_suspend_cache_addition'))
{
    function wp_suspend_cache_addition($suspend = null)
    {
        static $_suspend = false;

        if (is_bool($suspend))
        {
            $_suspend = $suspend;
        }

        return $_suspend;
    }
}

if (!function_exists('wp_using_ext_object_cache'))
{
    function wp_using_ext_object_cache($using = null)
    {
        return false;
    }
}

/**
 * Reads a cache entry with its found flag, which a step cannot take by reference.
 */
function speedyspec_compat_cache_get_found($key, $group = '', $force = false)
{
    $found = null;
    $value = wp_cache_get($key, $group, $force, $found);
    return ['value' => $value, 'found' => $found];
}

function speedyspec_compat_object(array $props)
{
    return (object) $props;
}

function speedyspec_compat_props(object $o)
{
    return get_object_vars($o);
}

function speedyspec_compat_prop_set(object $o, string $name, mixed $value)
{
    $o->$name = $value;
}

function speedyspec_compat_new_cache()
{
    return new WP_Object_Cache();
}
