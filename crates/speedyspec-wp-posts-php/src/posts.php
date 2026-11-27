declare(strict_types=1);

namespace
{
    use SpeedySpec\WpPosts\Native as N;

    // post.php (WordPress 7.2-RC2): reading posts and the post caches, native. The meta wrappers, the WP_Query
    // callers, update_post_author_caches() and the attachment readers stay WordPress's own.

    function is_sticky($post_id = 0)
    {
        return N\is_sticky($post_id);
    }

    function sanitize_post($post, $context = 'display')
    {
        return N\sanitize_post($post, $context);
    }

    function sanitize_post_field($field, $value, $post_id, $context = 'display')
    {
        return N\sanitize_post_field($field, $value, $post_id, $context);
    }

    function get_post($post = null, $output = OBJECT, $filter = 'raw')
    {
        return N\get_post($post, $output, $filter);
    }

    function get_post_ancestors($post)
    {
        return N\get_post_ancestors($post);
    }

    function get_post_field($field, $post = null, $context = 'display')
    {
        return N\get_post_field($field, $post, $context);
    }

    function get_post_mime_type($post = null)
    {
        return N\get_post_mime_type($post);
    }

    function get_post_status($post = null)
    {
        return N\get_post_status($post);
    }

    function get_post_type($post = null)
    {
        return N\get_post_type($post);
    }

    function _count_posts_cache_key($type = 'post', $perm = '')
    {
        return N\_count_posts_cache_key($type, $perm);
    }

    function wp_count_posts($type = 'post', $perm = '')
    {
        return N\wp_count_posts($type, $perm);
    }

    function wp_count_attachments($mime_type = '')
    {
        return N\wp_count_attachments($mime_type);
    }

    function get_enclosed($post_id)
    {
        return N\get_enclosed($post_id);
    }

    function get_pung($post)
    {
        return N\get_pung($post);
    }

    function get_to_ping($post)
    {
        return N\get_to_ping($post);
    }

    function get_all_page_ids()
    {
        return N\get_all_page_ids();
    }

    function get_page($page, $output = OBJECT, $filter = 'raw')
    {
        return N\get_page($page, $output, $filter);
    }

    function get_page_by_path($page_path, $output = OBJECT, $post_type = 'page')
    {
        return N\get_page_by_path($page_path, $output, $post_type);
    }

    function get_page_children($page_id, $pages)
    {
        return N\get_page_children($page_id, $pages);
    }

    function get_page_hierarchy(&$pages, $page_id = 0)
    {
        return N\get_page_hierarchy($pages, $page_id);
    }

    function _page_traverse_name($page_id, &$children, &$result)
    {
        $result = N\_page_traverse_name($page_id, $children, $result);
    }

    function get_page_uri($page = 0)
    {
        return N\get_page_uri($page);
    }

    function get_lastpostdate($timezone = 'server', $post_type = 'any')
    {
        return N\get_lastpostdate($timezone, $post_type);
    }

    function get_lastpostmodified($timezone = 'server', $post_type = 'any')
    {
        return N\get_lastpostmodified($timezone, $post_type);
    }

    function _get_last_post_time($timezone, $field, $post_type = 'any')
    {
        return N\_get_last_post_time($timezone, $field, $post_type);
    }

    function update_post_cache(&$posts)
    {
        N\update_post_cache($posts);
    }

    function clean_post_cache($post)
    {
        N\clean_post_cache($post);
    }

    function update_post_caches(&$posts, $post_type = 'post', $update_term_cache = true, $update_meta_cache = true)
    {
        N\update_post_caches($posts, $post_type, $update_term_cache, $update_meta_cache);
    }

    function update_post_parent_caches($posts)
    {
        N\update_post_parent_caches($posts);
    }

    function update_postmeta_cache($post_ids)
    {
        return N\update_postmeta_cache($post_ids);
    }

    function wp_get_post_parent_id($post = null)
    {
        return N\wp_get_post_parent_id($post);
    }

    function _prime_post_caches($ids, $update_term_cache = true, $update_meta_cache = true)
    {
        N\_prime_post_caches($ids, $update_term_cache, $update_meta_cache);
    }

    function _prime_post_parent_id_caches(array $ids)
    {
        N\_prime_post_parent_id_caches($ids);
    }

    function wp_cache_set_posts_last_changed()
    {
        N\wp_cache_set_posts_last_changed();
    }

    function get_available_post_mime_types($type = 'attachment')
    {
        return N\get_available_post_mime_types($type);
    }
}

/**
 * The PHP the native reading functions leave to PHP: building `WP_Post` as WordPress builds it, writing a sanitized
 * object back through its handle, and the term branch of `WP_Post::__get()`.
 */
namespace SpeedySpec\WpPosts\Php
{
    /**
     * `new WP_Post( $post )`, as `WP_Post::get_instance()` and `get_post()` make one.
     */
    function post($post)
    {
        return new \WP_Post(is_array($post) ? (object) $post : $post);
    }

    /**
     * `$value instanceof WP_Post`.
     */
    function is_post($value): bool
    {
        return $value instanceof \WP_Post;
    }

    /**
     * Writes each property to the object, as `sanitize_post()` does through the object's handle.
     */
    function assign($object, array $properties)
    {
        foreach ($properties as $name => $value) {
            $object->$name = $value;
        }
        return $object;
    }

    /**
     * `$post->$name`, through the class's magic.
     */
    function property($post, $name)
    {
        return $post->$name;
    }

    /**
     * `WP_Post::__get()`'s `post_category` and `tags_input` branch.
     */
    function term_fields($post, $taxonomy, $field)
    {
        if (is_object_in_taxonomy($post->post_type, $taxonomy)) {
            $terms = get_the_terms($post, $taxonomy);
        }
        if (empty($terms) || $terms instanceof \WP_Error) {
            return array();
        }
        return wp_list_pluck($terms, $field);
    }
}
