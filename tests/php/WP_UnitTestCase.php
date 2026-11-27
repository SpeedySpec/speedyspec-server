<?php

declare(strict_types=1);

use PHPUnit\Framework\TestCase;

require_once __DIR__ . '/includes/factory.php';

/**
 * Raised in place of a PHP deprecation while a test expects one.
 */
final class SpeedySpecDeprecationRaised extends RuntimeException
{
}

/**
 * Raised in place of a PHP warning while a test expects one.
 */
final class SpeedySpecWarningRaised extends RuntimeException
{
}

/**
 * Stands in for WordPress's WP_UnitTestCase, for its hook tests: the same set-up and tear-down names, hook backup
 * and restore over the Rust hook store, the deprecation expectations those tests use, and `assertSameSets()`.
 *
 * It needs no database. Parts adapted from WordPress's tests/phpunit/includes/abstract-testcase.php (GPL-2.0 or
 * later).
 */
abstract class WP_UnitTestCase extends TestCase
{
    /** @var string[] */
    protected $expected_deprecated = array();

    /** @var array<string, string> */
    protected $caught_deprecated = array();

    protected $expected_doing_it_wrong = array();

    protected $caught_doing_it_wrong = array();

    private ?int $speedyspec_snapshot = null;

    private bool $speedyspec_error_handler = false;

    final public static function setUpBeforeClass(): void
    {
        static::set_up_before_class();
    }

    final public static function tearDownAfterClass(): void
    {
        static::tear_down_after_class();
    }

    final protected function setUp(): void
    {
        $this->set_up();
    }

    final protected function tearDown(): void
    {
        $this->tear_down();
    }

    final protected function assertPostConditions(): void
    {
        $this->assert_post_conditions();
    }

    /** @var WP_UnitTest_Factory|null */
    private static $speedyspec_factory = null;

    /**
     * The test factory (networks only; see includes/factory.php).
     */
    protected static function factory()
    {
        return self::$speedyspec_factory ??= new WP_UnitTest_Factory();
    }

    /**
     * Runs the test class's `wpSetUpBeforeClass( $factory )`, as WordPress's test case does.
     */
    public static function set_up_before_class()
    {
        if (method_exists(static::class, 'wpSetUpBeforeClass'))
        {
            call_user_func(array(static::class, 'wpSetUpBeforeClass'), static::factory());
        }
    }

    /**
     * Runs the test class's `wpTearDownAfterClass()`, as WordPress's test case does.
     *
     * A bootstrap may name classes whose clean-up calls functions that are not loaded in
     *  `SPEEDYSPEC_SKIP_TEAR_DOWN_AFTER_CLASS`; their fixtures stay until the bootstrap empties the tables at the start
     *  of the next run.
     */
    public static function tear_down_after_class()
    {
        $skipped = defined('SPEEDYSPEC_SKIP_TEAR_DOWN_AFTER_CLASS') ? SPEEDYSPEC_SKIP_TEAR_DOWN_AFTER_CLASS : array();
        if (method_exists(static::class, 'wpTearDownAfterClass') && !in_array(static::class, $skipped, true))
        {
            call_user_func(array(static::class, 'wpTearDownAfterClass'));
        }
    }

    public function set_up()
    {
        $this->_backup_hooks();
        if (isset($GLOBALS['wpdb']))
        {
            $this->start_transaction();
            // WordPress's own set_up reads options (resetting post types and taxonomies), so each test starts with
            // alloptions loaded; the option tests count queries from there.
            if (function_exists('wp_load_alloptions'))
            {
                wp_load_alloptions();
            }
        }
        if (function_exists('speedyspec_doubles_snapshot'))
        {
            speedyspec_doubles_snapshot();
        }
        $this->expectDeprecated();
    }

    /**
     * Runs each test in a transaction, with created and dropped tables made temporary, as WordPress's test case
     * does, so a test's writes never outlive it.
     */
    public function start_transaction()
    {
        global $wpdb;
        $wpdb->query('SET autocommit = 0;');
        $wpdb->query('START TRANSACTION;');
        add_filter('query', array($this, '_create_temporary_tables'));
        add_filter('query', array($this, '_drop_temporary_tables'));
    }

    public function _create_temporary_tables($query)
    {
        if (0 === strpos(trim($query), 'CREATE TABLE'))
        {
            return substr_replace(trim($query), 'CREATE TEMPORARY TABLE', 0, 12);
        }
        return $query;
    }

    public function _drop_temporary_tables($query)
    {
        if (0 === strpos(trim($query), 'DROP TABLE'))
        {
            return substr_replace(trim($query), 'DROP TEMPORARY TABLE', 0, 10);
        }
        return $query;
    }

    /**
     * Expects a `_doing_it_wrong()` notice from the named function in the running test.
     */
    public function setExpectedIncorrectUsage($doing_it_wrong)
    {
        $this->expected_doing_it_wrong[] = $doing_it_wrong;
    }

    public function doing_it_wrong_run($function_name, $message, $version)
    {
        if (!isset($this->caught_doing_it_wrong[$function_name]))
        {
            if ($version)
            {
                $message .= ' ' . sprintf('(This message was added in version %s.)', $version);
            }
            $this->caught_doing_it_wrong[$function_name] = $message;
        }
    }

    public function deprecated_argument_run($function_name, $message, $version)
    {
        $this->deprecated_function_run($function_name, '', $version, $message);
    }

    public function deprecated_function_run_for_function($function_name, $replacement, $version, $message = '')
    {
        $this->deprecated_function_run($function_name, $replacement, $version, $message);
    }

    public function tear_down()
    {
        if ($this->speedyspec_error_handler) {
            restore_error_handler();
            $this->speedyspec_error_handler = false;
        }
        if (isset($GLOBALS['wpdb']))
        {
            $GLOBALS['wpdb']->query('ROLLBACK');
            // The rolled-back rows must not outlive the test in the object cache either.
            if (function_exists('wp_cache_flush'))
            {
                wp_cache_flush();
            }
        }
        // As WordPress's tear_down: the screen globals set_current_screen() writes, and no current user.
        foreach (array('current_screen', 'taxnow', 'typenow') as $global)
        {
            $GLOBALS[$global] = null;
        }
        if (function_exists('wp_set_current_user'))
        {
            wp_set_current_user(0);
        }
        if (function_exists('speedyspec_doubles_restore'))
        {
            speedyspec_doubles_restore();
        }
        $this->_restore_hooks();
        $this->expected_deprecated = array();
        $this->caught_deprecated = array();
        $this->expected_doing_it_wrong = array();
        $this->caught_doing_it_wrong = array();
    }

    /**
     * Empties the object cache between tests and declares the groups WordPress's test case declares, as WordPress's
     * test case does.
     */
    public static function flush_cache()
    {
        global $wp_object_cache;

        wp_cache_flush_runtime();

        if (is_object($wp_object_cache) && method_exists($wp_object_cache, '__remoteset'))
        {
            $wp_object_cache->__remoteset();
        }

        wp_cache_flush();

        wp_cache_add_global_groups(
            array(
                'blog-details',
                'blog-id-cache',
                'blog-lookup',
                'blog_meta',
                'global-posts',
                'networks',
                'network-queries',
                'sites',
                'site-details',
                'site-options',
                'site-queries',
                'site-transient',
                'theme_files',
                'rss',
                'users',
                'user-queries',
                'user_meta',
                'useremail',
                'userlogins',
                'userslugs',
            )
        );

        wp_cache_add_non_persistent_groups(array('counts', 'plugins', 'theme_json'));
    }

    /**
     * Unregisters every registered meta key, as WordPress's test case does.
     */
    public function unregister_all_meta_keys()
    {
        global $wp_meta_keys;
        if (!is_array($wp_meta_keys))
        {
            return;
        }
        foreach ($wp_meta_keys as $object_type => $type_keys)
        {
            foreach ($type_keys as $object_subtype => $subtype_keys)
            {
                foreach ($subtype_keys as $key => $value)
                {
                    unregister_meta_key($object_type, $key, $object_subtype);
                }
            }
        }
    }

    /**
     * Deletes a user: through `wp_delete_user()` when wp-admin's user functions are loaded, and otherwise the
     *  `wp_users` row and its meta (what includes/factory.php stores).
     *
     * @return bool
     *   Whether the user was deleted.
     */
    public static function delete_user($user_id)
    {
        if (function_exists('wp_delete_user'))
        {
            return wp_delete_user($user_id);
        }
        global $wpdb;
        $wpdb->delete($wpdb->usermeta, array('user_id' => $user_id));
        return (bool) $wpdb->delete($wpdb->users, array('ID' => $user_id));
    }

    protected function assert_post_conditions()
    {
        $this->expectedDeprecated();
    }

    /**
     * Saves the hook store, so that this test's hooks do not leak into the next.
     */
    protected function _backup_hooks()
    {
        $this->speedyspec_snapshot = \SpeedySpec\WpHooks\Testing::snapshot();
    }

    protected function _restore_hooks()
    {
        if (null !== $this->speedyspec_snapshot) {
            \SpeedySpec\WpHooks\Testing::restore($this->speedyspec_snapshot);
        }
    }

    /**
     * Reads the test's `@expectedDeprecated` annotations and starts listening for deprecation notices.
     */
    public function expectDeprecated()
    {
        $doc = (new ReflectionMethod($this, $this->name()))->getDocComment() ?: '';
        if (preg_match_all('/@expectedDeprecated\s+(\S+)/', $doc, $matches)) {
            $this->expected_deprecated = array_merge($this->expected_deprecated, $matches[1]);
        }
        if (preg_match_all('/@expectedIncorrectUsage\s+(\S+)/', $doc, $matches)) {
            $this->expected_doing_it_wrong = array_merge($this->expected_doing_it_wrong, $matches[1]);
        }
        add_action('deprecated_hook_run', array($this, 'deprecated_function_run'), 10, 4);
        add_action('deprecated_function_run', array($this, 'deprecated_function_run_for_function'), 10, 3);
        add_action('doing_it_wrong_run', array($this, 'doing_it_wrong_run'), 10, 3);
        add_action('deprecated_argument_run', array($this, 'deprecated_argument_run'), 10, 3);
    }

    public function deprecated_function_run($function_name, $replacement, $version, $message = '')
    {
        if (!isset($this->caught_deprecated[$function_name])) {
            $this->caught_deprecated[$function_name] = "$function_name is deprecated since version $version. $message";
        }
    }

    /**
     * Fails the test for every expected deprecation that did not happen, and every one that was not expected.
     */
    public function expectedDeprecated()
    {
        $errors = array();
        foreach (array_diff($this->expected_deprecated, array_keys($this->caught_deprecated)) as $not_caught) {
            $errors[] = "Failed to assert that $not_caught triggered a deprecation notice.";
        }
        foreach (array_diff(array_keys($this->caught_deprecated), $this->expected_deprecated) as $unexpected) {
            $errors[] = "Unexpected deprecation notice for $unexpected.";
            $errors[] = $this->caught_deprecated[$unexpected];
        }
        foreach (array_diff($this->expected_doing_it_wrong, array_keys($this->caught_doing_it_wrong)) as $not_caught) {
            $errors[] = "Failed to assert that $not_caught triggered an incorrect usage notice.";
        }
        foreach (array_diff(array_keys($this->caught_doing_it_wrong), $this->expected_doing_it_wrong) as $unexpected) {
            $errors[] = "Unexpected incorrect usage notice for $unexpected.";
            $errors[] = $this->caught_doing_it_wrong[$unexpected];
        }
        if (!empty($this->expected_deprecated) || !empty($this->caught_deprecated)
            || !empty($this->expected_doing_it_wrong) || !empty($this->caught_doing_it_wrong)) {
            $this->assertEmpty($errors, implode("\n", $errors));
        }
    }

    /**
     * Expects a PHP deprecation (PHPUnit 9's `expectDeprecation()`): the first one ends the test as an exception.
     */
    public function expectDeprecation(): void
    {
        set_error_handler(
            static function (int $errno, string $errstr): bool {
                if ($errno & (E_DEPRECATED | E_USER_DEPRECATED)) {
                    throw new SpeedySpecDeprecationRaised($errstr);
                }
                return false;
            }
        );
        $this->speedyspec_error_handler = true;
        $this->expectException(SpeedySpecDeprecationRaised::class);
    }

    public function expectDeprecationMessage(string $message): void
    {
        $this->expectExceptionMessage($message);
    }

    /**
     * Expects a PHP warning (PHPUnit 9's `expectWarning()`): the first one ends the test as an exception.
     */
    public function expectWarning(): void
    {
        set_error_handler(
            static function (int $errno, string $errstr): bool {
                if ($errno & (E_WARNING | E_USER_WARNING)) {
                    throw new SpeedySpecWarningRaised($errstr);
                }
                return false;
            }
        );
        $this->speedyspec_error_handler = true;
        $this->expectException(SpeedySpecWarningRaised::class);
    }

    public function expectWarningMessage(string $message): void
    {
        $this->expectExceptionMessage($message);
    }

    /**
     * Asserts that two values are the same once Windows line endings are made Unix ones, as WordPress's test case
     * does.
     */
    public function assertSameIgnoreEOL($expected, $actual, $message = '')
    {
        $unix = static function ($value) {
            return is_string($value) ? str_replace("\r\n", "\n", $value) : $value;
        };
        if (null !== $expected)
        {
            $expected = map_deep($expected, $unix);
        }
        if (null !== $actual)
        {
            $actual = map_deep($actual, $unix);
        }
        $this->assertSame($expected, $actual, $message);
    }

    /**
     * A new temporary file's name, as WordPress's test case makes one.
     *
     * @return string
     *     The file's path; the file exists and is empty.
     */
    public function temp_filename()
    {
        $tmp_dir = '';
        foreach (array('TMP', 'TMPDIR', 'TEMP') as $dir)
        {
            if (isset($_ENV[$dir]) && !empty($_ENV[$dir]))
            {
                $tmp_dir = $_ENV[$dir];
                break;
            }
        }
        if (empty($tmp_dir))
        {
            $tmp_dir = sys_get_temp_dir();
        }
        return tempnam(realpath($tmp_dir), 'wpunit');
    }

    /**
     * Asserts that two arrays hold the same values, in any order.
     */
    /**
     * Asserts that a value is a WP_Error, as WordPress's test case does.
     */
    public function assertWPError($actual, $message = '')
    {
        $this->assertInstanceOf('WP_Error', $actual, $message);
    }

    /**
     * Asserts that a value is not a WP_Error, naming the error's message when it is, as WordPress's test case does.
     */
    public function assertNotWPError($actual, $message = '')
    {
        if ('' === $message && $actual instanceof WP_Error)
        {
            $message = $actual->get_error_message();
        }
        $this->assertNotInstanceOf('WP_Error', $actual, $message);
    }

    /**
     * Asserts that a value is an IXR_Error, as WordPress's test case does.
     */
    public function assertIXRError($actual, $message = '')
    {
        $this->assertInstanceOf('IXR_Error', $actual, $message);
    }

    /**
     * Asserts that a value is not an IXR_Error, naming the error's message when it is, as WordPress's test case does.
     */
    public function assertNotIXRError($actual, $message = '')
    {
        if ($actual instanceof IXR_Error)
        {
            $message .= ' ' . $actual->message;
        }
        $this->assertNotInstanceOf('IXR_Error', $actual, $message);
    }

    /**
     * Removes the uploads a test added; the suites run here add none, so there is nothing to remove.
     */
    public function remove_added_uploads()
    {
    }

    public function assertSameSets($expected, $actual, $message = '')
    {
        $this->assertIsArray($expected, $message . ' Expected value must be an array.');
        $this->assertIsArray($actual, $message . ' Value under test is not an array.');

        sort($expected);
        sort($actual);
        $this->assertSame($expected, $actual, $message);
    }

    /**
     * Asserts that two arrays hold equal values, in any order (WordPress's `assertEqualSets()`).
     */
    public function assertEqualSets($expected, $actual, $message = '')
    {
        $this->assertIsArray($expected, $message . ' Expected value must be an array.');
        $this->assertIsArray($actual, $message . ' Value under test is not an array.');

        sort($expected);
        sort($actual);
        $this->assertEquals($expected, $actual, $message);
    }

    /**
     * Asserts that two arrays hold equal values under equal keys, in any order (WordPress's
     *  `assertEqualSetsWithIndex()`).
     */
    public function assertEqualSetsWithIndex($expected, $actual, $message = '')
    {
        $this->assertIsArray($expected, $message . ' Expected value must be an array.');
        $this->assertIsArray($actual, $message . ' Value under test is not an array.');

        ksort($expected);
        ksort($actual);
        $this->assertEquals($expected, $actual, $message);
    }

    /**
     * Asserts that two arrays hold the same keys and values, in any order.
     */
    public function assertSameSetsWithIndex($expected, $actual, $message = '')
    {
        $this->assertIsArray($expected, $message . ' Expected value must be an array.');
        $this->assertIsArray($actual, $message . ' Value under test is not an array.');

        ksort($expected);
        ksort($actual);
        $this->assertSame($expected, $actual, $message);
    }

    /**
     * Sets the permalink structure and flushes the rewrite rules, as WordPress's test case does.
     */
    public function set_permalink_structure($structure = '')
    {
        global $wp_rewrite;

        $wp_rewrite->init();
        $wp_rewrite->set_permalink_structure($structure);
        $wp_rewrite->flush_rules();
    }

    /**
     * Asserts that two HTML snippets have the same semantic tree, as WordPress's own test case does (it needs
     * `build_visual_html_tree()` and the HTML API loaded).
     */
    public function assertEqualHTML(string $expected, string $actual, ?string $fragment_context = '<body>', $message = 'HTML markup was not equivalent.'): void
    {
        try {
            $tree_expected = build_visual_html_tree($expected, $fragment_context);
            $tree_actual   = build_visual_html_tree($actual, $fragment_context);
        } catch (Exception $e) {
            // For PHP 8.4+, we can retry, using the built-in DOM\HTMLDocument parser.
            if (class_exists('DOM\HtmlDocument')) {
                $dom_expected  = DOM\HtmlDocument::createFromString($expected, LIBXML_NOERROR);
                $tree_expected = build_visual_html_tree($dom_expected->saveHtml(), $fragment_context);
                $dom_actual    = DOM\HtmlDocument::createFromString($actual, LIBXML_NOERROR);
                $tree_actual   = build_visual_html_tree($dom_actual->saveHtml(), $fragment_context);
            } else {
                throw $e;
            }
        }

        $this->assertSame($tree_expected, $tree_actual, $message);
    }

    /**
     * A single-level array of text strings as a named data provider, as WordPress's own test case makes one.
     */
    public static function text_array_to_dataprovider($input)
    {
        $data = array();

        foreach ($input as $value) {
            if (!is_string($value)) {
                throw new Exception('All values in the input array should be text strings. Fix the input data.');
            }

            if (isset($data[$value])) {
                throw new Exception(
                    "Attempting to add a duplicate data set for value $value to the data provider. Fix the input data."
                );
            }

            $data[$value] = array($value);
        }

        return $data;
    }
}
