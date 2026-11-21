declare(strict_types=1);

namespace
{
    use SpeedySpec\WpOptions\Native as N;

    /**
     * Retrieves an option value based on an option name.
     *
     *  Every argument goes on as given: the number passed decides `$passed_default` for `default_option_{$option}`.
     */
    function get_option($option, $default_value = false)
    {
        return N\get_option(...func_get_args());
    }

    /**
     * Retrieves multiple options, as WordPress's own body does it.
     *
     *  The array is built here in PHP: a value a PHP `option_{$option}` filter returned must not be written into an
     *  array from Rust (speedyspec-php's `convert::write` copies a whole zval into a hash bucket, chain link
     *  included, which corrupts the array).
     */
    function get_options($options)
    {
        wp_prime_option_caches($options);
        $result = array();
        foreach ($options as $option)
        {
            $result[$option] = get_option($option);
        }
        return $result;
    }

    function add_option($option, $value = '', $deprecated = '', $autoload = null)
    {
        return N\add_option(...func_get_args());
    }

    function update_option($option, $value, $autoload = null)
    {
        return N\update_option(...func_get_args());
    }

    function delete_option($option)
    {
        return N\delete_option(...func_get_args());
    }

    function wp_load_alloptions($force_cache = false)
    {
        return N\wp_load_alloptions(...func_get_args());
    }

    function wp_prime_option_caches($options)
    {
        N\wp_prime_option_caches(...func_get_args());
    }

    function wp_prime_option_caches_by_group($option_group)
    {
        N\wp_prime_option_caches_by_group(...func_get_args());
    }

    function wp_set_option_autoload_values(array $options)
    {
        return N\wp_set_option_autoload_values(...func_get_args());
    }

    function wp_set_options_autoload(array $options, $autoload)
    {
        return N\wp_set_options_autoload(...func_get_args());
    }

    function wp_set_option_autoload($option, $autoload)
    {
        return N\wp_set_option_autoload(...func_get_args());
    }

    function wp_determine_option_autoload_value($option, $value, $serialized_value, $autoload)
    {
        return N\wp_determine_option_autoload_value(...func_get_args());
    }

    function wp_filter_default_autoload_value_via_option_size($autoload, $option, $value, $serialized_value)
    {
        return N\wp_filter_default_autoload_value_via_option_size(...func_get_args());
    }

    function wp_autoload_values_to_autoload()
    {
        return N\wp_autoload_values_to_autoload();
    }

    function wp_protect_special_option($option)
    {
        N\wp_protect_special_option(...func_get_args());
    }

    /**
     * Prints an option value after escaping it for an HTML attribute, as WordPress's own does.
     */
    function form_option($option)
    {
        echo esc_attr(get_option($option));
    }

    // Transients (option.php 1381–1690, 2512–2735), native.
    function get_transient($transient)
    {
        return N\get_transient(...func_get_args());
    }

    function set_transient($transient, $value, $expiration = 0)
    {
        return N\set_transient(...func_get_args());
    }

    function delete_transient($transient)
    {
        return N\delete_transient(...func_get_args());
    }

    function delete_expired_transients($force_db = false)
    {
        N\delete_expired_transients(...func_get_args());
    }

    function get_site_transient($transient)
    {
        return N\get_site_transient(...func_get_args());
    }

    function set_site_transient($transient, $value, $expiration = 0)
    {
        return N\set_site_transient(...func_get_args());
    }

    function delete_site_transient($transient)
    {
        return N\delete_site_transient(...func_get_args());
    }

    // Site and network options (option.php 677–846, 1935–2500), native.
    function get_site_option($option, $default_value = false, $deprecated = true)
    {
        return N\get_site_option($option, $default_value);
    }

    function add_site_option($option, $value)
    {
        return N\add_site_option(...func_get_args());
    }

    function update_site_option($option, $value)
    {
        return N\update_site_option(...func_get_args());
    }

    function delete_site_option($option)
    {
        return N\delete_site_option(...func_get_args());
    }

    function get_network_option($network_id, $option, $default_value = false)
    {
        return N\get_network_option(...func_get_args());
    }

    function add_network_option($network_id, $option, $value)
    {
        return N\add_network_option(...func_get_args());
    }

    function update_network_option($network_id, $option, $value)
    {
        return N\update_network_option(...func_get_args());
    }

    function delete_network_option($network_id, $option)
    {
        return N\delete_network_option(...func_get_args());
    }

    function wp_prime_site_option_caches(array $options)
    {
        N\wp_prime_site_option_caches(...func_get_args());
    }

    function wp_prime_network_option_caches($network_id, array $options)
    {
        N\wp_prime_network_option_caches(...func_get_args());
    }

    function wp_load_core_site_options($network_id = null)
    {
        N\wp_load_core_site_options(...func_get_args());
    }

    // Registered settings (option.php 2997–3256), native over $wp_registered_settings and $new_allowed_options.
    function register_setting($option_group, $option_name, $args = array())
    {
        N\register_setting(...func_get_args());
    }

    function unregister_setting($option_group, $option_name, $deprecated = '')
    {
        N\unregister_setting(...func_get_args());
    }

    function get_registered_settings()
    {
        return N\get_registered_settings();
    }

    function filter_default_option($default_value, $option, $passed_default)
    {
        return N\filter_default_option(...func_get_args());
    }

    // `$new_whitelist_options` stays a reference to `$new_allowed_options`, as register_setting() makes it.
    if (!isset($GLOBALS['new_allowed_options']))
    {
        $GLOBALS['new_allowed_options'] = array();
    }
    $GLOBALS['new_whitelist_options'] = &$GLOBALS['new_allowed_options'];
}

namespace SpeedySpec\WpOptions
{
    /**
     * Copies an object, as `add_option()` and `update_option()` do before sanitizing.
     *
     * @return mixed
     *   A clone of an object; any other value as given.
     */
    function clone_value($value)
    {
        return is_object($value) ? clone $value : $value;
    }

    /**
     * Reads `register_setting()`'s options for a group.
     *
     * @return array|null
     *   The group's option names, or null when the group is not registered.
     */
    function allowed_options($group)
    {
        return $GLOBALS['new_allowed_options'][$group] ?? null;
    }

    /**
     * Unserializes as `maybe_unserialize()` does once the data is known to be serialized.
     *
     * @return mixed
     *   The value, or false when it does not unserialize.
     */
    function unserialize_value($data)
    {
        return @unserialize(trim($data));
    }
}

namespace SpeedySpec\WpOptions\Php
{
    /**
     * Stores a setting's args in `$wp_registered_settings`.
     */
    function settings_put($name, $args)
    {
        global $wp_registered_settings;
        if (!is_array($wp_registered_settings))
        {
            $wp_registered_settings = array();
        }
        $wp_registered_settings[$name] = $args;
    }

    /**
     * Removes a setting from `$wp_registered_settings`.
     */
    function settings_remove($name)
    {
        global $wp_registered_settings;
        unset($wp_registered_settings[$name]);
    }

    /**
     * `$new_allowed_options[ $group ][] = $name`, keeping `$new_whitelist_options` a reference to it.
     */
    function settings_allow($group, $name)
    {
        global $new_allowed_options;
        $GLOBALS['new_whitelist_options'] = &$new_allowed_options;
        $new_allowed_options[$group][] = $name;
    }

    /**
     * Removes `$name` from `$new_allowed_options[ $group ]` as unregister_setting() does: PHP's index holes stay.
     */
    function settings_disallow($group, $name)
    {
        global $new_allowed_options;
        $GLOBALS['new_whitelist_options'] = &$new_allowed_options;
        $pos = false;
        if (isset($new_allowed_options[$group]))
        {
            $pos = array_search($name, (array) $new_allowed_options[$group], true);
        }
        if (false !== $pos)
        {
            unset($new_allowed_options[$group][$pos]);
        }
    }
}
