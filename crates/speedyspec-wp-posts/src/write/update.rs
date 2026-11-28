//! `wp_update_post( $postarr, $wp_error, $fire_after_hooks )` (post.php 5568-5643) and `set_post_type( $post_id,
//! $post_type )` (post.php 2471-2480), WordPress 7.2-RC2.

use speedyspec_php_serialize::semantics::{
    array_merge, is_empty, set, to_array, to_php_int, to_php_string, unset,
};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    PostError, PostId, PostRef, PostTypeName, PostWriteError, WriteResult,
};

use crate::read::{Loaded, Reader};
use crate::write::api::{PostWritesApi, checked_rows, int};
use crate::write::error::php_error;
use crate::write::insert::type_name;
use crate::write::php::{Items, at, empty_at, identical, is_str, list_pluck, s, wp_slash};
use crate::{post_ancestors, sanitize_post_field_word};

impl PostWritesApi {
    /// `get_post( $post, ARRAY_A )`: the `WP_Post`'s properties, then `WP_Post::to_array()`'s magic ones in its
    /// order: `ancestors`, `page_template` (`_wp_page_template`), `post_category` and `tags_input` (through the
    /// taxonomy port).
    fn post_array(&self, loaded: &Loaded) -> Result<Items, PostError> {
        let mut items = loaded.properties.clone();
        let ancestors = post_ancestors(&Reader(&self.d.posts), &loaded.post)?;
        let ancestors = ancestors
            .iter()
            .enumerate()
            .map(|(i, a)| (PhpKey::Int(i as i64), PhpValue::Int(int(*a))))
            .collect();
        set(&mut items, "ancestors", PhpValue::Array(ancestors));
        let template = self.get_meta(loaded.post.id, b"_wp_page_template", true)?;
        set(&mut items, "page_template", template);
        let object = loaded.object();
        let tax = &*self.d.taxonomy;
        set(
            &mut items,
            "post_category",
            tax.post_term_fields(&object, "category", "term_id")?,
        );
        set(
            &mut items,
            "tags_input",
            tax.post_term_fields(&object, "post_tag", "name")?,
        );
        Ok(items)
    }

    /// `wp_update_post( $postarr, $wp_error, $fire_after_hooks )`, step by step:
    /// 1. an object is read with `get_object_vars()` and slashed;
    /// 2. `get_post( $postarr['ID'], ARRAY_A )` (the global `$post` for an empty id): none is `invalid_post`;
    /// 3. the stored fields, slashed, under the given ones; the given categories when there are some, else the
    ///    post's; a draft with no GMT date and no `edit_date` gets now as its date;
    /// 4. an attachment goes to `wp_insert_attachment()` through the uploads port;
    /// 5. `tags_input` equal to the post's tag names is dropped;
    /// 6. [`PostWritesApi::wp_insert_post`].
    ///
    /// A `$postarr` that is neither an array nor an object fails as PHP fails, before anything is written: a string at
    /// `$postarr['ID']` (`TypeError`, "Cannot access offset of type string on string"); any other scalar or `null`
    /// reads `ID` as `null`, so `get_post()` takes the global `$post` (none is `invalid_post`), and `array_merge()`
    /// throws its `TypeError` once the post is read.
    pub fn wp_update_post(
        &self,
        postarr: PhpValue,
        wp_error: bool,
        fire_after_hooks: bool,
    ) -> Result<WriteResult, PostError> {
        let scalar = match &postarr {
            PhpValue::Object { .. } | PhpValue::Array(_) => None,
            PhpValue::Str(_) => {
                return Err(php_error(
                    "TypeError",
                    "Cannot access offset of type string on string".into(),
                ));
            }
            other => Some(type_name(other)),
        };
        let given: Items = match postarr {
            PhpValue::Object { properties, .. } => to_array(wp_slash(PhpValue::Array(properties))),
            PhpValue::Array(items) => items,
            _ => Vec::new(),
        };
        let id = at(&given, "ID").cloned().unwrap_or(PhpValue::Null);
        let loaded = if is_empty(&id) {
            self.d.posts.resolve(PostRef::Current)?
        } else {
            self.d.posts.resolve_value(id)?
        };
        let Some(loaded) = loaded else {
            return Ok(Self::refuse(wp_error, PostWriteError::InvalidPost));
        };
        let post = to_array(wp_slash(PhpValue::Array(self.post_array(&loaded)?)));

        let categories = match at(&given, "post_category") {
            Some(PhpValue::Array(items)) if !items.is_empty() => PhpValue::Array(items.clone()),
            _ => at(&post, "post_category")
                .cloned()
                .unwrap_or(PhpValue::Null),
        };
        let clear_date = at(&post, "post_status").is_some_and(|status| {
            ["draft", "pending", "auto-draft"]
                .iter()
                .any(|draft| is_str(status, draft))
        }) && empty_at(&given, "edit_date")
            && at(&post, "post_date_gmt").is_some_and(|gmt| is_str(gmt, "0000-00-00 00:00:00"));

        if let Some(given_type) = scalar {
            return Err(php_error(
                "TypeError",
                format!("array_merge(): Argument #2 must be of type array, {given_type} given"),
            ));
        }
        let mut postarr = array_merge(post, given);
        set(&mut postarr, "post_category", categories);
        if clear_date {
            set(&mut postarr, "post_date", s(self.d.clock.now_local()?));
            set(&mut postarr, "post_date_gmt", s(""));
        }

        if at(&postarr, "post_type").is_some_and(|t| is_str(t, "attachment")) {
            let answer = self
                .d
                .uploads
                .insert_attachment(PhpValue::Array(postarr), wp_error)?;
            return Ok(match answer {
                PhpValue::Object { ref class, .. } if class.eq_ignore_ascii_case("WP_Error") => {
                    Err(PostWriteError::Delegated(answer))
                }
                other => Ok(PostId(u64::try_from(to_php_int(&other)).unwrap_or(0))),
            });
        }

        if let Some(tags_input) =
            speedyspec_php_serialize::semantics::isset(&postarr, "tags_input").cloned()
        {
            let post_type = at(&postarr, "post_type")
                .map(to_php_string)
                .unwrap_or_default();
            let post_type = PostTypeName::new(String::from_utf8_lossy(&post_type));
            if self
                .d
                .taxonomy
                .is_object_in_taxonomy(&post_type, "post_tag")?
            {
                let id = u64::try_from(at(&postarr, "ID").map_or(0, to_php_int)).unwrap_or(0);
                let tags = self.d.taxonomy.get_the_terms(PostId(id), "post_tag")?;
                let is_error = matches!(&tags, PhpValue::Object { class, .. } if class.eq_ignore_ascii_case("WP_Error"));
                let names = if !is_empty(&tags) && !is_error {
                    list_pluck(&tags, "name")
                } else {
                    PhpValue::Array(Vec::new())
                };
                if identical(&tags_input, &names) {
                    unset(&mut postarr, "tags_input");
                }
            }
        }

        self.wp_insert_post(PhpValue::Array(postarr), wp_error, fire_after_hooks)
    }

    /// `set_post_type( $post_id, $post_type )`: the type through `sanitize_post_field( 'post_type', …, 'db' )`, the
    /// update (`%d` prints `post_id` as it is), then `clean_post_cache( $post_id )`, which reads the global `$post`
    /// for 0 and nothing for a negative id. The rows changed, `None` where WordPress returns false.
    pub fn set_post_type(
        &self,
        post_id: i64,
        post_type: &PhpValue,
    ) -> Result<Option<u64>, PostError> {
        let posts = &self.d.posts;
        let sanitized = sanitize_post_field_word(
            &posts.sanitize_ports(),
            b"post_type",
            post_type.clone(),
            post_id,
            "db",
        )?;
        let name = PostTypeName::new(String::from_utf8_lossy(&to_php_string(&sanitized)));
        let rows = checked_rows(self.repository().update_post_type(post_id, &name)?)?;
        match post_id {
            0 => posts.clean_post_cache(PostRef::Current)?,
            id if id > 0 => posts.clean_post_cache(PostRef::Id(PostId(id as u64)))?,
            _ => {}
        }
        Ok(rows)
    }
}
