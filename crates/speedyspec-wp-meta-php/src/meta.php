declare(strict_types=1);

namespace
{
    use SpeedySpec\WpMeta\Native as N;

    // meta.php (WordPress 7.2-RC2), native but get_meta_sql(), which stays WordPress's own: it needs WP_Meta_Query.
    function add_metadata($meta_type, $object_id, $meta_key, $meta_value, $unique = false)
    {
        return N\add_metadata(...func_get_args());
    }

    function update_metadata($meta_type, $object_id, $meta_key, $meta_value, $prev_value = '')
    {
        return N\update_metadata(...func_get_args());
    }

    function delete_metadata($meta_type, $object_id, $meta_key, $meta_value = '', $delete_all = false)
    {
        return N\delete_metadata(...func_get_args());
    }

    function get_metadata($meta_type, $object_id, $meta_key = '', $single = false)
    {
        return N\get_metadata(...func_get_args());
    }

    function get_metadata_raw($meta_type, $object_id, $meta_key = '', $single = false)
    {
        return N\get_metadata_raw(...func_get_args());
    }

    function get_metadata_default($meta_type, $object_id, $meta_key, $single = false)
    {
        return N\get_metadata_default(...func_get_args());
    }

    function metadata_exists($meta_type, $object_id, $meta_key)
    {
        return N\metadata_exists(...func_get_args());
    }

    function get_metadata_by_mid($meta_type, $meta_id)
    {
        return N\get_metadata_by_mid(...func_get_args());
    }

    function update_metadata_by_mid($meta_type, $meta_id, $meta_value, $meta_key = false)
    {
        return N\update_metadata_by_mid(...func_get_args());
    }

    function delete_metadata_by_mid($meta_type, $meta_id)
    {
        return N\delete_metadata_by_mid(...func_get_args());
    }

    function update_meta_cache($meta_type, $object_ids)
    {
        return N\update_meta_cache(...func_get_args());
    }

    /**
     * Retrieves the request's metadata lazy loader, made on first use.
     *
     * @return WP_Metadata_Lazyloader
     *   The one loader of the request.
     */
    function wp_metadata_lazyloader()
    {
        static $wp_metadata_lazyloader;
        if (null === $wp_metadata_lazyloader)
        {
            $wp_metadata_lazyloader = new WP_Metadata_Lazyloader();
        }
        return $wp_metadata_lazyloader;
    }

    function _get_meta_table($type)
    {
        return N\_get_meta_table(...func_get_args());
    }

    function is_protected_meta($meta_key, $meta_type = '')
    {
        return N\is_protected_meta(...func_get_args());
    }

    function sanitize_meta($meta_key, $meta_value, $object_type, $object_subtype = '')
    {
        return N\sanitize_meta(...func_get_args());
    }

    function register_meta($object_type, $meta_key, $args, $deprecated = null)
    {
        return N\register_meta(...func_get_args());
    }

    function filter_default_metadata($value, $object_id, $meta_key, $single, $meta_type)
    {
        return N\filter_default_metadata(...func_get_args());
    }

    function registered_meta_key_exists($object_type, $meta_key, $object_subtype = '')
    {
        return N\registered_meta_key_exists(...func_get_args());
    }

    function unregister_meta_key($object_type, $meta_key, $object_subtype = '')
    {
        return N\unregister_meta_key(...func_get_args());
    }

    function get_registered_meta_keys($object_type, $object_subtype = '')
    {
        return N\get_registered_meta_keys(...func_get_args());
    }

    function get_registered_metadata($object_type, $object_id, $meta_key = '')
    {
        return N\get_registered_metadata(...func_get_args());
    }

    function _wp_register_meta_args_allowed_list($args, $default_args)
    {
        return N\_wp_register_meta_args_allowed_list(...func_get_args());
    }

    function get_object_subtype($object_type, $object_id)
    {
        return N\get_object_subtype(...func_get_args());
    }
}

namespace SpeedySpec\WpMeta\Php
{
    /**
     * Answers get_object_subtype()'s switch for posts, terms, comments and users before its filter runs.
     *
     *  These read WordPress's own objects until the posts, taxonomy, comments and users seams bind Rust answers.
     *  The `'blog'` case is Rust's.
     *
     * @return string
     *   The subtype, or an empty string when the object is unknown.
     */
    function object_subtype($object_type, $object_id)
    {
        switch ($object_type)
        {
            case 'post':
                $post_type = \function_exists('get_post_type') ? \get_post_type($object_id) : false;
                return !empty($post_type) ? (string) $post_type : '';
            case 'term':
                $term = \function_exists('get_term') ? \get_term($object_id) : null;
                return $term instanceof \WP_Term ? (string) $term->taxonomy : '';
            case 'comment':
                return \function_exists('get_comment') && \get_comment($object_id) ? 'comment' : '';
            case 'user':
                return \function_exists('get_userdata') && \get_userdata($object_id) ? 'user' : '';
        }
        return '';
    }

    /**
     * Reads a meta table a plugin declared as a dynamic property of `$wpdb`, which native wpdb does not hold.
     *
     * @return string
     *   The table name, or an empty string when none is declared.
     */
    function table($name)
    {
        global $wpdb;
        if (!\is_object($wpdb))
        {
            return '';
        }
        $properties = \get_object_vars($wpdb);
        return isset($properties[$name]) && \is_string($properties[$name]) ? $properties[$name] : '';
    }

    /**
     * Makes the object `$wpdb->get_row()` gives for a meta row.
     *
     * @param array<string, mixed> $fields
     *   The row's columns, in table order.
     * @return object
     *   The row as a standard object.
     */
    function row(array $fields)
    {
        return (object) $fields;
    }

    /**
     * Makes `$wp_meta_keys` an array, as register_meta() does first.
     */
    function keys_init()
    {
        global $wp_meta_keys;
        if (!\is_array($wp_meta_keys))
        {
            $wp_meta_keys = array();
        }
    }

    /**
     * Stores a registered key's args in `$wp_meta_keys`, with PHP's own key handling.
     */
    function keys_put($object_type, $object_subtype, $meta_key, $args)
    {
        global $wp_meta_keys;
        $wp_meta_keys[$object_type][$object_subtype][$meta_key] = $args;
    }

    /**
     * Removes a registered key from `$wp_meta_keys`, then the subtype and the type when they are left empty.
     */
    function keys_remove($object_type, $object_subtype, $meta_key)
    {
        global $wp_meta_keys;
        unset($wp_meta_keys[$object_type][$object_subtype][$meta_key]);
        if (empty($wp_meta_keys[$object_type][$object_subtype]))
        {
            unset($wp_meta_keys[$object_type][$object_subtype]);
        }
        if (empty($wp_meta_keys[$object_type]))
        {
            unset($wp_meta_keys[$object_type]);
        }
    }

    /**
     * Reads the keys registered for a type and subtype.
     *
     * @return array<string, array<string, mixed>>
     *   The keys' args by key, as `$wp_meta_keys` holds them, or an empty array.
     */
    function keys_get($object_type, $object_subtype)
    {
        global $wp_meta_keys;
        if (!\is_array($wp_meta_keys) || !isset($wp_meta_keys[$object_type])
            || !isset($wp_meta_keys[$object_type][$object_subtype]))
        {
            return array();
        }
        return $wp_meta_keys[$object_type][$object_subtype];
    }
}
