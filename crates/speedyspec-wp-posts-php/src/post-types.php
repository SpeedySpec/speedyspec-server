declare(strict_types=1);

namespace
{
    use SpeedySpec\WpPosts\Native as N;

    // post.php (WordPress 7.2-RC2): the post type and post status registries, native.

    function create_initial_post_types()
    {
        N\create_initial_post_types();
    }

    function register_post_status($post_status, $args = array())
    {
        return N\register_post_status($post_status, $args);
    }

    function get_post_status_object($post_status)
    {
        return N\get_post_status_object($post_status);
    }

    function get_post_stati($args = array(), $output = 'names', $operator = 'and')
    {
        return N\get_post_stati($args, $output, $operator);
    }

    function is_post_type_hierarchical($post_type)
    {
        return N\is_post_type_hierarchical($post_type);
    }

    function post_type_exists($post_type)
    {
        return N\post_type_exists($post_type);
    }

    function get_post_type_object($post_type)
    {
        return N\get_post_type_object($post_type);
    }

    function get_post_types($args = array(), $output = 'names', $operator = 'and')
    {
        return N\get_post_types($args, $output, $operator);
    }

    function register_post_type($post_type, $args = array())
    {
        return N\register_post_type($post_type, $args);
    }

    function unregister_post_type($post_type)
    {
        return N\unregister_post_type($post_type);
    }

    function get_post_type_capabilities($args)
    {
        return N\get_post_type_capabilities($args);
    }

    function get_post_type_labels($post_type_object)
    {
        return N\get_post_type_labels($post_type_object);
    }

    function _get_custom_object_labels($data_object, $nohier_vs_hier_defaults)
    {
        return N\_get_custom_object_labels($data_object, $nohier_vs_hier_defaults);
    }

    function add_post_type_support($post_type, $feature, ...$args)
    {
        N\add_post_type_support($post_type, $feature, ...$args);
    }

    function remove_post_type_support($post_type, $feature)
    {
        N\remove_post_type_support($post_type, $feature);
    }

    function get_all_post_type_supports($post_type)
    {
        return N\get_all_post_type_supports($post_type);
    }

    function post_type_supports($post_type, $feature)
    {
        return N\post_type_supports($post_type, $feature);
    }

    function get_post_types_by_support($feature, $operator = 'and')
    {
        return N\get_post_types_by_support($feature, $operator);
    }

    function is_post_type_viewable($post_type)
    {
        return N\is_post_type_viewable($post_type);
    }

    function is_post_status_viewable($post_status)
    {
        return N\is_post_status_viewable($post_status);
    }
}

/**
 * The writes the native functions leave to PHP, so the globals keep PHP's own key handling and references, and the
 * objects WordPress's classes make.
 */
namespace SpeedySpec\WpPosts\Php
{
    /**
     * Casts properties to a standard object.
     */
    function object(array $properties)
    {
        return (object) $properties;
    }

    /**
     * Makes the `WP_Error` a registry function returns.
     */
    function error($code, $message)
    {
        return new \WP_Error($code, $message);
    }

    /**
     * Makes `$wp_post_types` an array, as register_post_type() does first.
     */
    function types_init()
    {
        global $wp_post_types;
        if (!\is_array($wp_post_types))
        {
            $wp_post_types = array();
        }
    }

    /**
     * Makes `$wp_post_statuses` an array, as register_post_status() does first.
     */
    function statuses_init()
    {
        global $wp_post_statuses;
        if (!\is_array($wp_post_statuses))
        {
            $wp_post_statuses = array();
        }
    }

    /**
     * Makes a `WP_Post_Type` with the resolved properties, without its constructor (the native side ran set_props()'
     * rules): a declared property the resolution left out (`supports`, after add_supports()) is unset.
     *
     * @param array<string, mixed> $properties
     *   The properties, in `get_object_vars()` order.
     */
    function post_type_object(array $properties)
    {
        $object = (new \ReflectionClass('WP_Post_Type'))->newInstanceWithoutConstructor();
        foreach (\array_keys(\get_object_vars($object)) as $name)
        {
            if (!\array_key_exists($name, $properties))
            {
                unset($object->$name);
            }
        }
        foreach ($properties as $name => $value)
        {
            $object->$name = $value;
        }
        return $object;
    }

    /**
     * Stores a post type object in `$wp_post_types`.
     */
    function store_post_type($post_type, $post_type_object)
    {
        global $wp_post_types;
        $wp_post_types[$post_type] = $post_type_object;
    }

    /**
     * Removes a post type from `$wp_post_types`.
     */
    function forget_post_type($post_type)
    {
        global $wp_post_types;
        unset($wp_post_types[$post_type]);
    }

    /**
     * Stores a status's properties as the object register_post_status() returns.
     */
    function store_status($post_status, array $properties)
    {
        global $wp_post_statuses;
        $status = (object) $properties;
        $wp_post_statuses[$post_status] = $status;
        return $status;
    }

    /**
     * Writes add_post_type_support()'s features: each `true`, or the args list when there are args.
     */
    function add_support($post_type, $features, array $args)
    {
        global $_wp_post_type_features;
        foreach ((array) $features as $feature)
        {
            $_wp_post_type_features[$post_type][$feature] = $args ? $args : true;
        }
    }

    /**
     * Compares two values as `WP_List_Util::filter()` does (`==`), for the objects the native side holds as markers.
     */
    function loose_equals($a, $b)
    {
        return $a == $b;
    }

    /**
     * remove_post_type_support()'s write.
     */
    function remove_support($post_type, $feature)
    {
        global $_wp_post_type_features;
        unset($_wp_post_type_features[$post_type][$feature]);
    }

    /**
     * WP_Post_Type::remove_supports()' write.
     */
    function remove_supports($post_type)
    {
        global $_wp_post_type_features;
        unset($_wp_post_type_features[$post_type]);
    }

    /**
     * `$post_type_meta_caps[ $custom ] = $core` for each pair.
     *
     * @param list<array{0: string, 1: string}> $pairs
     */
    function meta_caps_add(array $pairs)
    {
        global $post_type_meta_caps;
        foreach ($pairs as $pair)
        {
            $post_type_meta_caps[$pair[0]] = $pair[1];
        }
    }

    /**
     * Replaces `$post_type_meta_caps` with the pairs, as unregister_post_type() rebuilds it.
     *
     * @param list<array{0: string, 1: string}> $pairs
     */
    function meta_caps_set(array $pairs)
    {
        global $post_type_meta_caps;
        $post_type_meta_caps = array();
        meta_caps_add($pairs);
    }

    /**
     * `$object->$name = $value`.
     */
    function set_prop($object, $name, $value)
    {
        $object->$name = $value;
    }
}
