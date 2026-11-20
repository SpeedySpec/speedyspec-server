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

    function get_options($options)
    {
        return N\get_options(...func_get_args());
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
