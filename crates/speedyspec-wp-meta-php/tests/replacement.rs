//! The spec's replacement test: a PHP implementation of the meta repositories, bound in the container at
//! `SpeedySpec\WpMeta\MetaRepositories`, replaces the SQL tables for the use cases.

mod support;

use support::{eval, yes};

fn main() {
    let Some(url) = support::mysql("metadata_php") else {
        return;
    };
    let booted = support::boot(&url);
    eval(support::WORDPRESS).unwrap();
    eval(
        r#"
        final class MemoryMetaRepository implements SpeedySpec\WpMeta\MetaRepository
        {
            public array $rows = [];
            public function engine() { return 'memory'; }
            public function count_for($object_id, $meta_key) { return count(array_filter($this->rows, fn ($r) => $r['object_id'] === $object_id && $r['meta_key'] === $meta_key)); }
            public function add($object_id, $meta_key, $meta_value) { $id = count($this->rows) + 1; $this->rows[$id] = ['meta_id' => $id, 'object_id' => $object_id, 'meta_key' => $meta_key, 'meta_value' => $meta_value]; return $id; }
            public function ids_for($object_id, $meta_key) { return []; }
            public function update_by_key($object_id, $meta_key, $meta_value, $previous) { return 0; }
            public function ids_matching($meta_key, $object_id, $meta_value) { return []; }
            public function objects_with($meta_key, $meta_value) { return []; }
            public function delete_ids($meta_ids) { return 0; }
            public function by_mid($meta_id) { return $this->rows[$meta_id] ?? null; }
            public function update_by_mid($meta_id, $meta_key, $meta_value) { return 0; }
            public function delete_by_mid($meta_id) { return 0; }
            public function for_objects($object_ids) { return array_values(array_map(fn ($r) => ['object_id' => $r['object_id'], 'meta_key' => $r['meta_key'], 'meta_value' => $r['meta_value']], array_filter($this->rows, fn ($r) => in_array($r['object_id'], $object_ids, true)))); }
            public function ids_of_object($object_id) { return array_values(array_map(fn ($r) => $r['meta_id'], array_filter($this->rows, fn ($r) => $r['object_id'] === $object_id))); }
        }
        final class MemoryMetaRepositories implements SpeedySpec\WpMeta\MetaRepositories
        {
            public static ?MemoryMetaRepository $post = null;
            public function for_type($meta_type) { return 'post' === $meta_type ? (self::$post ??= new MemoryMetaRepository()) : null; }
        }
        return true;"#,
    )
    .unwrap();
    yes(
        "php_repository_is_used",
        r#"global $wpdb;
        SpeedySpec\Container\Container::getInstance()->bind('SpeedySpec\WpMeta\MetaRepositories', fn () => new MemoryMetaRepositories(), true);
        $mid = add_metadata('post', 1, 'k', 'v');
        $rows = (int) $wpdb->get_var('SELECT COUNT(*) FROM wp_postmeta');
        $mine = MemoryMetaRepositories::$post->rows;
        wp_cache_flush();
        return $mid === 1 && 0 === $rows && $mine[1]['meta_value'] === 'v'
            && get_metadata('post', 1, 'k', true) === 'v' && add_metadata('comment', 1, 'k', 'v') === false;"#,
    );
    // A binding that fails reaches the caller; nothing falls back to the SQL tables.
    yes(
        "failed_binding_is_surfaced",
        r#"global $wpdb;
        SpeedySpec\Container\Container::getInstance()->bind('SpeedySpec\WpMeta\MetaRepositories', function () { throw new RuntimeException('no repositories'); });
        try { add_metadata('post', 2, 'k', 'v'); $caught = null; } catch (Throwable $e) { $caught = $e; }
        $rows = (int) $wpdb->get_var('SELECT COUNT(*) FROM wp_postmeta');
        return $caught instanceof SpeedySpec\Container\ContainerException
            && str_contains($caught->getMessage(), 'no repositories') && 0 === $rows ? true : [$caught, $rows];"#,
    );
    // A PHP binding lasts for its request: the next request writes to the SQL tables again.
    booted.php.restart_request().expect("a new request");
    eval(support::WORDPRESS).unwrap();
    yes(
        "binding_ends_with_the_request",
        r#"global $wpdb;
        $mid = add_metadata('post', 1, 'k', 'v');
        return is_int($mid) && 1 === (int) $wpdb->get_var('SELECT COUNT(*) FROM wp_postmeta')
            && !class_exists('MemoryMetaRepositories', false);"#,
    );
}
