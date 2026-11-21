<?php

declare(strict_types=1);

/**
 * Loads what WordPress's rewrite tests need around the native rewrite API: the stub test case and mocks, then the
 *  rewrite compatibility setup on the port's side (the release's `class-wp.php` patched to resolve through the Rust
 *  router, and the same stubbed WordPress the recorded scenarios use: options in an array, no posts).
 *
 *  Run here (tests/php/wordpress/rewrite): `addRewriteRule.php`, `addRewriteEndpoint.php`, `permastructs.php` and
 *  `rewriteTags.php` (renamed `Tests_Rewrite_Tags.php`: PHPUnit 11 finds a file's test class by its name, and its data provider made `static`, which PHPUnit 11 requires),
 *  except the
 *  three endpoint tests that visit posts with `go_to()`.
 *
 *  Not run: `rewrite.php`, `numericSlugs.php`, `oldDateRedirect.php`, `oldSlugRedirect.php` and
 *  `wpResolveNumericSlugConflicts.php`. They create and visit posts (`factory()->post`, `go_to()`, `url_to_postid()`,
 *  canonical and old-slug redirects), which need the posts and `WP_Query` ports and a full WordPress bootstrap that
 *  do not exist yet; the recorded scenarios in compat/rewrite cover the matching they would exercise.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('speedyspec_rewrite_match') || !class_exists('WP_Rewrite'))
{
    throw new RuntimeException('The native rewrite API is not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_REWRITE_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_REWRITE_WP_ROOT to the WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
define('DIR_TESTDATA', __DIR__ . '/data');
require dirname(__DIR__, 2) . '/compat/rewrite/setup.php';
speedyspec_compat_configure('');
do_action('wp_loaded');
