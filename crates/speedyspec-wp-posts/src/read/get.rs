//! `get_post()` (post.php 1146-1186), `WP_Post::get_instance()` (class-wp-post.php 279-306) and the readers beside
//! `get_post()`: `get_post_ancestors()`, `get_post_field()`, `get_post_mime_type()`, `get_post_status()`,
//! `get_post_type()` and `wp_get_post_parent_id()` (WordPress 7.2-RC2).

use speedyspec_php_serialize::semantics::{get, isset, to_php_int};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_meta_contracts::{MetaKey, ObjectId};
use speedyspec_wp_posts_contracts::{
    MimeType, Post, PostError, PostId, PostReader, PostRef, PostTypeName,
};

use crate::hook::hv;
use crate::read::api::{PostsApi, int, is_magic, is_str, meta_error, s, truthy};
use crate::{PostEntry, post_ancestors, sanitize_post_field_word, sanitize_post_properties};

/// A `WP_Post` as `get_post()` returns it with the `raw` filter: the entity, and the object's properties as PHP
/// holds them (hooks receive the object).
pub(crate) struct Loaded {
    pub post: Post,
    pub properties: Vec<(PhpKey, PhpValue)>,
}

impl Loaded {
    fn from_properties(properties: Vec<(PhpKey, PhpValue)>) -> Loaded {
        Loaded {
            post: PostEntry::from_properties(&properties),
            properties,
        }
    }

    /// The `WP_Post` object.
    pub fn object(&self) -> PhpValue {
        PostEntry::wp_post(self.properties.clone())
    }

    /// `$post->ID` as PHP's int.
    pub fn id(&self) -> i64 {
        get(&self.properties, "ID").map_or(0, to_php_int)
    }
}

/// PHP 8's `is_numeric()`: an int, a float, or a numeric string (leading and trailing whitespace allowed).
pub(crate) fn is_numeric(value: &PhpValue) -> bool {
    match value {
        PhpValue::Int(_) | PhpValue::Float(_) => true,
        PhpValue::Str(bytes) => numeric_bytes(bytes),
        _ => false,
    }
}

fn numeric_bytes(bytes: &[u8]) -> bool {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = bytes.iter().position(|b| !space(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !space(b))
        .map_or(start, |e| e + 1);
    let mut t = &bytes[start..end.max(start)];
    if let Some((b'+' | b'-', rest)) = t.split_first() {
        t = rest;
    }
    let digits = |t: &[u8]| t.iter().take_while(|b| b.is_ascii_digit()).count();
    let whole = digits(t);
    t = &t[whole..];
    let mut fraction = 0;
    if let Some((b'.', rest)) = t.split_first() {
        fraction = digits(rest);
        t = &rest[fraction..];
    }
    if whole + fraction == 0 {
        return false;
    }
    if let Some((b'e' | b'E', rest)) = t.split_first() {
        let mut rest = rest;
        if let Some((b'+' | b'-', r)) = rest.split_first() {
            rest = r;
        }
        let exp = digits(rest);
        if exp == 0 {
            return false;
        }
        t = &rest[exp..];
    }
    t.is_empty()
}

/// `get_post()` through the use cases, for the hierarchy rules ([`post_ancestors`], [`crate::page_uri`]), which read
/// a post exactly where WordPress calls `get_post( $id )` with a non-zero id.
pub(crate) struct Reader<'a>(pub &'a PostsApi);

impl PostReader for Reader<'_> {
    fn post(&self, id: PostId) -> Result<Option<Post>, PostError> {
        Ok(self.0.instance(int(id.0))?.map(|l| l.post))
    }
}

impl PostsApi {
    /// `WP_Post::get_instance( $post_id )`: `None` (false) for an id below 1; otherwise the `posts` entry when it is
    /// usable ([`PostEntry::usable`]), put through `sanitize_post( …, 'raw' )` unless its `filter` is `raw`;
    /// otherwise `SELECT * … LIMIT 1` through [`PostRepository::find`](speedyspec_wp_posts_contracts::PostRepository::find),
    /// whose raw entry replaces the cached value with `wp_cache_set()` (not `add`: an unusable value may be there).
    pub(crate) fn instance(&self, id: i64) -> Result<Option<Loaded>, PostError> {
        if id <= 0 {
            return Ok(None);
        }
        let key = HookValue::Int(id);
        let cached = self.cache_get(&key, "posts")?;
        if let Some(properties) = cached.as_ref().and_then(PostEntry::usable) {
            let mut properties = properties.to_vec();
            let filter = get(&properties, "filter");
            if !filter.is_some_and(|f| is_str(f, "raw")) {
                sanitize_post_properties(&self.sanitize_ports(), &mut properties, "raw")?;
            }
            return Ok(Some(Loaded::from_properties(
                PostEntry::wp_post_properties(&properties),
            )));
        }
        let Some(post) = self.d.repository.find(PostId(id as u64))? else {
            return Ok(None);
        };
        let entry = PostEntry::to_entry(&post);
        self.cache_set(&HookValue::Int(int(post.id.0)), &entry, "posts")?;
        Ok(Some(Loaded {
            properties: PostEntry::raw_properties(&post),
            post,
        }))
    }

    /// `get_post( $post )` with the `raw` filter: an id, or the global `$post` for [`PostRef::Current`] and for id 0
    /// (`empty( $post )`).
    pub(crate) fn resolve(&self, post: PostRef) -> Result<Option<Loaded>, PostError> {
        match post {
            PostRef::Id(PostId(0)) | PostRef::Current => match self.d.current.current() {
                Some(global) if !matches!(global, PhpValue::Null) => self.resolve_value(global),
                _ => Ok(None),
            },
            PostRef::Id(id) => self.instance(int(id.0)),
        }
    }

    /// `get_post( $post )` with the `raw` filter on a PHP value, branch by branch:
    /// - a `WP_Post` is used as it is when its `filter` is `raw`, else re-read by its `ID` (`WP_Post::filter( 'raw' )`);
    /// - another object with an empty `filter` is sanitized `raw` and becomes a `WP_Post`; one whose `filter` is
    ///   `raw` becomes a `WP_Post` as it is; one with an `ID` is re-read by it; any other gives `None`;
    /// - a numeric value is read by `(int)` it;
    /// - anything else gives `None`.
    pub(crate) fn resolve_value(&self, post: PhpValue) -> Result<Option<Loaded>, PostError> {
        match post {
            PhpValue::Object { class, properties } => {
                let filter = get(&properties, "filter");
                let raw = filter.is_some_and(|f| is_str(f, "raw"));
                if class.eq_ignore_ascii_case("WP_Post") {
                    if raw {
                        return Ok(Some(Loaded::from_properties(properties)));
                    }
                    let id = get(&properties, "ID").map_or(0, to_php_int);
                    return self.instance(id);
                }
                if filter.is_none_or(|f| !truthy(f)) {
                    let mut properties = properties;
                    sanitize_post_properties(&self.sanitize_ports(), &mut properties, "raw")?;
                    return Ok(Some(Loaded::from_properties(
                        PostEntry::wp_post_properties(&properties),
                    )));
                }
                if raw {
                    return Ok(Some(Loaded::from_properties(
                        PostEntry::wp_post_properties(&properties),
                    )));
                }
                match isset(&properties, "ID") {
                    Some(id) => self.instance(to_php_int(id)),
                    None => Ok(None),
                }
            }
            value if is_numeric(&value) => self.instance(to_php_int(&value)),
            _ => Ok(None),
        }
    }

    /// `get_post( $post, OBJECT, 'raw' )`: the raw post, `None` where WordPress returns `null`.
    pub fn get_post(&self, post: PostRef) -> Result<Option<Post>, PostError> {
        Ok(self.resolve(post)?.map(|l| l.post))
    }

    /// `get_post( $post, OBJECT, 'raw' )` as PHP holds it: the `WP_Post` object's properties.
    pub fn get_post_object(&self, post: PostRef) -> Result<Option<PhpValue>, PostError> {
        Ok(self.resolve(post)?.map(|l| l.object()))
    }

    /// `get_post_ancestors( $post )` (post.php 1198-1223).
    pub fn get_post_ancestors(&self, post: PostRef) -> Result<Vec<PostId>, PostError> {
        match self.resolve(post)? {
            Some(loaded) => post_ancestors(&Reader(self), &loaded.post),
            None => Ok(Vec::new()),
        }
    }

    /// `get_post_field( $field, $post, $context )` (post.php 1252-1264), with `WP_Post::__isset()` and `__get()`
    /// (class-wp-post.php 327-404) for a field that is not a property: `ancestors`, `page_template`,
    /// `post_category`, `tags_input`, then any post meta key (`metadata_exists()`, then `get_post_meta( …, true )`
    /// sanitized for the object's `raw` filter). `''` when there is no post or no such field.
    pub fn get_post_field(
        &self,
        field: &[u8],
        post: PostRef,
        context: &str,
    ) -> Result<PhpValue, PostError> {
        let Some(loaded) = self.resolve(post)? else {
            return Ok(s(""));
        };
        let id = loaded.id();
        let value = match loaded
            .properties
            .iter()
            .find(|(k, v)| *k == PhpKey::Str(field.to_vec()) && !matches!(v, PhpValue::Null))
        {
            Some((_, v)) => v.clone(),
            None => match self.magic(&loaded, field)? {
                Some(v) => v,
                None => return Ok(s("")),
            },
        };
        sanitize_post_field_word(&self.sanitize_ports(), field, value, id, context)
    }

    /// `WP_Post::__isset( $key )` then `__get( $key )` for a key that is not a set property; `None` when `__isset()`
    /// is false.
    fn magic(&self, loaded: &Loaded, key: &[u8]) -> Result<Option<PhpValue>, PostError> {
        let id = loaded.id();
        let object = ObjectId(u64::try_from(id).unwrap_or(0));
        let meta = Self::post_meta();
        let filter = get(&loaded.properties, "filter").cloned();
        let filtered = |value: PhpValue| -> Result<PhpValue, PostError> {
            match &filter {
                Some(f) if truthy(f) => sanitize_post_field_word(
                    &self.sanitize_ports(),
                    key,
                    value,
                    id,
                    &String::from_utf8_lossy(&speedyspec_php_serialize::semantics::to_php_string(
                        f,
                    )),
                ),
                _ => Ok(value),
            }
        };
        if is_magic(key) {
            return Ok(Some(match key {
                b"page_template" => self
                    .d
                    .metadata
                    .get(
                        &meta,
                        object,
                        Some(&MetaKey::from("_wp_page_template")),
                        true,
                    )
                    .map_err(meta_error)?,
                b"post_category" => {
                    self.d
                        .terms
                        .post_term_fields(&loaded.object(), "category", "term_id")?
                }
                b"tags_input" => {
                    self.d
                        .terms
                        .post_term_fields(&loaded.object(), "post_tag", "name")?
                }
                _ => {
                    let ancestors = post_ancestors(&Reader(self), &loaded.post)?;
                    filtered(PhpValue::Array(
                        ancestors
                            .iter()
                            .enumerate()
                            .map(|(i, a)| (PhpKey::Int(i as i64), PhpValue::Int(int(a.0))))
                            .collect(),
                    ))?
                }
            }));
        }
        let key_value = MetaKey::new(key.to_vec());
        if !self
            .d
            .metadata
            .exists(&meta, object, &key_value)
            .map_err(meta_error)?
        {
            return Ok(None);
        }
        let value = self
            .d
            .metadata
            .get(&meta, object, Some(&key_value), true)
            .map_err(meta_error)?;
        Ok(Some(filtered(value)?))
    }

    /// `get_post_mime_type( $post )` (post.php 1277-1285): `None` where WordPress returns false.
    pub fn get_post_mime_type(&self, post: PostRef) -> Result<Option<MimeType>, PostError> {
        Ok(self.resolve(post)?.map(|l| l.post.mime_type))
    }

    /// `get_post_status( $post )` (post.php 1298-1355), as WordPress returns it: `false` with no post, otherwise the
    /// `get_post_status` filter's answer, which a callback may make any value.
    pub fn get_post_status(&self, post: PostRef) -> Result<PhpValue, PostError> {
        match self.resolve(post)? {
            Some(loaded) => self.status_of(&loaded),
            None => Ok(PhpValue::Bool(false)),
        }
    }

    /// `get_post_status()` for a post already read. An inherited attachment status follows its parent's, and
    /// `get_post_status( $post->post_parent )` runs twice there, as in WordPress, each with its filter.
    fn status_of(&self, loaded: &Loaded) -> Result<PhpValue, PostError> {
        let post = &loaded.post;
        let attachment = post.post_type.as_str() == "attachment";
        let own = post.status.as_str();
        let parent = post.parent;
        let status = if attachment && own == "inherit" {
            if parent.0 == 0 || self.instance(int(parent.0))?.is_none() || post.id == parent {
                s("publish")
            } else if is_str(&self.get_post_status(PostRef::Id(parent))?, "trash") {
                let meta = self
                    .d
                    .metadata
                    .get(
                        &Self::post_meta(),
                        ObjectId(parent.0),
                        Some(&MetaKey::from("_wp_trash_meta_status")),
                        true,
                    )
                    .map_err(meta_error)?;
                if truthy(&meta) { meta } else { s("publish") }
            } else {
                self.get_post_status(PostRef::Id(parent))?
            }
        } else if attachment && !matches!(own, "private" | "trash" | "auto-draft") {
            s("publish")
        } else {
            s(own)
        };
        self.filter("get_post_status", status, || vec![hv(&loaded.object())])
    }

    /// `get_post_type( $post )` (post.php 1622-1629): `None` where WordPress returns false.
    pub fn get_post_type(&self, post: PostRef) -> Result<Option<PostTypeName>, PostError> {
        Ok(self.resolve(post)?.map(|l| l.post.post_type))
    }

    /// `wp_get_post_parent_id( $post )` (post.php 8503-8511): `None` (false) when there is no post, otherwise the
    /// parent, `PostId(0)` for none (WordPress returns the int 0 there, not false).
    pub fn wp_get_post_parent_id(&self, post: PostRef) -> Result<Option<PostId>, PostError> {
        Ok(self.resolve(post)?.map(|l| l.post.parent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_strings_follow_php_8() {
        for yes in ["2", " 2", "2 ", "1.9", ".5", "-3", "+3", "1e3", " 1E-2\n"] {
            assert!(numeric_bytes(yes.as_bytes()), "{yes}");
        }
        for no in ["", " ", "abc", "1abc", ".", "1e", "--1", "0x1A", "1 2"] {
            assert!(!numeric_bytes(no.as_bytes()), "{no}");
        }
    }
}
