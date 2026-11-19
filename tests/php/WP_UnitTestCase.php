<?php

declare(strict_types=1);

use PHPUnit\Framework\TestCase;

/**
 * Raised in place of a PHP deprecation while a test expects one.
 */
final class SpeedySpecDeprecationRaised extends RuntimeException
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

    public static function set_up_before_class()
    {
    }

    public static function tear_down_after_class()
    {
    }

    public function set_up()
    {
        $this->_backup_hooks();
        $this->expectDeprecated();
    }

    public function tear_down()
    {
        if ($this->speedyspec_error_handler) {
            restore_error_handler();
            $this->speedyspec_error_handler = false;
        }
        $this->_restore_hooks();
        $this->expected_deprecated = array();
        $this->caught_deprecated = array();
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
        add_action('deprecated_hook_run', array($this, 'deprecated_function_run'), 10, 4);
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
        if (!empty($this->expected_deprecated) || !empty($this->caught_deprecated)) {
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
     * Asserts that two arrays hold the same values, in any order.
     */
    public function assertSameSets($expected, $actual, $message = '')
    {
        $this->assertIsArray($expected, $message . ' Expected value must be an array.');
        $this->assertIsArray($actual, $message . ' Value under test is not an array.');

        sort($expected);
        sort($actual);
        $this->assertSame($expected, $actual, $message);
    }
}
