//! The spec's replacement test: a PHP implementation of the post repository, bound in the container at
//! `SpeedySpec\WpPosts\PostRepository`, replaces `wp_posts` for the use cases.

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
    public function insert($row, $import_id) { return null; }
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
    let (_release, root) = support::prepared("posts-read-php-port");
    let (php, _container) = support::boot(&url);
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
    if !ok {
        std::process::exit(1);
    }
}
