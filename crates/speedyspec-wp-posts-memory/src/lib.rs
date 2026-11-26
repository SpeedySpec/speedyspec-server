//! An in-memory post repository, for tests and for callers with no database.
//!
//! Rows live in insert order. `LIMIT 1` without `ORDER BY` answers the first matching row in that order. Names and
//! types are compared byte for byte, and the auto-increment continues past an explicit ID, as MySQL's does.

use std::cell::RefCell;

use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostRepository, PostTypeName, Slug};

/// `wp_posts` held in memory (Repository, Strategy for the engine).
pub struct MemoryPostRepository {
    state: RefCell<State>,
}

struct State {
    rows: Vec<Post>,
    last_id: u64,
}

impl MemoryPostRepository {
    /// An empty table whose first id is 1.
    pub fn new() -> MemoryPostRepository {
        MemoryPostRepository {
            state: RefCell::new(State {
                rows: Vec::new(),
                last_id: 0,
            }),
        }
    }

    /// The `post_name` of the first row in insert order that `matches`.
    fn first_name(&self, matches: impl Fn(&Post) -> bool) -> Option<Slug> {
        self.state
            .borrow()
            .rows
            .iter()
            .find(|p| matches(p))
            .map(|p| p.name.clone())
    }
}

impl Default for MemoryPostRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl PostRepository for MemoryPostRepository {
    fn engine(&self) -> String {
        "memory".to_owned()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError> {
        let mut state = self.state.borrow_mut();
        let id = if post.id == PostId(0) {
            state.last_id + 1
        } else {
            post.id.0
        };
        if state.rows.iter().any(|p| p.id.0 == id) {
            return Ok(None);
        }
        state.last_id = state.last_id.max(id);
        state.rows.push(Post {
            id: PostId(id),
            comment_count: 0,
            ..post.clone()
        });
        Ok(Some(PostId(id)))
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| &p.name == slug && p.id != except))
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| {
            &p.name == slug
                && (&p.post_type == post_type || p.post_type.as_str() == "attachment")
                && p.id != except
                && p.parent == parent
        }))
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| &p.name == slug && &p.post_type == post_type && p.id != except))
    }
}
