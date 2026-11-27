//! The native post reading functions from PHP over MySQL: a callback's exception reaches the caller as the same
//! object, with nothing cached after it, and the values callbacks return cross back as PHP gave them. PHP boots once
//! per process (`harness = false`); each case runs in a fresh request over compat/posts-read/setup.php's seed.

mod support;

use std::rc::Rc;

use speedyspec_cqrs::{QueryHandler, bind_query_handler};
use speedyspec_php_ext::Value;
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts::GetPostStatus;
use speedyspec_wp_posts_contracts::PostError;
use support::yes;

/// A `get_post_status()` handler bound in place of the default one that ignores the post it is asked about and reads
/// the current post's `ID` with a nested `get_post_field( 'ID' )`.
struct IgnoresThePost;

impl QueryHandler<GetPostStatus> for IgnoresThePost {
    fn handle(&self, _query: GetPostStatus) -> Result<PhpValue, PostError> {
        let id = speedyspec_php::call(
            &Value::Str("get_post_field".into()),
            &[Value::Str("ID".into())],
        );
        Ok(match id {
            Ok(Value::Int(i)) => PhpValue::Int(i),
            other => PhpValue::Str(format!("{other:?}").into_bytes()),
        })
    }
}

fn main() {
    let Some(url) = support::mysql("posts_read_php") else {
        return;
    };
    let (_release, root) = support::prepared("posts-read-php-port");
    let (php, container) = support::boot(&url);
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
    // On the edit screen get_sample_permalink() filters the global `sample` and changes its status: with no
    // argument get_post_status() reads the post again by its ID; given the object, it uses it as it is.
    ok &= yes(
        &php,
        &setup,
        "sample_global_is_read_again",
        r#"$GLOBALS['post'] = get_post(1);
        $GLOBALS['post']->filter = 'sample';
        $GLOBALS['post']->post_status = 'draft';
        return 'publish' === get_post_status() && 'draft' === get_post_status($GLOBALS['post'])
            ? true : [get_post_status(), get_post_status($GLOBALS['post'])];"#,
    );
    // The pings, the enclosures, is_sticky() and _page_traverse_name() run through their handlers.
    ok &= yes(
        &php,
        &setup,
        "pings_enclosures_sticky_and_traverse",
        r#"add_post_meta(1, 'enclosure', "http://example.org/a.mp3\n12\naudio/mpeg");
        $seen = [];
        add_filter('is_sticky', function ($sticky, $id) use (&$seen) { $seen[] = [$sticky, $id]; return $sticky; }, 10, 2);
        is_sticky();
        $GLOBALS['post'] = get_post(4);
        is_sticky();
        is_sticky('-7');
        $a = (object) ['ID' => 30, 'post_name' => 'x'];
        $b = (object) ['ID' => 31, 'post_name' => 'y'];
        $children = [0 => [$a], 30 => [$b]];
        $result = [5 => 'kept'];
        _page_traverse_name(0, $children, $result);
        $out = [get_pung(1), get_to_ping(1), get_enclosed(1), get_pung(999), $seen, $result];
        return [[''], [], ['http://example.org/a.mp3'], false, [[false, false], [false, 4], [false, 7]],
            [5 => 'kept', 30 => 'x', 31 => 'y']] === $out ? true : $out;"#,
    );
    // A handler bound in place of the default one never reads the object it was handed: the nested read inside it
    // sees the global, not that object (the hand-over is scoped to its dispatch).
    bind_query_handler::<GetPostStatus>(&container, |_, _| {
        Ok(Rc::new(IgnoresThePost) as Rc<dyn QueryHandler<GetPostStatus>>)
    });
    ok &= yes(
        &php,
        &setup,
        "handed_object_does_not_leak_into_nested_reads",
        r#"$GLOBALS['post'] = get_post(1);
        $status = get_post_status(get_post(2));
        return 1 === $status ? true : $status;"#,
    );

    if !ok {
        std::process::exit(1);
    }
}
