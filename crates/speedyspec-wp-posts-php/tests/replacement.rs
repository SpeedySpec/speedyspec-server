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
