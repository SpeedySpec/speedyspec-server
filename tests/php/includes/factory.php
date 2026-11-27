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

    /** @var WP_UnitTest_Factory_For_Attachment */
    public $attachment;

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
        $this->attachment = new WP_UnitTest_Factory_For_Attachment();
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
        $id = WP_UnitTest_Factory::insert($wpdb->users, array(
            'user_login' => $login,
            'user_nicename' => sanitize_title($login),
            'user_email' => $args['user_email'] ?? "user_$n@example.org",
            'user_registered' => '2026-01-01 00:00:00',
            'display_name' => $login,
        ));
        // With `SPEEDYSPEC_FACTORY_ROLES` (a bootstrap with WordPress's roles), the user gets the role as
        // wp_insert_user() gives it.
        if (defined('SPEEDYSPEC_FACTORY_ROLES') && SPEEDYSPEC_FACTORY_ROLES && !empty($args['role']))
        {
            (new WP_User($id))->set_role($args['role']);
        }
        return $id;
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
 *  With rows, it writes a `wp_posts` row with every column the post tests read (author, type, status, name, title,
 *  content, dates, mime type, menu order, parent, pings), from the arguments or the defaults, sanitized for the
 *  database as `wp_insert_post()` does. Unlike WordPress's factory, content and excerpt default to empty, the date to
 *  a fixed one (its GMT date the same), and the status is not adjusted for a future date.
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
        $title = $args['post_title'] ?? "Post title $n";
        $date = empty($args['post_date']) ? '2026-01-01 00:00:00' : $args['post_date'];
        $row = array(
            'post_author' => 0,
            'post_type' => 'post',
            'post_parent' => 0,
            'post_status' => 'publish',
            'post_title' => $title,
            'post_name' => sanitize_title($title),
            'post_content' => '',
            'post_excerpt' => '',
            'post_mime_type' => '',
            'menu_order' => 0,
            'to_ping' => '',
            'pinged' => '',
            'post_content_filtered' => '',
            'post_date' => $date,
            'post_date_gmt' => $date,
            'post_modified' => $date,
            'post_modified_gmt' => $date,
        );
        // Every other column the test gives is written as it is, as wp_insert_post() would store it unfiltered.
        $columns = array('post_author', 'post_type', 'post_parent', 'post_status', 'post_name', 'post_content',
            'post_excerpt', 'post_mime_type', 'menu_order', 'to_ping', 'pinged', 'post_date_gmt', 'post_modified',
            'post_modified_gmt', 'post_password', 'comment_status', 'ping_status', 'guid', 'comment_count');
        foreach (array_intersect_key($args, array_flip($columns)) as $column => $value)
        {
            $row[$column] = $value;
        }
        $row['post_parent'] = (int) $row['post_parent'];
        $row['menu_order'] = (int) $row['menu_order'];
        // wp_insert_post() sanitizes the fields for the database (the `*_save_pre` filters) and unslashes them.
        if (function_exists('sanitize_post'))
        {
            $row = wp_unslash(sanitize_post($row, 'db'));
            unset($row['ID'], $row['filter']);
        }
        $id = WP_UnitTest_Factory::insert($wpdb->posts, $row);
        // wp_insert_post() cleans the post's caches (bumping `last_changed`) and reads the post back, which caches it.
        if (function_exists('clean_post_cache'))
        {
            clean_post_cache($id);
            get_post($id);
        }
        return $id;
    }

    /**
     * Creates a post and reads it back with {@link get_post()}, as WordPress's factory does.
     *
     * @param array $args
     *   As for {@see create()}.
     *
     * @return WP_Post|null
     *   The new post.
     */
    public function create_and_get($args = array())
    {
        return get_post($this->create($args));
    }

    /**
     * Creates `$count` posts and returns their ids.
     *
     * @param int $count
     *   How many.
     * @param array $args
     *   As for {@see create()}.
     *
     * @return int[]
     *   The new posts' ids.
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
 * Makes attachments through WordPress's own `wp_insert_attachment()`, as WordPress's attachment factory does
 *  (tests/phpunit/includes/factory/class-wp-unittest-factory-for-attachment.php), for a suite that loads the post
 *  write path. Uploads (`create_upload_object()`) are not made.
 */
final class WP_UnitTest_Factory_For_Attachment
{
    /**
     * Creates an attachment and returns its id.
     *
     * @param array|string $args
     *   What wp_insert_attachment() takes, with `file` and `post_parent`; a file path in the legacy form.
     * @param int $legacy_parent
     *   The parent, in the legacy form.
     * @param array $legacy_args
     *   The arguments, in the legacy form.
     *
     * @return int
     *   The new attachment's id.
     */
    public function create_object($args, $legacy_parent = 0, $legacy_args = array())
    {
        if (is_string($args))
        {
            $file = $args;
            $args = $legacy_args;
            $args['post_parent'] = $legacy_parent;
            $args['file'] = $file;
        }
        $r = array_merge(array('file' => '', 'post_parent' => 0), $args);
        $id = wp_insert_attachment($r, $r['file'], $r['post_parent'], true);
        if (is_wp_error($id) || !$id)
        {
            $message = is_wp_error($id) ? $id->get_error_message() : 'no id';
            throw new RuntimeException("Unable to create the attachment: $message");
        }
        return $id;
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
