<?php

declare(strict_types=1);

/**
 * Stands in for WordPress's test factory (tests/phpunit/includes/factory): networks, for the option tests; users, for
 * the locale tests; posts, for the routing tests; and, for the meta tests, posts, users, comments, terms and blogs
 * stored as rows.
 *
 * WordPress's network factory calls `populate_network()` (wp-admin's schema, with users and roles), which is not
 * loaded here; the option tests only need a network id that is not the current one. Sites and users are not made:
 * the one option test that creates them (`Tests_Option_Multisite::test_with_another_site`) is excluded.
 *
 * With `SPEEDYSPEC_FACTORY_ROWS` defined (the meta tests' bootstrap), each object is the minimal row its table needs,
 * written through `$wpdb->insert` (the post's type and parent, the comment's post, the term and its taxonomy, the
 * user's login, the blog's path): what `get_object_subtype()` and the meta functions read. WordPress's insert
 * functions (`wp_insert_post()` and the rest) are not run.
 */
final class WP_UnitTest_Factory
{
    /** @var WP_UnitTest_Factory_For_Network */
    public $network;

    /** @var WP_UnitTest_Factory_For_User */
    public $user;

    /** @var WP_UnitTest_Factory_For_Post */
    public $post;

    /** @var WP_UnitTest_Factory_For_Comment */
    public $comment;

    /** @var WP_UnitTest_Factory_For_Term */
    public $term;

    /** @var WP_UnitTest_Factory_For_Blog */
    public $blog;

    public function __construct()
    {
        $this->network = new WP_UnitTest_Factory_For_Network();
        $this->user = new WP_UnitTest_Factory_For_User();
        $this->post = new WP_UnitTest_Factory_For_Post();
        $this->comment = new WP_UnitTest_Factory_For_Comment();
        $this->term = new WP_UnitTest_Factory_For_Term();
        $this->blog = new WP_UnitTest_Factory_For_Blog();
    }

    /**
     * Whether objects are stored as rows (`SPEEDYSPEC_FACTORY_ROWS`).
     */
    public static function stores_rows(): bool
    {
        return defined('SPEEDYSPEC_FACTORY_ROWS') && SPEEDYSPEC_FACTORY_ROWS;
    }

    /**
     * Inserts one row and returns its id.
     *
     * @param string $table
     *   The table.
     * @param array<string, mixed> $row
     *   The columns.
     *
     * @return int
     *   `$wpdb->insert_id`.
     */
    public static function insert(string $table, array $row): int
    {
        global $wpdb;
        if (false === $wpdb->insert($table, $row))
        {
            throw new RuntimeException("The factory could not insert into $table: {$wpdb->last_error}");
        }
        return (int) $wpdb->insert_id;
    }
}

/**
 * Makes users through `wp_insert_user()`, which the suite that needs users provides (for the locale tests, the
 * in-memory stand-in in includes/i18n-doubles.php), or as a `wp_users` row (login, nicename and email; no role).
 */
final class WP_UnitTest_Factory_For_User
{
    private static int $next = 1;

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
        if (!WP_UnitTest_Factory::stores_rows())
        {
            return (int) wp_insert_user($args);
        }
        global $wpdb;
        $n = self::$next++;
        $login = $args['user_login'] ?? "User $n";
        return WP_UnitTest_Factory::insert($wpdb->users, array(
            'user_login' => $login,
            'user_nicename' => sanitize_title($login),
            'user_email' => $args['user_email'] ?? "user_$n@example.org",
            'user_registered' => '2026-01-01 00:00:00',
            'display_name' => $login,
        ));
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

/**
 * Hands out post ids without storing posts: posts are not ported, and a test class whose `wpSetUpBeforeClass()`
 *  makes posts it never reads (the rewrite endpoint tests, whose tests that visit posts are excluded) still runs.
 *  With rows, it writes a `wp_posts` row (type, parent, title and status).
 */
final class WP_UnitTest_Factory_For_Post
{
    private static int $next = 1;

    /**
     * Gives the next post id.
     *
     * @param array $args
     *   Ignored: nothing is stored.
     *
     * @return int
     *   A new post id.
     */
    public function create($args = array())
    {
        if (!WP_UnitTest_Factory::stores_rows())
        {
            return self::$next++;
        }
        global $wpdb;
        $n = self::$next++;
        return WP_UnitTest_Factory::insert($wpdb->posts, array(
            'post_type' => $args['post_type'] ?? 'post',
            'post_parent' => (int) ($args['post_parent'] ?? 0),
            'post_status' => $args['post_status'] ?? 'publish',
            'post_title' => $args['post_title'] ?? "Post title $n",
            'post_name' => "post-title-$n",
            'post_content' => '',
            'post_excerpt' => '',
            'to_ping' => '',
            'pinged' => '',
            'post_content_filtered' => '',
            'post_date' => '2026-01-01 00:00:00',
            'post_date_gmt' => '2026-01-01 00:00:00',
            'post_modified' => '2026-01-01 00:00:00',
            'post_modified_gmt' => '2026-01-01 00:00:00',
        ));
    }
}

/**
 * Makes comments as `wp_comments` rows (the post, the type and the content), for the meta tests.
 */
final class WP_UnitTest_Factory_For_Comment
{
    /**
     * Creates a comment and returns its id.
     *
     * @param array $args
     *   `comment_post_ID`, `comment_type` and `comment_content`, as WordPress's factory takes them.
     *
     * @return int
     *   The new comment's id.
     */
    public function create($args = array())
    {
        global $wpdb;
        return WP_UnitTest_Factory::insert($wpdb->comments, array(
            'comment_post_ID' => (int) ($args['comment_post_ID'] ?? 0),
            'comment_author' => 'Commenter',
            'comment_content' => $args['comment_content'] ?? 'This is a comment',
            'comment_type' => $args['comment_type'] ?? 'comment',
            'comment_date' => '2026-01-01 00:00:00',
            'comment_date_gmt' => '2026-01-01 00:00:00',
        ));
    }
}

/**
 * Makes terms as a `wp_terms` row and its `wp_term_taxonomy` row (`post_tag` unless another taxonomy is given), for
 *  the meta tests.
 */
final class WP_UnitTest_Factory_For_Term
{
    private static int $next = 1;

    /**
     * Creates a term and returns its id.
     *
     * @param array $args
     *   `taxonomy` and `name`, as WordPress's factory takes them.
     *
     * @return int
     *   The new term's id.
     */
    public function create($args = array())
    {
        global $wpdb;
        $n = self::$next++;
        $name = $args['name'] ?? "Term $n";
        $id = WP_UnitTest_Factory::insert($wpdb->terms, array('name' => $name, 'slug' => sanitize_title($name)));
        WP_UnitTest_Factory::insert($wpdb->term_taxonomy, array(
            'term_id' => $id,
            'taxonomy' => $args['taxonomy'] ?? 'post_tag',
            'description' => '',
        ));
        return $id;
    }

    /**
     * Creates `$count` terms and returns their ids.
     *
     * @param int $count
     *   How many.
     * @param array $args
     *   As for {@see create()}.
     *
     * @return int[]
     *   The new terms' ids.
     */
    public function create_many($count, $args = array())
    {
        $ids = array();
        for ($i = 0; $i < $count; $i++)
        {
            $ids[] = $this->create($args);
        }
        return $ids;
    }
}

/**
 * Makes sites as `wp_blogs` rows on the current network, for the meta tests on multisite. WordPress's
 *  `wp_insert_site()` (which also makes the site's tables) is not run.
 */
final class WP_UnitTest_Factory_For_Blog
{
    private static int $next = 1;

    /**
     * Creates a site and returns its id.
     *
     * @param array $args
     *   `domain` and `path`, as WordPress's factory takes them.
     *
     * @return int
     *   The new site's id.
     */
    public function create($args = array())
    {
        global $wpdb;
        $n = self::$next++;
        return WP_UnitTest_Factory::insert($wpdb->blogs, array(
            'site_id' => get_current_network_id(),
            'domain' => $args['domain'] ?? 'example.org',
            'path' => $args['path'] ?? "/testpath$n/",
            'registered' => '2026-01-01 00:00:00',
            'last_updated' => '2026-01-01 00:00:00',
        ));
    }
}
