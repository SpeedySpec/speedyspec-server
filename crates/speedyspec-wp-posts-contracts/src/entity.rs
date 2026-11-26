//! The post entity: one `wp_posts` row as a plain value.

use crate::{
    CommentStatus, MimeType, PingStatus, PostDate, PostId, PostStatusName, PostTypeName, Slug,
};

/// One `wp_posts` row as a plain value (23 columns; each [`PostDate`] holds two), fields in the table's column
/// order (schema.php 159-182, WordPress 7.2-RC2).
///
/// It never loads or saves itself: a repository and its mapper do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    /// `ID`.
    pub id: PostId,
    /// `post_author`: a user ID.
    pub author: u64,
    /// `post_date` and `post_date_gmt`.
    pub date: PostDate,
    /// `post_content`.
    pub content: Vec<u8>,
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_excerpt`.
    pub excerpt: Vec<u8>,
    /// `post_status`.
    pub status: PostStatusName,
    /// `comment_status`.
    pub comment_status: CommentStatus,
    /// `ping_status`.
    pub ping_status: PingStatus,
    /// `post_password`.
    pub password: Vec<u8>,
    /// `post_name`.
    pub name: Slug,
    /// `to_ping`.
    pub to_ping: Vec<u8>,
    /// `pinged`.
    pub pinged: Vec<u8>,
    /// `post_modified` and `post_modified_gmt`.
    pub modified: PostDate,
    /// `post_content_filtered`.
    pub content_filtered: Vec<u8>,
    /// `post_parent`: [`PostId`]`(0)` for a top-level post.
    pub parent: PostId,
    /// `guid`.
    pub guid: Vec<u8>,
    /// `menu_order`. PHP's (int), which is wider than the int(11) column.
    pub menu_order: i64,
    /// `post_type`.
    pub post_type: PostTypeName,
    /// `post_mime_type`.
    pub mime_type: MimeType,
    /// `comment_count`.
    pub comment_count: i64,
}

/// `WP_Post`'s property defaults, which are also the columns' defaults: status `publish`, comment and ping status
/// `open`, post type `post`, every date [`crate::PostDateTime::ZERO`], every number 0, every text empty.
impl Default for Post {
    fn default() -> Self {
        Self {
            id: PostId(0),
            author: 0,
            date: PostDate::default(),
            content: Vec::new(),
            title: Vec::new(),
            excerpt: Vec::new(),
            status: PostStatusName::new("publish"),
            comment_status: CommentStatus::new("open"),
            ping_status: PingStatus::new("open"),
            password: Vec::new(),
            name: Slug::default(),
            to_ping: Vec::new(),
            pinged: Vec::new(),
            modified: PostDate::default(),
            content_filtered: Vec::new(),
            parent: PostId(0),
            guid: Vec::new(),
            menu_order: 0,
            post_type: PostTypeName::new("post"),
            mime_type: MimeType::default(),
            comment_count: 0,
        }
    }
}
