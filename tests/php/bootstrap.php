<?php

declare(strict_types=1);

/**
 * Loads what WordPress's hook tests need around the native Hook API: the stub test case, the mocks, and the few
 * WordPress functions they call that are not part of the Hook API.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('add_filter')) {
    throw new RuntimeException('The native Hook API is not loaded: run these tests in the embedded PHP.');
}

/**
 * Fires `deprecated_hook_run`, as WordPress's `_deprecated_hook()` does (functions.php is not loaded here).
 */
function _deprecated_hook($hook, $version, $replacement = '', $message = ''): void
{
    do_action('deprecated_hook_run', $hook, $replacement, $version, $message);
}

function __return_true(): bool
{
    return true;
}

function __return_false(): bool
{
    return false;
}

function __return_zero(): int
{
    return 0;
}

function __return_null()
{
    return null;
}

function __return_empty_array(): array
{
    return array();
}

function __return_empty_string(): string
{
    return '';
}

/**
 * Stands in for WordPress's post object, which the deprecated-action tests pass through hooks.
 */
#[AllowDynamicProperties]
final class WP_Post
{
    public $post_title = '';

    public function __construct($post)
    {
        foreach (get_object_vars($post) as $key => $value) {
            $this->$key = $value;
        }
    }
}

/**
 * Plucks a field out of each object or array in a list, as WordPress's `wp_list_pluck()` does.
 */
function wp_list_pluck($input_list, $field, $index_key = null): array
{
    $plucked = array();
    foreach ($input_list as $key => $item) {
        $value = is_object($item) ? $item->$field : $item[$field];
        if (null === $index_key) {
            $plucked[$key] = $value;
        } else {
            $index = is_object($item) ? ($item->$index_key ?? null) : ($item[$index_key] ?? null);
            if (null === $index) {
                $plucked[] = $value;
            } else {
                $plucked[$index] = $value;
            }
        }
    }
    return $plucked;
}

// WordPress's default filters register `the_content`; Tests_Filters::test_other_priority reads its hook object.
add_filter('the_content', '__return_null');
