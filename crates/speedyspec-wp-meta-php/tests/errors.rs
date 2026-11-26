//! The native meta functions from PHP over MySQL: exceptions reach the caller unchanged, and the wiring the use
//! cases rely on (the unfiltered protection rule, the lazy loader class, values that are objects). PHP boots once per
//! process (`harness = false`).

mod support;

use support::{check, eval, yes};

const MORE: &str = r#"
class WP_Error {
    public $code; public $message;
    public function __construct($code = '', $message = '') { $this->code = $code; $this->message = $message; }
    public function get_error_code() { return $this->code; }
    public function get_error_message() { return $this->message; }
}
class WP_Term {}
function get_term($term) { return null; }
global $wpdb;
$wpdb->query('DROP TABLE IF EXISTS wp_termmeta');
$wpdb->query("CREATE TABLE wp_termmeta (meta_id bigint(20) unsigned NOT NULL auto_increment, term_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY (meta_id), KEY term_id (term_id)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
return true;
"#;

fn main() {
    let Some(url) = support::mysql("metadata_php") else {
        return;
    };
    let _booted = support::boot(&url);
    eval(support::WORDPRESS).unwrap();
    eval(MORE).unwrap();

    // Review Focus 5: the same exception object, and nothing written after it.
    yes(
        "filter_exception_is_rethrown",
        r#"global $wpdb;
        $thrown = new RuntimeException('from add_post_metadata');
        $cb = function () use ($thrown) { throw $thrown; };
        add_filter('add_post_metadata', $cb);
        try { add_metadata('post', 1, 'k', 'v'); $caught = null; } catch (RuntimeException $e) { $caught = $e; }
        remove_filter('add_post_metadata', $cb);
        $rows = (int) $wpdb->get_var("SELECT COUNT(*) FROM wp_postmeta WHERE meta_key = 'k'");
        return $caught === $thrown && 0 === $rows;"#,
    );
    // A `query` callback's exception makes $wpdb->insert answer false; add_metadata() still rethrows it.
    yes(
        "query_exception_is_rethrown",
        r#"global $wpdb;
        $thrown = new LogicException('from query');
        $cb = function ($sql) use ($thrown) { if (str_starts_with($sql, 'INSERT')) { throw $thrown; } return $sql; };
        add_filter('query', $cb);
        try { $r = add_metadata('post', 1, 'q', 'v'); $caught = $r; } catch (LogicException $e) { $caught = $e; }
        remove_filter('query', $cb);
        $rows = (int) $wpdb->get_var("SELECT COUNT(*) FROM wp_postmeta WHERE meta_key = 'q'");
        return $caught === $thrown && 0 === $rows;"#,
    );
    // The protection rule bound in MetaDeps is the unfiltered one: one filter run per call.
    yes(
        "is_protected_meta_filter_runs_once",
        r#"$runs = 0;
        add_filter('is_protected_meta', function ($p) use (&$runs) { $runs++; return $p; });
        $a = is_protected_meta('_secret', 'post');
        $one = $runs;
        $b = is_protected_meta('open', 'post');
        register_meta('post', 'fallback', array());
        return $a === true && $b === false && $one === 1 && $runs === 3;"#,
    );
    yes(
        "is_protected_meta_exception_is_rethrown_by_register_meta",
        r#"$thrown = new DomainException('protected?');
        $cb = function () use ($thrown) { throw $thrown; };
        add_filter('is_protected_meta', $cb);
        try { register_meta('post', 'guarded', array()); $caught = null; } catch (DomainException $e) { $caught = $e; }
        remove_filter('is_protected_meta', $cb);
        return $caught === $thrown && !registered_meta_key_exists('post', 'guarded');"#,
    );
    yes(
        "by_mid_unserializes_the_value",
        r#"$mid = add_metadata('post', 3, 'arr', array('a' => 1));
        $row = get_metadata_by_mid('post', $mid);
        return is_object($row) && $row->meta_value === array('a' => 1) && $row->meta_key === 'arr'
            && $row->post_id === '3' && $row->meta_id === (string) $mid
            && array_keys(get_object_vars($row)) === array('meta_id', 'post_id', 'meta_key', 'meta_value');"#,
    );
    yes(
        "objects_round_trip",
        r#"add_metadata('post', 4, 'obj', (object) array('a' => 1));
        $v = get_metadata('post', 4, 'obj', true);
        wp_cache_flush();
        $w = get_metadata('post', 4, 'obj', true);
        $seen = null;
        $cb = function ($value) use (&$seen) { $seen = $value; return $value; };
        add_filter('sanitize_post_meta_obj2', $cb);
        add_metadata('post', 4, 'obj2', new ArrayObject(array(1)));
        remove_filter('sanitize_post_meta_obj2', $cb);
        return $v instanceof stdClass && $v->a === 1 && $w instanceof stdClass && $seen instanceof ArrayObject;"#,
    );
    yes(
        "registered_callbacks_keep_their_identity",
        r#"$cb = function ($value) { return strtoupper($value); };
        register_meta('post', 'color', array('sanitize_callback' => $cb, 'single' => true, 'default' => 'red'));
        $keys = get_registered_meta_keys('post');
        $ok = [$keys['color']['sanitize_callback'] === $cb, sanitize_meta('color', 'blue', 'post'),
            get_metadata('post', 99, 'color', true)];
        unregister_meta_key('post', 'color');
        $ok[] = registered_meta_key_exists('post', 'color'); $ok[] = sanitize_meta('color', 'blue', 'post');
        $ok[] = isset($GLOBALS['wp_meta_keys']['post']['']['color']);
        return $ok === [true, 'BLUE', 'red', false, 'blue', false] ? true : $ok;"#,
    );
    yes(
        "custom_type_table_from_a_dynamic_property",
        r#"global $wpdb; $wpdb->thingmeta = 'wp_thingmeta';
        return _get_meta_table('thing') === 'wp_thingmeta' && _get_meta_table('nothing') === false
            && _get_meta_table('post') === 'wp_postmeta';"#,
    );
    // The lazy loader class: the callback is hooked (10, 5) before the queued-objects action fires, the read object's
    // id is loaded with the queue, and the callback is unhooked after the load.
    yes(
        "lazy_loader_hooks_loads_and_unhooks",
        r#"add_metadata('term', 1, 'k', 'a'); add_metadata('term', 2, 'k', 'b'); add_metadata('term', 5, 'k', 'c');
        wp_cache_flush();
        $loader = wp_metadata_lazyloader();
        $hooked = null;
        $cb = function ($ids, $type, $l) use (&$hooked) { $hooked = has_filter('get_term_metadata', array($l, 'lazyload_meta_callback')); };
        add_action('metadata_lazyloader_queued_objects', $cb, 10, 3);
        $loader->queue_objects('term', array(1, 2));
        remove_action('metadata_lazyloader_queued_objects', $cb, 10);
        $v = get_metadata('term', 5, 'k', true);
        return $hooked === 10 && $v === 'c'
            && false !== wp_cache_get(1, 'term_meta') && false !== wp_cache_get(2, 'term_meta')
            && false === has_filter('get_term_metadata', array($loader, 'lazyload_meta_callback'));"#,
    );
    yes(
        "lazy_loader_rejects_unknown_types",
        r#"$loader = new WP_Metadata_Lazyloader();
        $a = $loader->queue_objects('post', array(1));
        $b = $loader->reset_queue('post');
        return $a instanceof WP_Error && $a->get_error_code() === 'invalid_object_type'
            && $a->get_error_message() === 'Invalid object type.' && $b instanceof WP_Error
            && false === has_filter('get_post_metadata');"#,
    );
    yes(
        "lazy_hook_error_is_rethrown",
        r#"$thrown = new UnexpectedValueException('queued');
        $cb = function () use ($thrown) { throw $thrown; };
        add_action('metadata_lazyloader_queued_objects', $cb);
        try { (new WP_Metadata_Lazyloader())->queue_objects('comment', array(1)); $caught = null; }
        catch (UnexpectedValueException $e) { $caught = $e; }
        remove_action('metadata_lazyloader_queued_objects', $cb);
        return $caught === $thrown;"#,
    );
    check("done", true, ());
}
