<?php

declare(strict_types=1);

/**
 * Loads what WordPress's kses tests need around the native kses functions: the stub test case, then the release's
 * own load, compat, UTF-8, formatting, functions, comment and HTML API files, and its kses.php for the globals (the
 * release is prepared by speedyspec-wp-kses-php, so the ported functions are the native ones). WordPress's default
 * kses filters (default-filters.php) are added, and the few options these files read come from `pre_option`, as no
 * database is used.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('wp_kses') || !function_exists('add_filter'))
{
    throw new RuntimeException('The native kses and Hook APIs are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_KSES_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_KSES_WP_ROOT to the prepared WordPress release.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_TESTS_DOMAIN', 'example.org');
define('WP_CONTENT_DIR', sys_get_temp_dir() . '/speedyspec-kses-content');
define('WP_CONTENT_URL', 'http://' . WP_TESTS_DOMAIN . '/wp-content');
// WordPress's test data the copied tests read (tests/phpunit/data in wordpress-develop).
define('DIR_TESTDATA', __DIR__ . '/data');

/** The options the loaded files read, as a fresh install has them. */
$GLOBALS['speedyspec_kses_options'] = array(
    'blog_charset'                  => 'UTF-8',
    'siteurl'                       => 'http://' . WP_TESTS_DOMAIN,
    'home'                          => 'http://' . WP_TESTS_DOMAIN,
    'upload_path'                   => '',
    'upload_url_path'               => '',
    'uploads_use_yearmonth_folders' => '1',
);
add_filter(
    'pre_option',
    static function ($pre, $option) {
        return $GLOBALS['speedyspec_kses_options'][$option] ?? '';
    },
    10,
    2
);
add_filter('pre_wp_load_alloptions', static fn () => $GLOBALS['speedyspec_kses_options']);

/** No user is logged in. */
function current_user_can($capability, ...$args)
{
    return false;
}

require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/class-wp-token-map.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/comment.php';
require ABSPATH . WPINC . '/blocks.php';
require ABSPATH . WPINC . '/link-template.php';
require ABSPATH . WPINC . '/http.php';
require ABSPATH . WPINC . '/class-wp-block-parser-block.php';
require ABSPATH . WPINC . '/class-wp-block-parser-frame.php';
// WordPress 7.2's block parser reads documents through WP_Block_Processor.
require ABSPATH . WPINC . '/class-wp-block-processor.php';
require ABSPATH . WPINC . '/class-wp-block-parser.php';
foreach (
    array(
        'html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span',
        'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder',
        'class-wp-html-tag-processor', 'class-wp-html-unsupported-exception', 'class-wp-html-active-formatting-elements',
        'class-wp-html-open-elements', 'class-wp-html-token', 'class-wp-html-stack-event',
        'class-wp-html-processor-state', 'class-wp-html-processor',
    ) as $speedyspec_file
)
{
    require ABSPATH . WPINC . "/html-api/{$speedyspec_file}.php";
}
require ABSPATH . WPINC . '/kses.php';
require __DIR__ . '/includes/build-visual-html-tree.php';

// default-filters.php's lines for kses and the comment fields kses filters, as WordPress's tests have them.
foreach (array('pre_term_name', 'pre_comment_author_name', 'pre_link_name', 'pre_link_target', 'pre_link_rel', 'pre_user_display_name', 'pre_user_first_name', 'pre_user_last_name', 'pre_user_nickname') as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'sanitize_text_field');
    add_filter($speedyspec_filter, 'wp_filter_kses');
    add_filter($speedyspec_filter, '_wp_specialchars', 30);
}
foreach (array('pre_term_description', 'pre_link_description', 'pre_link_notes', 'pre_user_description') as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'wp_filter_kses');
}
foreach (array('pre_comment_author_email', 'pre_user_email') as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'trim');
    add_filter($speedyspec_filter, 'sanitize_email');
    add_filter($speedyspec_filter, 'wp_filter_kses');
}
foreach (array('pre_comment_author_url', 'pre_user_url', 'pre_link_url', 'pre_link_image', 'pre_link_rss', 'pre_post_guid') as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'wp_strip_all_tags');
    add_filter($speedyspec_filter, 'sanitize_url');
    add_filter($speedyspec_filter, 'wp_filter_kses');
}
foreach (array('content_save_pre', 'excerpt_save_pre', 'comment_save_pre', 'pre_comment_content') as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'convert_invalid_entities');
    add_filter($speedyspec_filter, 'balanceTags', 50);
}
add_filter('pre_kses', 'wp_pre_kses_less_than');
add_filter('pre_kses', 'wp_pre_kses_block_attributes', 10, 3);
add_filter('pre_comment_content', 'wp_rel_ugc', 15);
add_filter('wp_kses_allowed_html', '_wp_kses_allow_note_mention_span', 10, 2);
add_filter('pre_comment_content', '_wp_kses_sanitize_note_mention_classes', 11);

// What `init` does for kses with no user logged in.
kses_init();
