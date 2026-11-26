//! Test doubles shared by the crate's unit tests.

use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostRepository, PostTypeName, Slug};

/// A repository that answers nothing: any statement panics.
pub(crate) struct Silent;

impl PostRepository for Silent {
    fn engine(&self) -> String {
        "silent".to_owned()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        panic!("find({})", id.0)
    }

    fn insert(&self, _post: &Post) -> Result<Option<PostId>, PostError> {
        panic!("insert")
    }

    fn attachment_slug_owner(
        &self,
        _slug: &Slug,
        _except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        panic!("attachment_slug_owner")
    }

    fn hierarchical_slug_owner(
        &self,
        _slug: &Slug,
        _post_type: &PostTypeName,
        _except: PostId,
        _parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        panic!("hierarchical_slug_owner")
    }

    fn flat_slug_owner(
        &self,
        _slug: &Slug,
        _post_type: &PostTypeName,
        _except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        panic!("flat_slug_owner")
    }
}
