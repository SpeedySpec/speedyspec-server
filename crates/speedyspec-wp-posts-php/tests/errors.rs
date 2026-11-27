//! The native post reading functions from PHP over MySQL: a callback's exception reaches the caller as the same
//! object, with nothing cached after it, and the values callbacks return cross back as PHP gave them. PHP boots once
//! per process (`harness = false`); each case runs in a fresh request over compat/posts-read/setup.php's seed.

mod support;

use support::yes;

fn main() {
    let Some(url) = support::mysql("posts_read_php") else {
        return;
    };
    let (_release, root) = support::prepared("posts-read-php-port");
    let (php, _container) = support::boot(&url);
    let setup = support::setup_code(&root);
    let mut ok = true;

    // A `query` callback's exception during get_post()'s read: wpdb hides it behind an empty answer, and the native
    // function still rethrows it; the miss is not cached.
    ok &= yes(
        &php,
        &setup,
        "query_exception_during_get_post_is_rethrown",
        r#"$thrown = new LogicException('from query');
        $cb = function ($sql) use ($thrown) { if (str_contains($sql, 'WHERE ID = 1 ')) { throw $thrown; } return $sql; };
        add_filter('query', $cb);
        try { $r = get_post(1); $caught = $r; } catch (LogicException $e) { $caught = $e; }
        remove_filter('query', $cb);
        return $caught === $thrown && false === wp_cache_get(1, 'posts') ? true : [$caught, wp_cache_get(1, 'posts')];"#,
    );
    // clean_post_cache(): the action throws after the post's entries are gone; `last_changed` is not bumped.
    ok &= yes(
        &php,
        &setup,
        "clean_post_cache_action_exception_is_rethrown",
        r#"get_post(2);
        $before = wp_cache_get('last_changed', 'posts');
        $thrown = new RuntimeException('from clean_post_cache');
        add_action('clean_post_cache', function () use ($thrown) { throw $thrown; });
        try { clean_post_cache(2); $caught = null; } catch (RuntimeException $e) { $caught = $e; }
        return $caught === $thrown && false === wp_cache_get(2, 'posts')
            && $before === wp_cache_get('last_changed', 'posts')
            && false === wp_cache_get('all_page_ids', 'posts') ? true : [$caught, $before, wp_cache_get('last_changed', 'posts')];"#,
    );
    // get_post_status() on an attachment asks for its parent's status first: a `get_post_status` callback that throws
    // there stops the work before the attachment's own filter, and the read of the parent stays cached as WordPress
    // leaves it (nothing is written after the throw).
    ok &= yes(
        &php,
        &setup,
        "get_post_status_exception_is_rethrown",
        r#"$thrown = new DomainException('from get_post_status');
        $seen = [];
        add_filter('get_post_status', function ($status, $post) use ($thrown, &$seen) { $seen[] = $post->ID; throw $thrown; }, 10, 2);
        try { get_post_status(9); $caught = null; } catch (DomainException $e) { $caught = $e; }
        return $caught === $thrown && [1] === $seen && false === wp_cache_get('last_changed', 'posts')
            ? true : [$caught, $seen];"#,
    );
    // A port's exception (format_to_edit's filter, through the Formatting adapter) reaches the caller unchanged.
    ok &= yes(
        &php,
        &setup,
        "formatting_exception_is_rethrown",
        r#"$thrown = new UnexpectedValueException('from format_to_edit');
        add_filter('format_to_edit', function () use ($thrown) { throw $thrown; });
        try { get_post_field('post_content', 1, 'edit'); $caught = null; } catch (UnexpectedValueException $e) { $caught = $e; }
        return $caught === $thrown;"#,
    );
    // A live object a filter returns crosses back as the very object; a stdClass crosses as an equal copy, which
    // wp_count_posts() then fills.
    ok &= yes(
        &php,
        &setup,
        "objects_returned_by_callbacks_cross_back",
        r#"$mine = new ArrayObject(['x' => 1]);
        add_filter('get_post_status', fn () => $mine);
        $status = get_post_status(1);
        add_filter('wp_count_posts', fn ($counts) => (object) ['publish' => 'many']);
        $counts = wp_count_posts('page');
        add_filter('get_page_uri', fn ($uri, $page) => [$uri, $page], 10, 2);
        [$uri, $page] = get_page_uri(4);
        return $status === $mine && $counts instanceof stdClass && 'many' === $counts->publish
            && 'about/team/lead' === $uri && $page instanceof WP_Post && 4 === $page->ID
            ? true : [$status, $counts, $uri, $page];"#,
    );
    // get_post( 0, OBJECT, 'display' ) sanitizes the global WP_Post in place, as WordPress does.
    ok &= yes(
        &php,
        &setup,
        "display_sanitizes_the_global_in_place",
        r#"$GLOBALS['post'] = get_post(1);
        $global = $GLOBALS['post'];
        $out = get_post(0, OBJECT, 'display');
        return $out === $global && 'display' === $global->filter
            && 'Hello &#8220;World&#8221; &amp; <b>friends</b>' !== $global->post_title ? true : [$out, $global];"#,
    );

    if !ok {
        std::process::exit(1);
    }
}
