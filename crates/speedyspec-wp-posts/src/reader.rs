//! `get_post()` with no object cache, the [`PostReader`] phase 2b binds.

use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostReader, PostRepository};

/// `get_post( $id )` with no cache: a zero id is `None` with no statement (`WP_Post::get_instance()` returns false
/// before it queries), otherwise the repository's [`PostRepository::find`] (Adapter from repository to reader).
pub struct Uncached<'a>(pub &'a dyn PostRepository);

impl PostReader for Uncached<'_> {
    fn post(&self, id: PostId) -> Result<Option<Post>, PostError> {
        if id == PostId(0) {
            return Ok(None);
        }
        self.0.find(id)
    }
}
