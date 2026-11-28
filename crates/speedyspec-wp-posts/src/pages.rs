//! The page trees (WordPress 7.2-RC2): `get_page_hierarchy()` with `_page_traverse_name()` (post.php 6649-6687)
//! and `get_page_children()` (post.php 6608-6635), over a list of posts with no reads.
//!
//! Both walk the list depth first, each parent's children in list order. Over a list with a cycle reachable from
//! the start, PHP never ends (`_page_traverse_name()` recurses forever; `get_page_children()`'s stack never
//! empties). The port stops at the first revisit: a page whose ID is already on the path from the start is listed
//! but not descended into. A repeated ID elsewhere is not a cycle, and is walked again as PHP walks it.

use std::collections::HashMap;

use indexmap::IndexMap;
use speedyspec_wp_posts_contracts::{Post, PostId, Slug};

/// `$children[ (int) $page->post_parent ][] = $page` for each page, in list order.
fn children_of(pages: &[Post]) -> HashMap<PostId, Vec<&Post>> {
    let mut children: HashMap<PostId, Vec<&Post>> = HashMap::new();
    for page in pages {
        children
            .entry(crate::hierarchy::parent_id(page))
            .or_default()
            .push(page);
    }
    children
}

/// Visits the pages below `page_id`, depth first in list order, stopping at a revisit (see the module docs).
fn walk<'a>(pages: &'a [Post], page_id: PostId, mut visit: impl FnMut(&'a Post)) {
    let children = children_of(pages);
    // The path from the start: each page and the index of its next child.
    let mut path: Vec<(PostId, usize)> = vec![(page_id, 0)];
    while let Some(&(parent, next)) = path.last() {
        let Some(child) = children.get(&parent).and_then(|list| list.get(next)) else {
            path.pop();
            continue;
        };
        if let Some(top) = path.last_mut() {
            top.1 += 1;
        }
        visit(child);
        if !path.iter().any(|(id, _)| *id == child.id) {
            path.push((child.id, 0));
        }
    }
}

/// `get_page_hierarchy( $pages, $page_id )`: ID => name below `page_id`, depth first in list order. A repeated ID
/// keeps its first position and takes the later name (PHP array assignment). Stops at a revisit where PHP recurses
/// forever.
pub fn page_hierarchy(pages: &[Post], page_id: PostId) -> Vec<(PostId, Slug)> {
    let mut result: IndexMap<PostId, Slug> = IndexMap::new();
    walk(pages, page_id, |page| {
        result.insert(page.id, page.name.clone());
    });
    result.into_iter().collect()
}

/// `get_page_children( $page_id, $pages )`: the pages below `page_id`, depth first in list order, a repeated ID
/// listed each time it is reached. Stops at a revisit where PHP loops forever.
pub fn page_children(page_id: PostId, pages: &[Post]) -> Vec<Post> {
    let mut list = Vec::new();
    walk(pages, page_id, |page| list.push(page.clone()));
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_wp_posts_contracts::PostParent;

    /// Pages of `(ID, post_parent, post_name)`, in list order.
    fn pages(rows: &[(u64, u64, &str)]) -> Vec<Post> {
        rows.iter()
            .map(|(id, parent, name)| Post {
                id: PostId(*id),
                parent: PostParent::from(PostId(*parent)),
                name: Slug::from(*name),
                ..Post::default()
            })
            .collect()
    }

    fn names(hierarchy: &[(PostId, Slug)]) -> Vec<(u64, &[u8])> {
        hierarchy
            .iter()
            .map(|(id, name)| (id.0, name.as_bytes()))
            .collect()
    }

    fn ids(pages: &[Post]) -> Vec<u64> {
        pages.iter().map(|page| page.id.0).collect()
    }

    /// Review Focus 4: PHP recurses forever into 7 -> 8 -> 7; the port lists each once on the path and stops.
    #[test]
    fn page_hierarchy_stops_on_a_cycle() {
        let pages = pages(&[(8, 7, "y"), (7, 8, "x"), (9, 8, "z")]);
        assert_eq!(
            names(&page_hierarchy(&pages, PostId(7))),
            [(8, &b"y"[..]), (7, b"x"), (9, b"z")]
        );
    }

    /// Review Focus 4: PHP's stack never empties on 7 -> 8 -> 7; the port stops at the revisit.
    #[test]
    fn page_children_stops_on_a_cycle() {
        let pages = pages(&[(8, 7, "y"), (7, 8, "x"), (9, 8, "z")]);
        assert_eq!(ids(&page_children(PostId(7), &pages)), [8, 7, 9]);
    }

    /// A repeated ID is not a cycle: PHP visits it again, and the hierarchy's array assignment keeps the first
    /// position and takes the later name.
    #[test]
    fn duplicate_id_keeps_first_position() {
        let pages = pages(&[(1, 0, "first"), (2, 1, "b"), (1, 0, "second")]);
        assert_eq!(
            names(&page_hierarchy(&pages, PostId(0))),
            [(1, &b"second"[..]), (2, b"b")]
        );
        assert_eq!(ids(&page_children(PostId(0), &pages)), [1, 2, 1, 2]);
    }

    #[test]
    fn no_pages_no_hierarchy() {
        assert!(page_hierarchy(&[], PostId(0)).is_empty());
        assert!(page_children(PostId(0), &[]).is_empty());
    }
}
