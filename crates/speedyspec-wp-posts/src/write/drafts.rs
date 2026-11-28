//! `wp_delete_auto_drafts()` (post.php 8615-8625, WordPress 7.2-RC2): every auto-draft more than a week old, as the
//! repository lists them, force-deleted through [`PostWritesApi::wp_delete_post`].

use speedyspec_wp_posts_contracts::PostError;

use crate::write::api::{PostWritesApi, int};

impl PostWritesApi {
    /// `wp_delete_auto_drafts()`: `SELECT ID FROM {posts} WHERE post_status = 'auto-draft' AND DATE_SUB( NOW(),
    /// INTERVAL 7 DAY ) > post_date`, then `wp_delete_post( $id, true )` for each, in that order. What each delete
    /// answers is ignored, as WordPress ignores it.
    pub fn wp_delete_auto_drafts(&self) -> Result<(), PostError> {
        for id in self.repository().auto_drafts_older_than_a_week()? {
            self.wp_delete_post(int(id), true)?;
        }
        Ok(())
    }
}
