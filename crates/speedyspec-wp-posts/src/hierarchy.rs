//! The post hierarchy (WordPress 7.2-RC2): `wp_get_post_parent_id()` (post.php 8503-8511), the tortoise and hare of
//! `wp_find_hierarchy_loop()` (functions.php 7350-7415), `wp_check_post_hierarchy_for_loops()` (post.php
//! 8527-8565), `get_post_ancestors()` (post.php 1198-1223) and `get_page_uri()` before its filter (post.php
//! 6700-6727).
//!
//! Each reads through a [`PostReader`] exactly where WordPress calls `get_post()`, so with
//! [`crate::Uncached`] the statements are uncached WordPress's.

use speedyspec_php_serialize::semantics::bytes_truthy;
use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostReader};

/// `wp_get_post_parent_id( $id )`: the post's `post_parent` (0 for none), or `None` for a missing post (false).
pub fn post_parent_id(posts: &dyn PostReader, id: PostId) -> Result<Option<PostId>, PostError> {
    Ok(posts.post(id)?.map(|post| post.parent))
}

/// A tortoise and hare value with PHP truth: `None` (false) and 0 end the walk.
fn truthy(value: Option<PostId>) -> Option<PostId> {
    value.filter(|id| id.0 != 0)
}

/// `$override[ $id ] ?? wp_get_post_parent_id( $id )`, with `wp_find_hierarchy_loop()`'s override
/// `array( $start => $start_parent )` (none when `$start_parent` is null). An override of 0 is kept: `??` only
/// skips null.
fn parent_of(
    posts: &dyn PostReader,
    over: Option<(PostId, PostId)>,
    id: PostId,
) -> Result<Option<PostId>, PostError> {
    match over {
        Some((from, to)) if from == id => Ok(Some(to)),
        _ => post_parent_id(posts, id),
    }
}

/// `wp_find_hierarchy_loop_tortoise_hare()`: some member of a loop reached from `start`, or `None` (false). With
/// `members`, the IDs it passed, inserted tortoise, evanescent hare, hare at each step, each keeping its first
/// position (PHP's `$return[ $id ] = true`).
fn tortoise_hare(
    posts: &dyn PostReader,
    start: PostId,
    over: Option<(PostId, PostId)>,
    mut members: Option<&mut Vec<PostId>>,
) -> Result<Option<PostId>, PostError> {
    let mut tortoise = Some(start);
    let mut hare = start;
    loop {
        // The while condition, in PHP's order: tortoise, then one hare step, then the second.
        let Some(t) = truthy(tortoise) else {
            return Ok(None);
        };
        let Some(evanescent) = truthy(parent_of(posts, over, hare)?) else {
            return Ok(None);
        };
        let Some(h) = truthy(parent_of(posts, over, evanescent)?) else {
            return Ok(None);
        };
        hare = h;
        if let Some(members) = members.as_deref_mut() {
            for id in [t, evanescent, h] {
                if !members.contains(&id) {
                    members.push(id);
                }
            }
        }
        // Tortoise got lapped: a loop.
        if t == evanescent || t == h {
            return Ok(Some(t));
        }
        tortoise = parent_of(posts, over, t)?;
    }
}

/// `wp_find_hierarchy_loop( 'wp_get_post_parent_id', $start, $start_parent )`: the members of the loop reached
/// from `start`, in WordPress's key order, or none. `start_parent` stands in for `start`'s parent (`None` reads it).
pub fn find_hierarchy_loop(
    posts: &dyn PostReader,
    start: PostId,
    start_parent: Option<PostId>,
) -> Result<Vec<PostId>, PostError> {
    let over = start_parent.map(|parent| (start, parent));
    let Some(member) = tortoise_hare(posts, start, over, None)? else {
        return Ok(Vec::new());
    };
    let mut members = Vec::new();
    // The first pass found `member` on a loop, so a walk that starts there never ends and always laps.
    let lapped = tortoise_hare(posts, member, over, Some(&mut members))?;
    debug_assert!(lapped.is_some(), "a walk that starts on a loop member laps");
    Ok(members)
}

/// `wp_check_post_hierarchy_for_loops()`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentCheck {
    /// The parent to keep: 0 for none.
    pub parent: PostId,
    /// The members of a loop that does not contain the post, in `wp_find_hierarchy_loop()`'s order. WordPress
    /// detaches each with `wp_update_post( array( 'ID' => $member, 'post_parent' => 0 ) )`; phase 5 runs them.
    pub break_loop: Vec<PostId>,
}

/// `wp_check_post_hierarchy_for_loops( $post_parent, $post_id )` (the `wp_insert_post_parent` filter's callback),
/// without its `wp_update_post()` calls: see [`ParentCheck::break_loop`].
pub fn check_post_hierarchy_for_loops(
    parent: PostId,
    post_id: PostId,
    posts: &dyn PostReader,
) -> Result<ParentCheck, PostError> {
    let keep = |parent| ParentCheck {
        parent,
        break_loop: Vec::new(),
    };
    // Nothing fancy; a new post cannot cause a loop; a post cannot be its own parent.
    if parent.0 == 0 {
        return Ok(keep(PostId(0)));
    }
    if post_id.0 == 0 {
        return Ok(keep(parent));
    }
    if parent == post_id {
        return Ok(keep(PostId(0)));
    }
    let members = find_hierarchy_loop(posts, post_id, Some(parent))?;
    if members.is_empty() {
        return Ok(keep(parent));
    }
    if members.contains(&post_id) {
        return Ok(keep(PostId(0)));
    }
    Ok(ParentCheck {
        parent,
        break_loop: members,
    })
}

/// `get_post_ancestors()` for a post already read: its parent, grandparent and so on, stopping at a post with no
/// parent, a missing post, the post itself or an ancestor already listed. Reads each ancestor once.
pub fn post_ancestors(posts: &dyn PostReader, post: &Post) -> Result<Vec<PostId>, PostError> {
    if post.parent.0 == 0 || post.parent == post.id {
        return Ok(Vec::new());
    }
    let mut ancestors = vec![post.parent];
    let mut id = post.parent;
    while let Some(ancestor) = posts.post(id)? {
        if ancestor.parent.0 == 0
            || ancestor.parent == post.id
            || ancestors.contains(&ancestor.parent)
        {
            break;
        }
        id = ancestor.parent;
        ancestors.push(id);
    }
    Ok(ancestors)
}

/// `get_page_uri()` for a page already read, before its `get_page_uri` filter: each ancestor's name (when truthy
/// in PHP: neither `""` nor `"0"`) then `/`, nearest last, before the page's own name. `$page->ancestors` reads
/// every ancestor, then each is read again for its name, as WordPress does.
pub fn page_uri(posts: &dyn PostReader, page: &Post) -> Result<Vec<u8>, PostError> {
    let mut uri = page.name.as_bytes().to_vec();
    for id in post_ancestors(posts, page)? {
        if let Some(parent) = posts.post(id)? {
            let name = parent.name.as_bytes();
            if bytes_truthy(name) {
                uri = [name, b"/", &uri].concat();
            }
        }
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use speedyspec_wp_posts_contracts::{PostRepository, Slug};
    use speedyspec_wp_posts_memory::MemoryPostRepository;

    use super::*;
    use crate::Uncached;

    /// A table of `(ID, post_parent, post_name)`.
    fn table(rows: &[(u64, u64, &str)]) -> MemoryPostRepository {
        let repository = MemoryPostRepository::new();
        for (id, parent, name) in rows {
            repository
                .insert(&Post {
                    id: PostId(*id),
                    parent: PostId(*parent),
                    name: Slug::from(*name),
                    ..Post::default()
                })
                .unwrap();
        }
        repository
    }

    fn ids(ids: &[u64]) -> Vec<PostId> {
        ids.iter().copied().map(PostId).collect()
    }

    /// Review Focus 4: post 10 given parent 7 meets the 7 <-> 8 loop, which does not contain it. WordPress keeps
    /// the parent and detaches the loop's members, in `wp_find_hierarchy_loop( …, 10, 7 )`'s recorded order.
    #[test]
    fn foreign_loop_is_broken_not_kept() {
        let repository = table(&[(7, 8, "x"), (8, 7, "y"), (10, 0, "ten")]);
        let posts = Uncached(&repository);
        let check = check_post_hierarchy_for_loops(PostId(7), PostId(10), &posts).unwrap();
        assert_eq!(
            check,
            ParentCheck {
                parent: PostId(7),
                break_loop: ids(&[7, 8]),
            }
        );
    }

    #[test]
    fn own_loop_drops_the_parent() {
        let repository = table(&[(1, 0, "a"), (2, 1, "b"), (3, 2, "c")]);
        let posts = Uncached(&repository);
        let check = check_post_hierarchy_for_loops(PostId(3), PostId(1), &posts).unwrap();
        assert_eq!(
            check,
            ParentCheck {
                parent: PostId(0),
                break_loop: Vec::new(),
            }
        );
        assert_eq!(
            find_hierarchy_loop(&posts, PostId(1), Some(PostId(3))).unwrap(),
            ids(&[3, 2, 1])
        );
    }

    #[test]
    fn no_loop_and_trivial_parents() {
        let repository = table(&[(1, 0, "a"), (2, 1, "b")]);
        let posts = Uncached(&repository);
        let parent = |parent, post| {
            check_post_hierarchy_for_loops(PostId(parent), PostId(post), &posts)
                .unwrap()
                .parent
        };
        assert_eq!(parent(0, 2), PostId(0));
        assert_eq!(parent(2, 0), PostId(2));
        assert_eq!(parent(2, 2), PostId(0));
        assert_eq!(parent(1, 2), PostId(1));
        assert!(
            find_hierarchy_loop(&posts, PostId(2), Some(PostId(0)))
                .unwrap()
                .is_empty()
        );
        assert_eq!(post_parent_id(&posts, PostId(99)).unwrap(), None);
    }

    #[test]
    fn ancestors_stop_at_a_revisit() {
        let repository = table(&[(7, 8, "x"), (8, 7, "y"), (9, 7, "z"), (6, 6, "self")]);
        let posts = Uncached(&repository);
        let read = |id| posts.post(PostId(id)).unwrap().unwrap();
        assert_eq!(post_ancestors(&posts, &read(9)).unwrap(), ids(&[7, 8]));
        assert_eq!(post_ancestors(&posts, &read(7)).unwrap(), ids(&[8]));
        assert!(post_ancestors(&posts, &read(6)).unwrap().is_empty());
        assert_eq!(page_uri(&posts, &read(9)).unwrap(), b"y/x/z");
    }

    /// PHP truth: an ancestor named `"0"` adds nothing to the URI, as one named `""` does not.
    #[test]
    fn page_uri_skips_falsy_names() {
        let repository = table(&[(1, 0, "0"), (2, 1, ""), (3, 2, "c"), (4, 3, "d")]);
        let posts = Uncached(&repository);
        let page = posts.post(PostId(4)).unwrap().unwrap();
        assert_eq!(page_uri(&posts, &page).unwrap(), b"c/d");
    }
}
