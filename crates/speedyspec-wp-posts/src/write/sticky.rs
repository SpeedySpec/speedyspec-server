//! `stick_post( $post_id )` (post.php 3429-3457) and `unstick_post( $post_id )` (3466-3500), WordPress 7.2-RC2: the
//! `sticky_posts` option, read and written through the options contracts (Adapter), with `post_stuck` and
//! `post_unstuck` after a write (Observer).

use indexmap::IndexSet;
use speedyspec_php_serialize::semantics::to_php_int;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::PostError;

use crate::write::api::PostWritesApi;

/// `array_unique( array_map( 'intval', $stickies ) )`: each int at the key of its first occurrence.
fn unique_ints(items: &[(PhpKey, PhpValue)]) -> Vec<(PhpKey, i64)> {
    let mut seen = IndexSet::new();
    items
        .iter()
        .filter_map(|(k, v)| {
            let n = to_php_int(v);
            seen.insert(n).then(|| (k.clone(), n))
        })
        .collect()
}

/// A list of ints, keyed 0, 1, 2 … (`array_values()`).
fn list(ids: impl IntoIterator<Item = i64>) -> PhpValue {
    PhpValue::Array(
        ids.into_iter()
            .enumerate()
            .map(|(i, n)| (PhpKey::Int(i as i64), PhpValue::Int(n)))
            .collect(),
    )
}

impl PostWritesApi {
    /// `stick_post( $post_id )`: `sticky_posts` (an empty list when it is not an array, else its unique ints) gains
    /// `post_id` when it lacks it, written back as a list; `post_stuck` fires when `update_option()` wrote.
    pub fn stick_post(&self, id: i64) -> Result<(), PostError> {
        let stickies = match self.option("sticky_posts")? {
            PhpValue::Array(items) => unique_ints(&items),
            _ => Vec::new(),
        };
        if stickies.iter().any(|(_, n)| *n == id) {
            return Ok(());
        }
        let ids = stickies.into_iter().map(|(_, n)| n).chain([id]);
        if self.update_option("sticky_posts", list(ids))? {
            self.action("post_stuck", || vec![HookValue::Int(id)])?;
        }
        Ok(())
    }

    /// `unstick_post( $post_id )`: nothing when `sticky_posts` is not an array or lacks `post_id`; otherwise its unique
    /// ints without `post_id`, written as a list, and `post_unstuck` when `update_option()` wrote.
    pub fn unstick_post(&self, id: i64) -> Result<(), PostError> {
        let PhpValue::Array(items) = self.option("sticky_posts")? else {
            return Ok(());
        };
        let mut stickies: Vec<i64> = unique_ints(&items).into_iter().map(|(_, n)| n).collect();
        let Some(offset) = stickies.iter().position(|n| *n == id) else {
            return Ok(());
        };
        stickies.remove(offset);
        if self.update_option("sticky_posts", list(stickies))? {
            self.action("post_unstuck", || vec![HookValue::Int(id)])?;
        }
        Ok(())
    }
}
