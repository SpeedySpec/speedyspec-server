declare(strict_types=1);

namespace
{
    use SpeedySpec\WpPosts\Native as N;

    // post.php (WordPress 7.2-RC2): writing posts, native. Sticky posts, set_post_type(), deleting, trashing and
    // untrashing, wp_insert_post(), wp_update_post(), publishing and future posts, the status transitions, pings,
    // auto-drafts, and the default callbacks WordPress registers by name.

    function stick_post($post_id)
    {
        N\stick_post($post_id);
    }

    function unstick_post($post_id)
    {
        N\unstick_post($post_id);
    }

    function set_post_type($post_id = 0, $post_type = 'post')
    {
        return N\set_post_type($post_id, $post_type);
    }

    function wp_delete_post($post_id = 0, $force_delete = false)
    {
        return N\wp_delete_post($post_id, $force_delete);
    }

    function _reset_front_page_settings_for_post($post_id)
    {
        N\_reset_front_page_settings_for_post($post_id);
    }

    function _reset_privacy_policy_page_for_post(int $post_id): void
    {
        N\_reset_privacy_policy_page_for_post($post_id);
    }

    function wp_trash_post($post_id = 0)
    {
        return N\wp_trash_post($post_id);
    }

    function wp_untrash_post($post_id = 0)
    {
        return N\wp_untrash_post($post_id);
    }

    function wp_insert_post($postarr, $wp_error = false, $fire_after_hooks = true)
    {
        return N\wp_insert_post($postarr, $wp_error, $fire_after_hooks);
    }

    function wp_update_post($postarr = array(), $wp_error = false, $fire_after_hooks = true)
    {
        return N\wp_update_post($postarr, $wp_error, $fire_after_hooks);
    }

    function wp_publish_post($post)
    {
        N\wp_publish_post($post);
    }

    function check_and_publish_future_post($post)
    {
        N\check_and_publish_future_post($post);
    }

    function wp_transition_post_status($new_status, $old_status, $post)
    {
        N\wp_transition_post_status($new_status, $old_status, $post);
    }

    function add_ping($post, $uri)
    {
        return N\add_ping($post, $uri);
    }

    function wp_delete_auto_drafts()
    {
        N\wp_delete_auto_drafts();
    }

    function _transition_post_status($new_status, $old_status, $post)
    {
        N\_transition_post_status($new_status, $old_status, $post);
    }

    function _future_post_hook($deprecated, $post)
    {
        N\_future_post_hook($deprecated, $post);
    }

    function _publish_post_hook($post_id)
    {
        N\_publish_post_hook($post_id);
    }

    function wp_check_post_hierarchy_for_loops($post_parent, $post_id)
    {
        return N\wp_check_post_hierarchy_for_loops($post_parent, $post_id);
    }
}

/**
 * The PHP the native writing functions and their PHP adapters leave to PHP: `WP_Error`, `WP_Query`, the theme's page
 * templates, the column charsets, the slug settings, and the term and comment helpers whose WordPress code is inline.
 */
namespace SpeedySpec\WpPosts\Php
{
    /**
     * `new WP_Error( $code, $message, $data )`.
     */
    function wp_error($code, $message, $data)
    {
        return new \WP_Error($code, $message, $data);
    }

    /**
     * `( new WP_Query() )->query( $args )`.
     */
    function query($args)
    {
        return (new \WP_Query())->query($args);
    }

    /**
     * `wp_get_theme()->get_page_templates( $post )` with `$post->page_template` set, as wp_insert_post() asks.
     */
    function page_templates($post, $page_template)
    {
        $post->page_template = $page_template;
        return wp_get_theme()->get_page_templates($post);
    }

    /**
     * `$wpdb->get_col_charset( $wpdb->$table, $column )`: the charset, or false (for false or a WP_Error).
     */
    function col_charset($table, $column)
    {
        global $wpdb;
        $charset = $wpdb->get_col_charset($wpdb->$table, $column);
        return is_string($charset) ? $charset : false;
    }

    /**
     * What wp_unique_post_slug() reads: `$wp_rewrite->feeds` (an empty list when it is not an array),
     * `$wp_rewrite->pagination_base` and `get_option( 'permalink_structure' )`.
     */
    function slug_context()
    {
        global $wp_rewrite;
        $feeds = $wp_rewrite?->feeds;
        return [
            is_array($feeds) ? $feeds : [],
            (string) $wp_rewrite?->pagination_base,
            (string) get_option('permalink_structure'),
        ];
    }

    /**
     * The taxonomies of `get_object_taxonomies( $post_type, 'object' )` with a `default_term`, in order.
     */
    function default_terms($post_type)
    {
        $names = [];
        foreach (get_object_taxonomies($post_type, 'object') as $taxonomy => $object) {
            if (!empty($object->default_term)) {
                $names[] = $taxonomy;
            }
        }
        return $names;
    }

    /**
     * `get_taxonomy( $taxonomy )->cap->assign_terms` as a one-item list, or null where get_taxonomy() is false.
     */
    function assign_terms_cap($taxonomy)
    {
        $object = get_taxonomy($taxonomy);
        return $object ? [$object->cap->assign_terms] : null;
    }

    /**
     * wp_delete_post()'s comment loop (post.php 4118-4124).
     */
    function delete_post_comments($post_id)
    {
        global $wpdb;
        $comment_ids = $wpdb->get_col(
            $wpdb->prepare("SELECT comment_ID FROM $wpdb->comments WHERE comment_post_ID = %d ORDER BY comment_ID DESC", $post_id)
        );
        foreach ($comment_ids as $comment_id) {
            wp_delete_comment($comment_id, true);
        }
    }
}
