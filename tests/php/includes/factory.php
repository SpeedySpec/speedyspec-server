<?php

declare(strict_types=1);

/**
 * Stands in for WordPress's test factory (tests/phpunit/includes/factory): networks, for the option tests, and users,
 * for the locale tests.
 *
 * WordPress's network factory calls `populate_network()` (wp-admin's schema, with users and roles), which is not
 * loaded here; the option tests only need a network id that is not the current one. Sites and users are not made:
 * the one option test that creates them (`Tests_Option_Multisite::test_with_another_site`) is excluded.
 */
final class WP_UnitTest_Factory
{
    /** @var WP_UnitTest_Factory_For_Network */
    public $network;

    /** @var WP_UnitTest_Factory_For_User */
    public $user;

    public function __construct()
    {
        $this->network = new WP_UnitTest_Factory_For_Network();
        $this->user = new WP_UnitTest_Factory_For_User();
    }
}

/**
 * Makes users through `wp_insert_user()`, which the suite that needs users provides (for the locale tests, the
 * in-memory stand-in in includes/i18n-doubles.php).
 */
final class WP_UnitTest_Factory_For_User
{
    /**
     * Creates a user and returns its id.
     *
     * @param array<string, mixed> $args
     *     `role`, `locale` and other fields, as WordPress's factory takes them.
     * @return int
     *     The new user's id.
     */
    public function create($args = array())
    {
        return (int) wp_insert_user($args);
    }
}

/**
 * Makes networks: a `wp_site` row on multisite (when the table exists), numbered from 2 as WordPress's factory does.
 */
final class WP_UnitTest_Factory_For_Network
{
    private static int $next = 2;

    /**
     * Creates a network and returns its id.
     *
     * @param array $args
     *   `domain`, `path` and `network_id`, as WordPress's factory takes them.
     *
     * @return int
     *   The new network's id.
     */
    public function create($args = array())
    {
        global $wpdb;
        $id = (int) ($args['network_id'] ?? self::$next);
        self::$next = max(self::$next, $id) + 1;
        if (is_multisite() && !empty($wpdb->site))
        {
            $wpdb->insert(
                $wpdb->site,
                array(
                    'id' => $id,
                    'domain' => $args['domain'] ?? 'example.org',
                    'path' => $args['path'] ?? "/testpath$id/",
                )
            );
        }
        return $id;
    }
}
