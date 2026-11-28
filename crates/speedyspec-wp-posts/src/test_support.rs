//! Test doubles shared by the crate's unit tests.

use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError, PostId,
    PostParent, PostRepository, PostRow, PostStatusName, PostTypeName, Slug, Timezone,
    WriteOutcome,
};

/// A repository that answers nothing: any statement panics.
pub(crate) struct Silent;

impl PostRepository for Silent {
    fn engine(&self) -> String {
        "silent".to_owned()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        panic!("find({})", id.0)
    }

    fn insert(&self, _row: &PostRow) -> Result<WriteOutcome<PostId>, PostError> {
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
        _parent: PostParent,
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

    fn find_many(&self, _ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        panic!("find_many")
    }

    fn parent_ids(&self, _ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        panic!("parent_ids")
    }

    fn pages_by_names(
        &self,
        _names: &[Slug],
        _post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        panic!("pages_by_names")
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        panic!("all_page_ids")
    }

    fn count_by_status(
        &self,
        _post_type: &PostTypeName,
        _reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        panic!("count_by_status")
    }

    fn count_attachments(
        &self,
        _mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        panic!("count_attachments")
    }

    fn last_post_time(
        &self,
        _field: LastPostField,
        _post_types: &[PostTypeName],
        _timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        panic!("last_post_time")
    }

    fn available_mime_types(&self, _post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        panic!("available_mime_types")
    }

    fn update(&self, _id: PostId, _row: &PostRow) -> Result<WriteOutcome<u64>, PostError> {
        panic!("update")
    }

    fn update_name(&self, _id: PostId, _name: &Slug) -> Result<Option<u64>, PostError> {
        panic!("update_name")
    }

    fn update_guid(&self, _id: PostId, _guid: &[u8]) -> Result<Option<u64>, PostError> {
        panic!("update_guid")
    }

    fn update_status(
        &self,
        _id: PostId,
        _status: &PostStatusName,
    ) -> Result<Option<u64>, PostError> {
        panic!("update_status")
    }

    fn update_pinged(&self, _id: PostId, _pinged: &[u8]) -> Result<Option<u64>, PostError> {
        panic!("update_pinged")
    }

    fn update_post_type(
        &self,
        _id: i64,
        _post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        panic!("update_post_type")
    }

    fn import_id_taken(&self, _id: i64) -> Result<bool, PostError> {
        panic!("import_id_taken")
    }

    fn raw_row(&self, _id: PostId) -> Result<Option<Post>, PostError> {
        panic!("raw_row")
    }

    fn children_of_type(
        &self,
        _parent: PostId,
        _post_type: &PostTypeName,
    ) -> Result<Vec<Post>, PostError> {
        panic!("children_of_type")
    }

    fn reparent_children(
        &self,
        _from: PostId,
        _to: PostParent,
        _post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        panic!("reparent_children")
    }

    fn revision_ids(&self, _parent: PostId) -> Result<Vec<PostId>, PostError> {
        panic!("revision_ids")
    }

    fn reparent_attachments(
        &self,
        _from: PostId,
        _to: PostParent,
    ) -> Result<Option<u64>, PostError> {
        panic!("reparent_attachments")
    }

    fn remove(&self, _id: PostId) -> Result<Option<u64>, PostError> {
        panic!("remove")
    }

    fn auto_drafts_older_than_a_week(&self) -> Result<Vec<PostId>, PostError> {
        panic!("auto_drafts_older_than_a_week")
    }
}
