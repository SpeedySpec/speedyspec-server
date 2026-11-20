declare(strict_types=1);

namespace
{
    use SpeedySpec\WpObjectCache\Native as N;

    use function SpeedySpec\WpObjectCache\clone_objects;

    /**
     * Sets up the object cache, refusing an object-cache.php drop-in.
     *
     *  SpeedySpec's object cache is native; a site gets persistence by configuring a Caching Store instead.
     */
    function wp_cache_init()
    {
        if (defined('WP_CONTENT_DIR') && file_exists(WP_CONTENT_DIR . '/object-cache.php'))
        {
            throw new Error(
                'The object-cache.php drop-in (' . WP_CONTENT_DIR . '/object-cache.php) is not supported: SpeedySpec\'s'
                . ' object cache is native. Remove the file; persistence comes from configuring a Caching Store.'
            );
        }
        $GLOBALS['wp_object_cache'] = new WP_Object_Cache();
        N\select($GLOBALS['wp_object_cache']);
    }

    function wp_cache_add($key, $data, $group = '', $expire = 0)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->add($key, $data, $group, (int) $expire);
        }
        if (is_object($data))
        {
            $data = clone $data;
        }
        return N\cache_add(spl_object_id($wp_object_cache), $key, $data, $group, (int) $expire);
    }

    function wp_cache_add_multiple(array $data, $group = '', $expire = 0)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->add_multiple($data, $group, (int) $expire);
        }
        return N\cache_add_multiple(spl_object_id($wp_object_cache), clone_objects($data), $group, (int) $expire);
    }

    function wp_cache_replace($key, $data, $group = '', $expire = 0)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->replace($key, $data, $group, (int) $expire);
        }
        if (is_object($data))
        {
            $data = clone $data;
        }
        return N\cache_replace(spl_object_id($wp_object_cache), $key, $data, $group, (int) $expire);
    }

    function wp_cache_set($key, $data, $group = '', $expire = 0)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->set($key, $data, $group, (int) $expire);
        }
        if (is_object($data))
        {
            $data = clone $data;
        }
        return N\cache_set(spl_object_id($wp_object_cache), $key, $data, $group, (int) $expire);
    }

    function wp_cache_set_multiple(array $data, $group = '', $expire = 0)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->set_multiple($data, $group, (int) $expire);
        }
        return N\cache_set_multiple(spl_object_id($wp_object_cache), clone_objects($data), $group, (int) $expire);
    }

    function wp_cache_get_multiple($keys, $group = '', $force = false)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->get_multiple($keys, $group, $force);
        }
        return clone_objects(N\cache_get_multiple(spl_object_id($wp_object_cache), $keys, $group, $force));
    }

    function wp_cache_delete($key, $group = '')
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->delete($key, $group);
        }
        return N\cache_delete(spl_object_id($wp_object_cache), $key, $group);
    }

    function wp_cache_delete_multiple(array $keys, $group = '')
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->delete_multiple($keys, $group);
        }
        return N\cache_delete_multiple(spl_object_id($wp_object_cache), $keys, $group);
    }

    function wp_cache_incr($key, $offset = 1, $group = '')
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->incr($key, $offset, $group);
        }
        return N\cache_incr(spl_object_id($wp_object_cache), $key, $offset, $group);
    }

    function wp_cache_decr($key, $offset = 1, $group = '')
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->decr($key, $offset, $group);
        }
        return N\cache_decr(spl_object_id($wp_object_cache), $key, $offset, $group);
    }

    function wp_cache_flush()
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->flush();
        }
        return N\cache_flush(spl_object_id($wp_object_cache));
    }

    function wp_cache_flush_group($group)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->flush_group($group);
        }
        return N\cache_flush_group(spl_object_id($wp_object_cache), $group);
    }

    function wp_cache_add_global_groups($groups)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->add_global_groups($groups);
        }
        return N\cache_add_global_groups(spl_object_id($wp_object_cache), $groups);
    }

    function wp_cache_switch_to_blog($blog_id)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->switch_to_blog($blog_id);
        }
        return N\cache_switch_to_blog(spl_object_id($wp_object_cache), $blog_id);
    }

    function wp_cache_get($key, $group = '', $force = false, &$found = null)
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->get($key, $group, $force, $found);
        }
        $value = N\cache_get(spl_object_id($wp_object_cache), $key, $group, $force);
        $found = N\found();
        return is_object($value) ? clone $value : $value;
    }

    function wp_cache_flush_runtime()
    {
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return wp_cache_flush();
        }
        // WordPress flushes everything because its cache has no second level; the shared one stays.
        return N\cache_flush_runtime(spl_object_id($wp_object_cache));
    }

    function wp_cache_supports($feature)
    {
        return N\cache_supports($feature);
    }

    function wp_cache_close()
    {
        return true;
    }

    function wp_cache_add_non_persistent_groups($groups)
    {
        global $wp_object_cache;
        if ($wp_object_cache instanceof WP_Object_Cache && get_class($wp_object_cache) === 'WP_Object_Cache')
        {
            N\cache_add_non_persistent_groups(spl_object_id($wp_object_cache), $groups);
        }
    }

    function wp_cache_reset()
    {
        _deprecated_function(__FUNCTION__, '3.5.0', 'wp_cache_switch_to_blog()');
        global $wp_object_cache;
        if (get_class($wp_object_cache) !== 'WP_Object_Cache')
        {
            return $wp_object_cache->reset();
        }
        N\cache_reset(spl_object_id($wp_object_cache));
    }

    /**
     * Holds the request's cached data, native over SpeedySpec's Rust object cache.
     *
     *  The methods keep WordPress's names, parameters and defaults; the values and counters live in Rust, and
     *  any property the class does not keep there is an ordinary dynamic property.
     */
    #[AllowDynamicProperties]
    class WP_Object_Cache
    {
        public function __construct()
        {
            N\object_construct($this);
        }

        public function __destruct()
        {
            N\object_destruct($this);
        }

        public function __get($name)
        {
            $found = N\object_prop_get($this, (string) $name);
            // Not one of WordPress's properties: a dynamic one `__set()` made, as WordPress reads it.
            return 'ok' === $found[0] ? $found[1] : $this->$name;
        }

        public function __set($name, $value)
        {
            if (!N\object_prop_set($this, (string) $name, $value))
            {
                $this->$name = $value;
            }
        }

        public function __isset($name)
        {
            $found = N\object_prop_get($this, (string) $name);
            return 'ok' === $found[0] ? null !== $found[1] : isset($this->$name);
        }

        public function __unset($name)
        {
            if (!N\object_prop_set($this, (string) $name, null))
            {
                unset($this->$name);
            }
        }

        public function add($key, $data, $group = 'default', $expire = 0)
        {
            if (is_object($data))
            {
                $data = clone $data;
            }
            return N\object_add($this, $key, $data, $group, (int) $expire);
        }

        public function add_multiple(array $data, $group = '', $expire = 0)
        {
            return N\object_add_multiple($this, clone_objects($data), $group, (int) $expire);
        }

        public function replace($key, $data, $group = 'default', $expire = 0)
        {
            if (is_object($data))
            {
                $data = clone $data;
            }
            return N\object_replace($this, $key, $data, $group, (int) $expire);
        }

        public function set($key, $data, $group = 'default', $expire = 0)
        {
            if (is_object($data))
            {
                $data = clone $data;
            }
            return N\object_set($this, $key, $data, $group, (int) $expire);
        }

        public function set_multiple(array $data, $group = '', $expire = 0)
        {
            return N\object_set_multiple($this, clone_objects($data), $group, (int) $expire);
        }

        public function get($key, $group = 'default', $force = false, &$found = null)
        {
            $value = N\object_get($this, $key, $group, $force);
            $found = N\found();
            return is_object($value) ? clone $value : $value;
        }

        public function get_multiple($keys, $group = 'default', $force = false)
        {
            return clone_objects(N\object_get_multiple($this, $keys, $group, $force));
        }

        public function delete($key, $group = 'default', $deprecated = false)
        {
            return N\object_delete($this, $key, $group);
        }

        public function delete_multiple(array $keys, $group = '')
        {
            return N\object_delete_multiple($this, $keys, $group);
        }

        public function incr($key, $offset = 1, $group = 'default')
        {
            return N\object_incr($this, $key, $offset, $group);
        }

        public function decr($key, $offset = 1, $group = 'default')
        {
            return N\object_decr($this, $key, $offset, $group);
        }

        public function flush()
        {
            return N\object_flush($this);
        }

        public function flush_group($group)
        {
            return N\object_flush_group($this, $group);
        }

        public function add_global_groups($groups)
        {
            N\object_add_global_groups($this, $groups);
        }

        public function switch_to_blog($blog_id)
        {
            N\object_switch_to_blog($this, $blog_id);
        }

        public function reset()
        {
            N\object_reset($this);
        }

        public function stats()
        {
            echo '<p>';
            echo "<strong>Cache Hits:</strong> {$this->cache_hits}<br />";
            echo "<strong>Cache Misses:</strong> {$this->cache_misses}<br />";
            echo '</p>';
            echo '<ul>';
            foreach ($this->cache as $group => $cache)
            {
                $size = number_format(strlen(serialize($cache)) / (defined('KB_IN_BYTES') ? KB_IN_BYTES : 1024), 2);
                $name = function_exists('esc_html') ? esc_html($group) : htmlspecialchars((string) $group, ENT_QUOTES);
                echo '<li><strong>Group:</strong> ' . $name . ' - ( ' . $size . 'k )</li>';
            }
            echo '</ul>';
        }
    }
}

namespace SpeedySpec\WpObjectCache
{
    /**
     * Copies each top-level object of a list, as WordPress's `add()`, `set()` and `get()` do for one value.
     *
     *  WordPress's `*_multiple()` methods loop over those, so a caller's object is never the cached one.
     *
     * @param array<int|string, mixed> $values
     *   Values by cache key.
     * @return array<int|string, mixed>
     *   The same values, each object a clone.
     */
    function clone_objects(array $values): array
    {
        foreach ($values as $key => $value)
        {
            if (is_object($value))
            {
                $values[$key] = clone $value;
            }
        }
        return $values;
    }
}
