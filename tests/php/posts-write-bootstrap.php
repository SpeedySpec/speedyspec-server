<?php

declare(strict_types=1);

/**
 * Loads what WordPress's post-writing tests need.
 *
 *  Everything the post-reading tests' bootstrap loads (posts-bootstrap.php), with the factory making posts through
 *  the native `wp_insert_post()` and `wp_update_post()` (`SPEEDYSPEC_FACTORY_POSTS_INSERT`, see includes/factory.php),
 *  and default-filters.php's callbacks that the writers
 *  run or pass their values through, in its order, as compat/posts-write/setup.php registers them: the key, mime
 *  type, GUID and title sanitizers, kses (`kses_init` on `init` and `set_current_user`),
 *  `wp_check_post_hierarchy_for_loops`, the posts and comments `last_changed` actions, the future-post check, the
 *  revision savers, `_publish_post_hook`, `_transition_post_status`, `_update_term_count_on_transition_post_status`,
 *  `wp_check_for_changed_slugs` and `wp_check_for_changed_dates`, `delete_get_calendar_cache`,
 *  `__clear_multi_author_cache`, the front-page and privacy-page resets, and on multisite `wp_switch_roles_and_user`.
 *
 *  Beside them, what WordPress's install and bootstrap give that the writers read: the default comment, ping and
 *  category options, kses through the HTML API (default-filters.php's `wp_kses_force_legacy_parser`),
 *  `WP_POST_REVISIONS`, deprecated.php, the comment classes, wp-admin's post.php (`edit_post()`), and the test
 *  utilities `rand_str()` and `get_locale()`.
 *
 *  Users, terms and comments stay rows (`SPEEDYSPEC_FACTORY_ROWS`). Every post writer is the native one: insert,
 *  update, the transitions and set_post_type(), and delete, trash, untrash, publish, future posts, sticky posts,
 *  pings and auto-drafts; the bootstrap refuses to run otherwise. The option functions are native too, as the
 *  runtime runs them beside the writers (the harness strips option.php for this suite). The attachment and
 *  revision functions behind their ports (`wp_delete_attachment()`, `wp_delete_post_revision()`) are WordPress's own.
 */

define('SPEEDYSPEC_FACTORY_POSTS_INSERT', true);
require __DIR__ . '/posts-bootstrap.php';
// The point of the suite: the writers under test are the native ones (the area's facade over its natives), not a
// PHP copy left in the release.
$speedyspec_writers = [
    'wp_insert_post',
    'wp_update_post',
    'wp_transition_post_status',
    'set_post_type',
    'wp_delete_post',
    'wp_trash_post',
    'wp_untrash_post',
    'wp_publish_post',
    'check_and_publish_future_post',
    'stick_post',
    'unstick_post',
    'add_ping',
    'wp_delete_auto_drafts',
];
foreach ($speedyspec_writers as $speedyspec_writer)
{
    if (str_starts_with((string) (new ReflectionFunction($speedyspec_writer))->getFileName(), ABSPATH))
    {
        throw new RuntimeException("$speedyspec_writer() is not the native one.");
    }
}
// __clear_multi_author_cache(), which runs on every transition; the comment classes wp_delete_post() reads comments
// with; deprecated.php (`_truncate_post_slug()`); wp-admin's `edit_post()`.
require_once ABSPATH . WPINC . '/author-template.php';
require_once ABSPATH . WPINC . '/class-wp-comment.php';
require_once ABSPATH . WPINC . '/class-wp-comment-query.php';
require_once ABSPATH . WPINC . '/deprecated.php';
require_once ABSPATH . 'wp-admin/includes/post.php';
// default-constants.php's wp_functionality_constants(): revisions are kept.
if (!defined('WP_POST_REVISIONS'))
{
    define('WP_POST_REVISIONS', true);
}

/**
 * Answers the site's locale as l10n.php's does with no locale set (`remove_accents()` reads it); l10n.php is not
 *  loaded.
 */
function get_locale()
{
    return 'en_US';
}

/**
 * Makes a random string, as WordPress's tests/phpunit/includes/utils.php does.
 */
function rand_str($length = 32)
{
    return substr(md5(uniqid((string) rand())), 0, $length);
}

// The options the install sets that the writers read: new posts open to comments and pings, and category 1 as the
// default category.
update_option('default_comment_status', 'open');
update_option('default_ping_status', 'open');
update_option('default_category', 1);

foreach (['pre_post_type', 'pre_post_status', 'pre_post_comment_status', 'pre_post_ping_status'] as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'sanitize_key');
}
foreach (['wp_strip_all_tags', 'sanitize_url', 'wp_filter_kses'] as $speedyspec_callback)
{
    add_filter('pre_post_guid', $speedyspec_callback);
}
add_filter('pre_post_mime_type', 'sanitize_mime_type');
add_filter('post_mime_type', 'sanitize_mime_type');
add_action('added_post_meta', 'wp_cache_set_posts_last_changed');
add_action('updated_post_meta', 'wp_cache_set_posts_last_changed');
add_action('deleted_post_meta', 'wp_cache_set_posts_last_changed');
add_action('added_comment_meta', 'wp_cache_set_comments_last_changed');
add_action('updated_comment_meta', 'wp_cache_set_comments_last_changed');
add_action('deleted_comment_meta', 'wp_cache_set_comments_last_changed');
add_filter('wp_insert_post_parent', 'wp_check_post_hierarchy_for_loops', 10, 2);
add_filter('title_save_pre', 'trim');
add_action('publish_future_post', 'check_and_publish_future_post', 10, 1);
add_action('wp_after_insert_post', 'wp_save_post_revision_on_insert', 9, 3);
add_action('post_updated', 'wp_save_post_revision', 10, 1);
add_action('publish_post', '_publish_post_hook', 5, 1);
add_action('transition_post_status', '_transition_post_status', 5, 3);
add_action('transition_post_status', '_update_term_count_on_transition_post_status', 10, 3);
add_action('post_updated', 'wp_check_for_changed_slugs', 12, 3);
add_action('attachment_updated', 'wp_check_for_changed_slugs', 12, 3);
add_action('post_updated', 'wp_check_for_changed_dates', 12, 3);
add_action('attachment_updated', 'wp_check_for_changed_dates', 12, 3);
add_action('save_post', 'delete_get_calendar_cache');
add_action('delete_post', 'delete_get_calendar_cache');
add_action('transition_post_status', '__clear_multi_author_cache');
add_action('before_delete_post', '_reset_front_page_settings_for_post');
add_action('before_delete_post', '_reset_privacy_policy_page_for_post');
add_action('wp_trash_post', '_reset_front_page_settings_for_post');
// kses filters with the HTML API, not the legacy parser.
add_filter('wp_kses_force_legacy_parser', '__return_false');
add_action('init', 'kses_init');
add_action('set_current_user', 'kses_init');
if (is_multisite())
{
    add_action('switch_blog', 'wp_switch_roles_and_user', 1, 2);
}
// As on `init`: no user is logged in, so kses filters what the writers save.
kses_init();
