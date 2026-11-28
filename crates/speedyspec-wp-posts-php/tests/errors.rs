//! The native post reading and writing functions from PHP over MySQL: a callback's exception reaches the caller as
//! the same object, with nothing cached after it, and the values callbacks return cross back as PHP gave them. A
//! writer's row stays written where WordPress has written it before the exception, and is never written where
//! WordPress throws first. PHP boots once per process (`harness = false`); each case runs in a fresh request over
//! compat/posts-read/setup.php's seed, or compat/posts-write/setup.php's for the writers.

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
    let (_release, root) = support::prepared_with("posts-read-php-port", true);
    let (php, container) = support::boot_with(&url, false, true);
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
    // A callback that catches a nested get_post()'s failure: the outer native returns normally, nothing it did not
    // meet is rethrown (wpdb's side channel is emptied by the nested call that took it).
    ok &= yes(
        &php,
        &setup,
        "caught_nested_failure_is_not_rethrown",
        r#"$thrown = new LogicException('from query');
        add_filter('query', function ($sql) use ($thrown) { if (str_contains($sql, 'ID = 999 ')) { throw $thrown; } return $sql; });
        $caught = null;
        add_filter('get_post_status', function ($status) use (&$caught) {
            try { get_post(999); } catch (LogicException $e) { $caught = $e; }
            return $status;
        });
        try { $status = get_post_status(1); } catch (Throwable $e) { $status = $e; }
        return 'publish' === $status && $caught instanceof LogicException ? true : [$status, $caught];"#,
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
    // is_sticky() reads `sticky_posts` through the options contracts (the Rust options API over the same wpdb and
    // object cache): what option.php stored, its `option_` filter, and a `pre_option_` callback's exception, rethrown
    // whole.
    ok &= yes(
        &php,
        &setup,
        "sticky_posts_is_read_through_the_options_api",
        r#"update_option('sticky_posts', [4, '7']);
        $stored = [is_sticky(4), is_sticky(7), is_sticky(5)];
        add_filter('option_sticky_posts', function ($ids) { $ids[] = 5; return $ids; });
        $filtered = is_sticky(5);
        add_filter('pre_option_sticky_posts', function () { throw new LogicException('stop'); });
        try { is_sticky(4); $thrown = null; } catch (LogicException $e) { $thrown = $e->getMessage(); }
        remove_all_filters('pre_option_sticky_posts');
        remove_all_filters('option_sticky_posts');
        delete_option('sticky_posts');
        $out = [$stored, $filtered, $thrown, is_sticky(4)];
        return [[true, true, false], true, 'stop', false] === $out ? true : $out;"#,
    );
    // A warm get_post() builds its WP_Post over the cached entry without copying it (PostObject::Entry): every call
    // is still a new object, a change to one reaches neither the entry nor the next read, and the object is the one
    // `new WP_Post( $entry )` makes. The entry stays a raw stdClass.
    ok &= yes(
        &php,
        &setup,
        "warm_get_post_is_a_new_object_each_time",
        r#"wp_cache_delete(1, 'posts');
        $a = get_post(1);
        $b = get_post(1);
        $c = get_post(1);
        $title = $c->post_title;
        $b->post_title .= ' changed';
        $sibling = $c->post_title === $title && $a->post_title === $title;
        $c->ID = 99;
        $d = get_post(1);
        $entry = wp_cache_get(1, 'posts');
        $out = [
            $sibling, $a !== $b, $b !== $c, $c !== $d, $d instanceof WP_Post, $title === $d->post_title, 1 === $d->ID,
            'raw' === $d->filter, $entry instanceof stdClass, 'raw' === $entry->filter,
            get_object_vars(new WP_Post($entry)) === get_object_vars($d),
            get_object_vars($a) === get_object_vars($d),
            $d->to_array() === get_post(1, ARRAY_A),
        ];
        return !in_array(false, $out, true) ? true : $out;"#,
    );
    // An entry replaced in the same request renders its new content, not the earlier rendering; a warm read asked
    // for 'display' still goes through WP_Post::filter().
    ok &= yes(
        &php,
        &setup,
        "replaced_entry_renders_its_new_content",
        r#"wp_cache_delete(1, 'posts');
        $first = get_post(1)->post_title;
        get_post(1);
        clean_post_cache(1);
        $GLOBALS['wpdb']->update($GLOBALS['wpdb']->posts, ['post_title' => 'Retitled'], ['ID' => 1]);
        $second = get_post(1);
        $third = get_post(1);
        $display = get_post(1, OBJECT, 'display');
        $GLOBALS['wpdb']->update($GLOBALS['wpdb']->posts, ['post_title' => $first], ['ID' => 1]);
        clean_post_cache(1);
        $out = [
            'Retitled' === $second->post_title, 'Retitled' === $third->post_title,
            'display' === $display->filter, 'Retitled' === $display->post_title, $display !== $third,
            'raw' === $third->filter, $first === get_post(1)->post_title,
        ];
        return !in_array(false, $out, true) ? true : $out;"#,
    );
    // An entry another caller cached with a property WP_Post does not declare is read as a copy, extra included.
    ok &= yes(
        &php,
        &setup,
        "cached_entry_with_an_extra_property_is_kept",
        r#"get_post(1);
        $entry = wp_cache_get(1, 'posts');
        $entry->extra = 'x';
        wp_cache_set(1, $entry, 'posts');
        $p = get_post(1);
        $q = get_post(1);
        wp_cache_delete(1, 'posts');
        return 'x' === $p->extra && $p !== $q && 'x' === $q->extra && 'raw' === $p->filter ? true : [$p, $q];"#,
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

    ok &= writes(&php, &support::write_setup_code(&root));

    if !ok {
        std::process::exit(1);
    }
}

/// The writers: an exception from `save_post`, `transition_post_status`, `wp_insert_post_data`, a `query` filter,
/// `before_delete_post`, `delete_post`, `pre_trash_post`, `trashed_post` or `post_stuck` reaches the caller unchanged,
/// and the rows stay written exactly where WordPress has written them by then.
fn writes(php: &speedyspec_php::Php, setup: &str) -> bool {
    let mut ok = true;
    // save_post fires after the row, its meta and terms are written: the post stays, and the hooks after save_post
    // (wp_insert_post, wp_after_insert_post) never run.
    ok &= yes(
        php,
        setup,
        "save_post_exception_keeps_the_row",
        r#"wp_set_current_user(1);
        $thrown = new LogicException('from save_post');
        $after = [];
        add_action('save_post', function () use ($thrown) { throw $thrown; });
        add_action('wp_insert_post', function ($id) use (&$after) { $after[] = $id; });
        try { wp_insert_post(['post_title' => 'Saved then thrown', 'post_status' => 'publish']); $caught = null; }
        catch (LogicException $e) { $caught = $e; }
        global $wpdb;
        $row = $wpdb->get_row("SELECT ID, post_status, post_name FROM $wpdb->posts WHERE post_title = 'Saved then thrown'");
        return $caught === $thrown && $row && 'publish' === $row->post_status && 'saved-then-thrown' === $row->post_name
            && [] === $after ? true : [$caught, $row, $after];"#,
    );
    // An update: the row is changed before save_post throws, and stays changed.
    ok &= yes(
        php,
        setup,
        "save_post_exception_keeps_the_update",
        r#"wp_set_current_user(1);
        $thrown = new RuntimeException('from save_post on update');
        add_action('save_post', function () use ($thrown) { throw $thrown; });
        try { wp_update_post(['ID' => 6, 'post_title' => 'Retitled before the throw']); $caught = null; }
        catch (RuntimeException $e) { $caught = $e; }
        global $wpdb;
        $title = $wpdb->get_var("SELECT post_title FROM $wpdb->posts WHERE ID = 6");
        return $caught === $thrown && 'Retitled before the throw' === $title ? true : [$caught, $title];"#,
    );
    // transition_post_status fires after the row, its terms and meta_input are written: they stay, and save_post
    // never runs.
    ok &= yes(
        php,
        setup,
        "transition_post_status_exception_keeps_the_row",
        r#"wp_set_current_user(1);
        $thrown = new DomainException('from transition_post_status');
        add_action('transition_post_status', function () use ($thrown) { throw $thrown; }, 10, 3);
        $saved = 0;
        add_action('save_post', function () use (&$saved) { $saved++; });
        try {
            wp_insert_post(['post_title' => 'Transitioned then thrown', 'post_status' => 'publish', 'meta_input' => ['m' => 'v']]);
            $caught = null;
        } catch (DomainException $e) { $caught = $e; }
        global $wpdb;
        $id = (int) $wpdb->get_var("SELECT ID FROM $wpdb->posts WHERE post_title = 'Transitioned then thrown'");
        $meta = $wpdb->get_var($wpdb->prepare("SELECT COUNT(*) FROM $wpdb->postmeta WHERE post_id = %d AND meta_key = 'm'", $id));
        return $caught === $thrown && $id > 0 && '1' === $meta && 0 === $saved ? true : [$caught, $id, $meta, $saved];"#,
    );
    // wp_insert_post_data runs before the statement: nothing is written.
    ok &= yes(
        php,
        setup,
        "wp_insert_post_data_exception_writes_nothing",
        r#"wp_set_current_user(1);
        global $wpdb;
        $before = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
        $thrown = new UnexpectedValueException('from wp_insert_post_data');
        add_filter('wp_insert_post_data', function () use ($thrown) { throw $thrown; });
        try { wp_insert_post(['post_title' => 'Never written']); $caught = null; }
        catch (UnexpectedValueException $e) { $caught = $e; }
        $after = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
        return $caught === $thrown && $before === $after ? true : [$caught, $before, $after];"#,
    );
    // A `query` callback that throws on the INSERT: wpdb hides it behind a failed insert, and the native writer still
    // rethrows the very exception; no row, no db_insert_error answered, and no hook after the insert runs.
    ok &= yes(
        php,
        setup,
        "query_exception_during_insert_is_rethrown",
        r#"wp_set_current_user(1);
        global $wpdb;
        $before = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
        $thrown = new LogicException('from query');
        $transitions = 0;
        add_action('transition_post_status', function () use (&$transitions) { $transitions++; });
        $cb = function ($sql) use ($thrown) { if (str_starts_with($sql, "INSERT INTO `$GLOBALS[table_prefix]posts`")) { throw $thrown; } return $sql; };
        add_filter('query', $cb);
        try { $r = wp_insert_post(['post_title' => 'Query thrown'], true); $caught = $r; }
        catch (LogicException $e) { $caught = $e; }
        remove_filter('query', $cb);
        $after = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts");
        return $caught === $thrown && $before === $after && 0 === $transitions ? true : [$caught, $before, $after, $transitions];"#,
    );
    ok &= deletes_and_trashes(php, setup);
    ok
}

/// Deleting, trashing and sticking: the exception reaches the caller as the very object, and what WordPress wrote
/// before it stays.
fn deletes_and_trashes(php: &speedyspec_php::Php, setup: &str) -> bool {
    let mut ok = true;
    // before_delete_post fires before anything is deleted: the post, its meta and its comments stay, and nothing
    // after it runs.
    ok &= yes(
        php,
        setup,
        "before_delete_post_exception_deletes_nothing",
        r#"$thrown = new LogicException('from before_delete_post');
        $after = 0;
        add_action('before_delete_post', function () use ($thrown) { throw $thrown; });
        add_action('delete_post', function () use (&$after) { $after++; });
        try { wp_delete_post(1, true); $caught = null; } catch (LogicException $e) { $caught = $e; }
        global $wpdb;
        $row = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts WHERE ID = 1");
        $meta = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->postmeta WHERE post_id = 1");
        $comments = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->comments WHERE comment_post_ID = 1");
        return $caught === $thrown && '1' === $row && '0' !== $meta && '0' !== $comments && 0 === $after
            ? true : [$caught, $row, $meta, $comments, $after];"#,
    );
    // delete_post fires after the terms, comments and meta are deleted and before the row: the row stays, the rest is
    // gone, and deleted_post never runs.
    ok &= yes(
        php,
        setup,
        "delete_post_exception_keeps_the_row",
        r#"$thrown = new RuntimeException('from delete_post');
        $deleted = 0;
        // Post 1's revision (4) is deleted first, through wp_delete_post() and its own delete_post.
        add_action('delete_post', function ($id) use ($thrown) { if (1 === $id) { throw $thrown; } });
        add_action('deleted_post', function ($id) use (&$deleted) { if (1 === $id) { $deleted++; } });
        try { wp_delete_post(1, true); $caught = null; } catch (RuntimeException $e) { $caught = $e; }
        global $wpdb;
        $row = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->posts WHERE ID = 1");
        $meta = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->postmeta WHERE post_id = 1");
        $comments = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->comments WHERE comment_post_ID = 1");
        $terms = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->term_relationships WHERE object_id = 1");
        return $caught === $thrown && '1' === $row && '0' === $meta && '0' === $comments && '0' === $terms
            && 0 === $deleted ? true : [$caught, $row, $meta, $comments, $terms, $deleted];"#,
    );
    // pre_trash_post runs first: nothing is written.
    ok &= yes(
        php,
        setup,
        "pre_trash_post_exception_writes_nothing",
        r#"$thrown = new DomainException('from pre_trash_post');
        add_filter('pre_trash_post', function () use ($thrown) { throw $thrown; });
        try { wp_trash_post(6); $caught = null; } catch (DomainException $e) { $caught = $e; }
        global $wpdb;
        $status = $wpdb->get_var("SELECT post_status FROM $wpdb->posts WHERE ID = 6");
        $meta = $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->postmeta WHERE post_id = 6");
        return $caught === $thrown && 'publish' === $status && '0' === $meta ? true : [$caught, $status, $meta];"#,
    );
    // trashed_post fires after the post is trashed: the status and the trash meta stay.
    ok &= yes(
        php,
        setup,
        "trashed_post_exception_keeps_the_trash",
        r#"$thrown = new UnexpectedValueException('from trashed_post');
        add_action('trashed_post', function () use ($thrown) { throw $thrown; });
        try { wp_trash_post(6); $caught = null; } catch (UnexpectedValueException $e) { $caught = $e; }
        global $wpdb;
        $status = $wpdb->get_var("SELECT post_status FROM $wpdb->posts WHERE ID = 6");
        $previous = $wpdb->get_var("SELECT meta_value FROM $wpdb->postmeta WHERE post_id = 6 AND meta_key = '_wp_trash_meta_status'");
        return $caught === $thrown && 'trash' === $status && 'publish' === $previous ? true : [$caught, $status, $previous];"#,
    );
    // post_stuck fires after `sticky_posts` is written: the post stays stuck.
    ok &= yes(
        php,
        setup,
        "post_stuck_exception_keeps_the_option",
        r#"$thrown = new LogicException('from post_stuck');
        add_action('post_stuck', function () use ($thrown) { throw $thrown; });
        try { stick_post(6); $caught = null; } catch (LogicException $e) { $caught = $e; }
        wp_cache_delete('alloptions', 'options');
        wp_cache_delete('sticky_posts', 'options');
        return $caught === $thrown && [1, 6] === get_option('sticky_posts') ? true : [$caught, get_option('sticky_posts')];"#,
    );
    // `sticky_posts` holding an object: get_option() answers the object, which the native writer reads as not an
    // array (the options API's live objects are carried across, not refused), so the list starts again.
    ok &= yes(
        php,
        setup,
        "sticky_posts_holding_an_object_starts_again",
        r#"update_option('sticky_posts', new stdClass());
        stick_post(6);
        return [6] === get_option('sticky_posts') ? true : get_option('sticky_posts');"#,
    );
    // An argument get_post() reads as null (`true`, an array, a string that is not numeric, an id below 1) is no
    // post: the trash and untrash functions answer null, add_ping() false, the publishers nothing, and no row is
    // written. An id below 1 does not fall back to the global post.
    ok &= yes(
        php,
        setup,
        "non_numeric_post_arguments_are_no_post",
        r#"global $wpdb;
        $GLOBALS['post'] = get_post(21);
        $status = fn ($id) => $wpdb->get_var("SELECT post_status FROM $wpdb->posts WHERE ID = $id");
        $pinged = fn ($id) => $wpdb->get_var("SELECT pinged FROM $wpdb->posts WHERE ID = $id");
        $before = array_map($pinged, [1, 6, 21]);
        $out = [];
        foreach ([true, [6], '6abc', -1] as $arg) {
            $out[] = null === wp_trash_post($arg);
            $out[] = false === add_ping($arg, 'http://example.org/pinged');
        }
        foreach ([true, [7], '7abc', -7] as $arg) {
            $out[] = null === wp_untrash_post($arg);
        }
        foreach ([true, [8], '8abc', -8] as $arg) {
            $out[] = null === wp_publish_post($arg);
        }
        foreach ([true, [15], '15abc', -15] as $arg) {
            $out[] = null === check_and_publish_future_post($arg);
        }
        $out = array_merge($out, [
            'publish' === $status(1), 'publish' === $status(6), 'publish' === $status(21), 'trash' === $status(7),
            'draft' === $status(8), 'future' === $status(15), $before === array_map($pinged, [1, 6, 21]),
            '0' === $wpdb->get_var("SELECT COUNT(*) FROM $wpdb->postmeta WHERE meta_key = '_wp_trash_meta_status' AND post_id IN (1, 6, 21)"),
        ]);
        return !in_array(false, $out, true) ? true : $out;"#,
    );
    // The post the trash and delete functions answer is the very object the caller passed, or the WP_Post the hooks
    // received (an attachment's too, through wp_delete_attachment()); no post is `null`.
    ok &= yes(
        php,
        setup,
        "trash_and_delete_answer_the_post",
        r#"$post = get_post(6);
        $trashed = wp_trash_post($post);
        $seen = null;
        add_action('deleted_post', function ($id, $p) use (&$seen) { $seen = $p; }, 10, 2);
        $deleted = wp_delete_post(21, true);
        $deleted_seen = $seen;
        $attachment = wp_delete_post(14, true);
        $out = [$trashed === $post, $deleted === $deleted_seen, $deleted instanceof WP_Post, 21 === $deleted->ID,
            $attachment instanceof WP_Post, $attachment === $seen, 14 === $attachment->ID,
            null === wp_delete_post(999), null === wp_trash_post(999),
            'trash' === $GLOBALS['wpdb']->get_var("SELECT post_status FROM {$GLOBALS['wpdb']->posts} WHERE ID = 6")];
        return !in_array(false, $out, true) ? true : $out;"#,
    );
    ok
}
