//! The spec's replacement test: a PHP implementation of the post repository, bound in the container at
//! `SpeedySpec\WpPosts\PostRepository`, replaces `wp_posts` for the use cases; and PHP implementations of the
//! writers' ports (rewrite, comments, cron, users, taxonomy, charset, revisions, uploads), each bound at its twin's
//! key, replace WordPress's functions for `wp_insert_post()`, the transitions and `wp_delete_post()`.

mod support;

use support::yes;

const MEMORY: &str = r#"
final class MemoryPostRepository implements SpeedySpec\WpPosts\PostRepository
{
    public static array $calls = [];
    public array $rows = [
        5 => ['ID' => 5, 'post_title' => 'From PHP', 'post_name' => 'from-php', 'post_type' => 'page', 'post_status' => 'publish', 'post_parent' => 0],
    ];
    private function seen($method) { self::$calls[] = $method; }
    public function engine() { return 'memory'; }
    public function find($id) { $this->seen('find'); return $this->rows[$id] ?? null; }
    public function insert($row) { return null; }
    public function attachment_slug_owner($slug, $except) { return null; }
    public function hierarchical_slug_owner($slug, $post_type, $except, $parent) { return null; }
    public function flat_slug_owner($slug, $post_type, $except) { return null; }
    public function find_many($ids) { $this->seen('find_many'); return array_values(array_intersect_key($this->rows, array_flip($ids))); }
    public function parent_ids($ids) { return []; }
    public function pages_by_names($names, $post_types) { $this->seen('pages_by_names'); return array_values(array_filter($this->rows, fn ($r) => in_array($r['post_name'], $names, true))); }
    public function all_page_ids() { return [5]; }
    public function count_by_status($post_type, $reader) { return ['publish' => 1]; }
    public function count_attachments($mime_type) { return ['counts' => [], 'trash' => 0]; }
    public function last_post_time($field, $post_types, $timezone) { return null; }
    public function available_mime_types($post_type) { return []; }
    public function update($id, $row) { return 0; }
    public function update_name($id, $name) { return 0; }
    public function update_guid($id, $guid) { return 0; }
    public function update_status($id, $status) { return 0; }
    public function update_pinged($id, $pinged) { return 0; }
    public function update_post_type($id, $post_type) { return 0; }
    public function import_id_taken($id) { return isset($this->rows[$id]); }
    public function raw_row($id) { return $this->rows[$id] ?? null; }
    public function children_of_type($parent, $post_type) { return []; }
    public function reparent_children($from, $to, $post_type) { return 0; }
    public function revision_ids($parent) { return []; }
    public function reparent_attachments($from, $to) { return 0; }
    public function remove($id) { return 0; }
    public function auto_drafts_older_than_a_week() { return []; }
}
"#;

fn main() {
    let Some(url) = support::mysql("posts_read_php") else {
        return;
    };
    let (_release, root) = support::prepared_with("posts-read-php-port", true);
    let (php, _container) = support::boot_with(&url, false, true);
    let setup = support::setup_code(&root);
    let bind = format!(
        "{MEMORY}\nSpeedySpec\\Container\\Container::getInstance()->bind('SpeedySpec\\WpPosts\\PostRepository', fn () => new MemoryPostRepository(), true);\n"
    );
    let mut ok = true;
    ok &= yes(
        &php,
        &setup,
        "php_repository_is_used",
        &format!(
            r#"{bind}
            global $wpdb;
            $before = $wpdb->num_queries;
            $post = get_post(5);
            $page = get_page_by_path('from-php');
            $ids = get_all_page_ids();
            return $post instanceof WP_Post && 'From PHP' === $post->post_title && 5 === $page->ID
                && ['5'] === $ids && $before === $wpdb->num_queries && null === get_post(1)
                && ['find', 'pages_by_names', 'find'] === array_slice(MemoryPostRepository::$calls, 0, 3)
                ? true : [$post, $page, $ids, MemoryPostRepository::$calls];"#
        ),
    );
    // WP_Post::get_instance() (its body swapped in the prepared release) and get_post() of a WP_Post that is not raw
    // read through the bound repository too: get_post() has one read path.
    ok &= yes(
        &php,
        &setup,
        "php_repository_serves_get_instance",
        &format!(
            r#"{bind}
            $direct = WP_Post::get_instance(5);
            $display = new WP_Post((object) ['ID' => 5, 'post_title' => 'stale', 'filter' => 'display']);
            $raw = get_post($display);
            return $direct instanceof WP_Post && 'From PHP' === $direct->post_title && 'raw' === $direct->filter
                && $raw instanceof WP_Post && 'From PHP' === $raw->post_title
                && false === WP_Post::get_instance(0) && false === WP_Post::get_instance('abc')
                && false === WP_Post::get_instance(-3) && false === WP_Post::get_instance(1)
                ? true : [$direct, $raw];"#
        ),
    );
    // The taxonomy port: a PHP object implementing the twin, bound in the container, serves the post reads' term
    // cache calls and WP_Post's term properties.
    ok &= yes(
        &php,
        &setup,
        "php_taxonomy_is_used",
        r#"final class MemoryTaxonomy implements SpeedySpec\WpPosts\Taxonomy
        {
            public static array $calls = [];
            public function update_object_term_cache($object_ids, $object_types) { self::$calls[] = ['update', $object_ids, $object_types]; }
            public function clean_object_term_cache($object_ids, $object_type) { self::$calls[] = ['clean', $object_ids, $object_type]; }
            public function post_term_fields($post, $taxonomy, $field) { self::$calls[] = ['fields', $post->ID, $taxonomy, $field]; return [7, 8]; }
            public function set_post_terms($post_id, $terms, $taxonomy, $append) { return []; }
            public function object_taxonomies($post_type) { return []; }
            public function default_terms($post_type) { return []; }
            public function assign_terms_cap($taxonomy) { return null; }
            public function get_object_terms($post_id, $taxonomy, $args) { return []; }
            public function get_the_terms($post_id, $taxonomy) { return false; }
            public function is_object_in_taxonomy($post_type, $taxonomy) { return false; }
            public function delete_object_term_relationships($post_id, $taxonomies) {}
        }
        SpeedySpec\Container\Container::getInstance()->bind('SpeedySpec\WpPosts\Taxonomy', fn () => new MemoryTaxonomy(), true);
        $category = get_post_field('post_category', 1, 'raw');
        clean_post_cache(1);
        _prime_post_caches([2], true, false);
        $expected = [['fields', 1, 'category', 'term_id'], ['clean', [1], 'post'], ['update', [2], ['page']]];
        return [7, 8] === $category && $expected === MemoryTaxonomy::$calls ? true : [$category, MemoryTaxonomy::$calls];"#,
    );
    // A binding that fails reaches the caller; nothing falls back to the SQL table.
    ok &= yes(
        &php,
        &setup,
        "failed_binding_is_surfaced",
        r#"SpeedySpec\Container\Container::getInstance()->bind('SpeedySpec\WpPosts\PostRepository', function () { throw new RuntimeException('no repository'); });
        try { get_post(1); $caught = null; } catch (Throwable $e) { $caught = $e; }
        return $caught instanceof Error && str_contains($caught->getMessage(), 'no repository') ? true : $caught;"#,
    );
    // A PHP binding lasts for its request: the next request reads wp_posts again.
    ok &= yes(
        &php,
        &setup,
        "binding_ends_with_the_request",
        r#"$post = get_post(1);
        return $post instanceof WP_Post && 'hello-world' === $post->post_name && !class_exists('MemoryPostRepository', false);"#,
    );
    ok &= write_ports(&php, &support::write_setup_code(&root));
    if !ok {
        std::process::exit(1);
    }
}

/// The writers' container ports, each replaced by a PHP object implementing its twin.
fn write_ports(php: &speedyspec_php::Php, setup: &str) -> bool {
    let mut ok = true;
    // Rewrite, comments, charset and the cron port: the GUID is the bound permalink, the comment status the bound
    // default, the emoji check reads the bound charset, and publishing schedules through the bound cron port.
    ok &= yes(
        php,
        setup,
        "php_write_ports_are_used",
        r#"wp_set_current_user(1);
        final class Calls { public static array $seen = []; }
        final class FixedRewrite implements SpeedySpec\WpPosts\Rewrite
        {
            public function permalink($post_id) { Calls::$seen[] = ['permalink', $post_id]; return "http://bound.test/$post_id"; }
        }
        final class ClosedComments implements SpeedySpec\WpPosts\Comments
        {
            public function default_comment_status($post_type, $comment_type) { Calls::$seen[] = ['status', $post_type, $comment_type]; return 'closed'; }
            public function trash_post_comments($post_id) {}
            public function untrash_post_comments($post_id) {}
            public function delete_post_comments($post_id) {}
            public function defer_counting($defer) {}
        }
        final class Utf8Charset implements SpeedySpec\WpPosts\Charset
        {
            public function col_charset($table, $column) { Calls::$seen[] = ['charset', $table, $column]; return 'utf8'; }
        }
        final class RecordedCron implements SpeedySpec\WpPosts\Scheduling
        {
            public function clear_scheduled_hook($hook, $args) { Calls::$seen[] = ['clear', $hook, $args]; return 0; }
            public function schedule_single_event($timestamp, $hook, $args) { Calls::$seen[] = ['schedule', $hook, $args]; return true; }
            public function next_scheduled($hook, $args) { Calls::$seen[] = ['next', $hook]; return false; }
        }
        $c = SpeedySpec\Container\Container::getInstance();
        $c->bind('SpeedySpec\WpPosts\Rewrite', fn () => new FixedRewrite(), true);
        $c->bind('SpeedySpec\WpPosts\Comments', fn () => new ClosedComments(), true);
        $c->bind('SpeedySpec\WpPosts\Charset', fn () => new Utf8Charset(), true);
        $c->bind('SpeedySpec\WpPosts\Scheduling', fn () => new RecordedCron(), true);
        $id = wp_insert_post(['post_title' => "Bound \u{1F600}", 'post_status' => 'publish']);
        $post = get_post($id);
        $kinds = array_values(array_unique(array_map(fn ($c) => $c[0], Calls::$seen)));
        $out = [
            "http://bound.test/$id" === $post->guid, 'closed' === $post->comment_status, 'closed' === $post->ping_status,
            str_contains($post->post_title, '&#x1f600;'),
            in_array(['clear', 'publish_future_post', [$id]], Calls::$seen, true),
            in_array(['schedule', 'do_pings', []], Calls::$seen, true),
            ['status', 'charset', 'permalink', 'clear', 'next', 'schedule'] === $kinds,
        ];
        return !in_array(false, $out, true) ? true : [$out, $post, Calls::$seen];"#,
    );
    // Users and taxonomy: the bound users port decides the author, and the bound taxonomy port receives the
    // categories.
    ok &= yes(
        php,
        setup,
        "php_users_and_taxonomy_are_used",
        r#"final class Seven implements SpeedySpec\WpPosts\Users
        {
            public static array $asked = [];
            public function is_user_logged_in() { return true; }
            public function get_current_user_id() { return 7; }
            public function current_user_can($capability, $args) { self::$asked[] = [$capability, $args]; return true; }
        }
        final class Terms implements SpeedySpec\WpPosts\Taxonomy
        {
            public static array $set = [];
            public function update_object_term_cache($object_ids, $object_types) {}
            public function clean_object_term_cache($object_ids, $object_type) {}
            public function post_term_fields($post, $taxonomy, $field) { return []; }
            public function set_post_terms($post_id, $terms, $taxonomy, $append) { self::$set[] = [$post_id, $terms, $taxonomy, $append]; return []; }
            public function object_taxonomies($post_type) { return ['category']; }
            public function default_terms($post_type) { return []; }
            public function assign_terms_cap($taxonomy) { return 'manage_categories'; }
            public function get_object_terms($post_id, $taxonomy, $args) { return []; }
            public function get_the_terms($post_id, $taxonomy) { return false; }
            public function is_object_in_taxonomy($post_type, $taxonomy) { return 'category' === $taxonomy; }
            public function delete_object_term_relationships($post_id, $taxonomies) {}
        }
        $c = SpeedySpec\Container\Container::getInstance();
        $c->bind('SpeedySpec\WpPosts\Users', fn () => new Seven(), true);
        $c->bind('SpeedySpec\WpPosts\Taxonomy', fn () => new Terms(), true);
        $id = wp_insert_post(['post_title' => 'By seven', 'post_category' => [3]]);
        $post = get_post($id);
        return '7' === $post->post_author && [[$id, [3], 'category', false]] === Terms::$set
            ? true : [$post, Terms::$set, Seven::$asked];"#,
    );
    // Revisions, uploads and comments on delete: wp_delete_post() hands post 1's revision (4) to the bound revisions
    // port and its comments to the bound comments port, and an attachment to the bound uploads port, whose answer it
    // returns as it is.
    ok &= yes(
        php,
        setup,
        "php_delete_ports_are_used",
        r#"final class Seen { public static array $calls = []; }
        final class KeptRevisions implements SpeedySpec\WpPosts\Revisions
        {
            public function save_post_revision($post_id) { Seen::$calls[] = ['save', $post_id]; }
            public function delete_post_revision($revision_id) { Seen::$calls[] = ['revision', $revision_id]; }
        }
        final class KeptComments implements SpeedySpec\WpPosts\Comments
        {
            public function default_comment_status($post_type, $comment_type) { return 'open'; }
            public function trash_post_comments($post_id) {}
            public function untrash_post_comments($post_id) {}
            public function delete_post_comments($post_id) { Seen::$calls[] = ['comments', $post_id]; }
            public function defer_counting($defer) { Seen::$calls[] = ['defer', $defer]; }
        }
        final class KeptUploads implements SpeedySpec\WpPosts\Uploads
        {
            public function insert_attachment($postarr, $wp_error) { return 0; }
            public function delete_attachment($post_id, $force) { Seen::$calls[] = ['attachment', $post_id, $force]; return 'kept'; }
            public function update_attached_file($post_id, $file) {}
            public function attachment_is($kind, $post_id) { return false; }
            public function set_post_thumbnail($post_id, $thumbnail_id) {}
            public function delete_post_thumbnail($post_id) {}
        }
        $c = SpeedySpec\Container\Container::getInstance();
        $c->bind('SpeedySpec\WpPosts\Revisions', fn () => new KeptRevisions(), true);
        $c->bind('SpeedySpec\WpPosts\Comments', fn () => new KeptComments(), true);
        $c->bind('SpeedySpec\WpPosts\Uploads', fn () => new KeptUploads(), true);
        $deleted = wp_delete_post(1, true);
        $attachment = wp_delete_post(5, true);
        global $wpdb;
        $revision = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts WHERE ID = 4");
        $comments = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->comments WHERE comment_post_ID = 1");
        $expected = [['revision', 4], ['defer', true], ['comments', 1], ['defer', false], ['attachment', 5, true]];
        return $deleted instanceof WP_Post && 'kept' === $attachment && $expected === Seen::$calls
            && '1' === $revision && '0' !== $comments ? true : [$deleted, $attachment, Seen::$calls, $revision, $comments];"#,
    );
    // A bound revisions port's exception stops wp_delete_post() at the revision, and a bound uploads port's stops it
    // at the attachment: each reaches the caller as the same object, and the post stays.
    ok &= yes(
        php,
        setup,
        "bound_revisions_and_uploads_exceptions_are_rethrown",
        r#"final class BrokenRevisions implements SpeedySpec\WpPosts\Revisions
        {
            public static $thrown;
            public function save_post_revision($post_id) {}
            public function delete_post_revision($revision_id) { throw self::$thrown; }
        }
        final class BrokenUploads implements SpeedySpec\WpPosts\Uploads
        {
            public static $thrown;
            public function insert_attachment($postarr, $wp_error) { return 0; }
            public function delete_attachment($post_id, $force) { throw self::$thrown; }
            public function update_attached_file($post_id, $file) {}
            public function attachment_is($kind, $post_id) { return false; }
            public function set_post_thumbnail($post_id, $thumbnail_id) {}
            public function delete_post_thumbnail($post_id) {}
        }
        BrokenRevisions::$thrown = new LogicException('from the bound revisions');
        BrokenUploads::$thrown = new RuntimeException('from the bound uploads');
        $c = SpeedySpec\Container\Container::getInstance();
        $c->bind('SpeedySpec\WpPosts\Revisions', fn () => new BrokenRevisions(), true);
        $c->bind('SpeedySpec\WpPosts\Uploads', fn () => new BrokenUploads(), true);
        try { wp_delete_post(1, true); $revision = null; } catch (LogicException $e) { $revision = $e; }
        try { wp_delete_post(5, true); $upload = null; } catch (RuntimeException $e) { $upload = $e; }
        global $wpdb;
        $rows = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts WHERE ID IN (1, 4, 5)");
        return $revision === BrokenRevisions::$thrown && $upload === BrokenUploads::$thrown && '3' === $rows
            ? true : [$revision, $upload, $rows];"#,
    );
    // A bound port's exception reaches the caller as the same object; the row written before it stays.
    ok &= yes(
        php,
        setup,
        "bound_port_exception_is_rethrown",
        r#"final class Broken implements SpeedySpec\WpPosts\Rewrite
        {
            public static $thrown;
            public function permalink($post_id) { throw self::$thrown; }
        }
        Broken::$thrown = new LogicException('from the bound rewrite');
        SpeedySpec\Container\Container::getInstance()->bind('SpeedySpec\WpPosts\Rewrite', fn () => new Broken(), true);
        try { wp_insert_post(['post_title' => 'Permalink thrown', 'post_status' => 'publish']); $caught = null; }
        catch (LogicException $e) { $caught = $e; }
        global $wpdb;
        $row = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts WHERE post_title = 'Permalink thrown'");
        return $caught === Broken::$thrown && '1' === $row ? true : [$caught, $row];"#,
    );
    ok
}
