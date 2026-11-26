//! `get_post()` with no object cache, the [`PostReader`] phase 2b binds.

use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostReader, PostRepository};

/// `get_post( $id )` with no cache: a zero id is `None` with no statement (`WP_Post::get_instance()` returns false
/// before it queries), otherwise the repository's [`PostRepository::find`] (Adapter from repository to reader).
///
/// `get_post()` given 0 falls back to the global `$post` when one is set. That fallback is not modelled here; phase
/// 5 decides where it lives.
pub struct Uncached<'a>(pub &'a dyn PostRepository);

impl PostReader for Uncached<'_> {
    fn post(&self, id: PostId) -> Result<Option<Post>, PostError> {
        if id == PostId(0) {
            return Ok(None);
        }
        self.0.find(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Silent;

    #[test]
    fn uncached_zero_is_none_without_a_statement() {
        assert_eq!(Uncached(&Silent).post(PostId(0)).unwrap(), None);
    }

    #[test]
    #[should_panic(expected = "find(3)")]
    fn uncached_reads_through_find() {
        let _ = Uncached(&Silent).post(PostId(3));
    }
}
