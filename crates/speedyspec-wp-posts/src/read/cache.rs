//! The `posts` cache functions (WordPress 7.2-RC2): `update_post_cache()` (post.php 8152-8166),
//! `clean_post_cache()` (8182-8224), `update_post_caches()` (8240-8287), `update_post_parent_caches()`
//! (8312-8320), `_prime_post_caches()` (8736-8758), `_prime_post_parent_id_caches()`
//! (8769-8805) and `wp_cache_set_posts_last_changed()` (8875-8877), with the cache helpers they use:
//! `_get_non_cached_ids()` (functions.php 7566-7584), `wp_cache_get_last_changed()` and
//! `wp_cache_set_last_changed()` (functions.php 8411-8450), `wp_cache_get_salted()` and `wp_cache_set_salted()`
//! (cache-compat.php 214-255).

use indexmap::IndexSet;
use speedyspec_php_serialize::semantics::{get, to_array_key, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{HookValue, Key};
use speedyspec_wp_meta_contracts::ObjectId;
use speedyspec_wp_posts_contracts::{Post, PostError, PostId, PostRef};

use crate::hook::hv;
use crate::read::api::{PostsApi, int, is_str, meta_error, s, truthy};
use crate::{PostEntry, sanitize_post_properties};

/// `array_unique( array_map( 'intval', $ids ) )`, first occurrences kept (`_validate_cache_id()` accepts every int).
fn unique(ids: &[PostId]) -> Vec<i64> {
    ids.iter()
        .map(|id| int(id.0))
        .collect::<IndexSet<i64>>()
        .into_iter()
        .collect()
}

/// A cache key's id.
fn key_id(key: &Key) -> i64 {
    match key {
        Key::Int(i) => *i,
        Key::Str(s) => s.parse().unwrap_or(0),
    }
}

impl PostsApi {
    /// `wp_cache_set_last_changed( $group )`: `microtime()` stored as `last_changed`, then the
    /// `wp_cache_set_last_changed` action with `( $group, $time, $previous_time )`, `false` for no previous time.
    pub fn wp_cache_set_last_changed(&self, group: &str) -> Result<PhpValue, PostError> {
        let key = HookValue::Str("last_changed".into());
        let previous = self
            .cache_get(&key, group)?
            .unwrap_or(PhpValue::Bool(false));
        let time = s(self.d.clock.microtime());
        self.cache_set(&key, &time, group)?;
        self.action("wp_cache_set_last_changed", || {
            vec![HookValue::Str(group.to_owned()), hv(&time), hv(&previous)]
        })?;
        Ok(time)
    }

    /// `wp_cache_get_last_changed( $group )`: the stored value when truthy, else a new one.
    pub fn wp_cache_get_last_changed(&self, group: &str) -> Result<PhpValue, PostError> {
        match self.cache_get(&HookValue::Str("last_changed".into()), group)? {
            Some(time) if truthy(&time) => Ok(time),
            _ => self.wp_cache_set_last_changed(group),
        }
    }

    /// `wp_cache_set_posts_last_changed()`.
    pub fn wp_cache_set_posts_last_changed(&self) -> Result<(), PostError> {
        self.wp_cache_set_last_changed("posts")?;
        Ok(())
    }

    /// `implode( ':', $salt )` for an array salt, the salt itself otherwise.
    fn salt(salt: &PhpValue) -> PhpValue {
        match salt {
            PhpValue::Array(items) => s(items
                .iter()
                .map(|(_, v)| to_php_string(v))
                .collect::<Vec<_>>()
                .join(&b':')),
            other => other.clone(),
        }
    }

    /// `wp_cache_get_salted( $key, $group, $salt )`: the entry's `data` when the entry is an array whose `salt` and
    /// `data` are set and whose `salt` is identical (`===`) to this one; `None` (false) otherwise.
    pub(crate) fn wp_cache_get_salted(
        &self,
        key: &str,
        group: &str,
        salt: &PhpValue,
    ) -> Result<Option<PhpValue>, PostError> {
        let salt = Self::salt(salt);
        let Some(PhpValue::Array(entry)) =
            self.cache_get(&HookValue::Str(key.to_owned()), group)?
        else {
            return Ok(None);
        };
        let set = |name: &str| get(&entry, name).filter(|v| !matches!(v, PhpValue::Null));
        match (set("salt"), set("data")) {
            (Some(stored), Some(data)) if identical(stored, &salt) => Ok(Some(data.clone())),
            _ => Ok(None),
        }
    }

    /// `wp_cache_set_salted( $key, $data, $group, $salt )`: `array( 'data' => $data, 'salt' => $salt )`.
    pub(crate) fn wp_cache_set_salted(
        &self,
        key: &str,
        data: PhpValue,
        group: &str,
        salt: &PhpValue,
    ) -> Result<(), PostError> {
        let entry = PhpValue::Array(vec![
            (PhpKey::Str(b"data".to_vec()), data),
            (PhpKey::Str(b"salt".to_vec()), Self::salt(salt)),
        ]);
        self.cache_set(&HookValue::Str(key.to_owned()), &entry, group)
    }

    /// `update_post_cache( $posts )`: each post's raw entry ([`PostEntry::to_entry`]) by its id, through
    /// `wp_cache_add_multiple()`, so an entry already cached stays. A repeated id keeps the last post at the first
    /// position, as `$data[ $post->ID ] = $post` does. No posts, no call.
    pub fn update_post_cache(&self, posts: &[Post]) -> Result<(), PostError> {
        if posts.is_empty() {
            return Ok(());
        }
        let mut data: Vec<(Key, PhpValue)> = Vec::new();
        for post in posts {
            let key = Key::Int(int(post.id.0));
            let entry = PostEntry::to_entry(post);
            match data.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = entry,
                None => data.push((key, entry)),
            }
        }
        self.cache_add_multiple(data, "posts")
    }

    /// `update_post_cache( $posts )` for posts as PHP holds them (objects): each is sanitized `raw` unless its
    /// `filter` is `raw`, then keyed by its `ID`.
    pub fn update_post_cache_objects(&self, posts: Vec<PhpValue>) -> Result<(), PostError> {
        if posts.is_empty() {
            return Ok(());
        }
        let mut data: Vec<(Key, PhpValue)> = Vec::new();
        for post in posts {
            let PhpValue::Object {
                class,
                mut properties,
            } = post
            else {
                continue;
            };
            if !get(&properties, "filter").is_some_and(|f| is_str(f, "raw")) {
                sanitize_post_properties(&self.sanitize_ports(), &mut properties, "raw")?;
            }
            // $data[ $post->ID ]: PHP's array key rules ("5" is 5, null is "").
            let id = get(&properties, "ID").cloned().unwrap_or(PhpValue::Null);
            let key = match to_array_key(&id) {
                Some(PhpKey::Int(i)) => Key::Int(i),
                Some(PhpKey::Str(b)) => Key::Str(String::from_utf8_lossy(&b).into_owned()),
                None => Key::Str(String::new()),
            };
            let entry = PhpValue::Object { class, properties };
            match data.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = entry,
                None => data.push((key, entry)),
            }
        }
        self.cache_add_multiple(data, "posts")
    }

    /// `clean_post_cache( $post )`, in order: nothing while invalidation is suspended or with no post; then the
    /// post's `posts` entry, its `post_parent:{ID}` entry and its `post_meta` entry are deleted; the terms port's
    /// `clean_object_term_cache()`; `wp_get_archives` in `general`; the `clean_post_cache` action with
    /// `( $post->ID, $post )`; for a page, `all_page_ids` and the `clean_page_cache` action with `( $post->ID )`;
    /// last, `wp_cache_set_posts_last_changed()`.
    pub fn clean_post_cache(&self, post: PostRef) -> Result<(), PostError> {
        if self.d.invalidation.suspended() {
            return Ok(());
        }
        let Some(loaded) = self.resolve(post)? else {
            return Ok(());
        };
        let id = loaded.id();
        let post = &loaded.post;
        self.cache_delete(&HookValue::Int(id), "posts")?;
        self.cache_delete(&HookValue::Str(format!("post_parent:{id}")), "posts")?;
        self.cache_delete(&HookValue::Int(id), "post_meta")?;
        self.d
            .terms
            .clean_object_term_cache(&[post.id], post.post_type.as_str())?;
        self.cache_delete(&HookValue::Str("wp_get_archives".into()), "general")?;
        self.action("clean_post_cache", || {
            vec![HookValue::Int(id), hv(&loaded.object())]
        })?;
        if post.post_type.as_str() == "page" {
            self.cache_delete(&HookValue::Str("all_page_ids".into()), "posts")?;
            self.action("clean_page_cache", || vec![HookValue::Int(id)])?;
        }
        self.wp_cache_set_posts_last_changed()
    }

    /// `_get_non_cached_ids( $ids, 'posts' )`: the distinct ids whose `posts` entry is a miss (`false`), in order.
    pub(crate) fn non_cached_ids(
        &self,
        ids: &[PostId],
        group: &str,
    ) -> Result<Vec<i64>, PostError> {
        let ids = unique(ids);
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<HookValue> = ids.iter().map(|i| HookValue::Int(*i)).collect();
        Ok(self
            .cache_get_multiple(&keys, group)?
            .into_iter()
            .filter(|(_, v)| matches!(v, PhpValue::Bool(false)))
            .map(|(k, _)| key_id(&k))
            .collect())
    }

    /// `_prime_post_caches( $ids, $update_term_cache, $update_meta_cache )`: the ids not cached are read with
    /// [`PostRepository::find_many`](speedyspec_wp_posts_contracts::PostRepository::find_many) and cached through
    /// [`PostsApi::update_post_cache`]; then, for every id given, `update_postmeta_cache()` (the metadata API's
    /// `update_cache`) and `update_object_term_cache( $ids, array_unique( array_map( 'get_post_type', $ids ) ) )`
    /// through the terms port.
    ///
    /// The ids are typed, so WordPress's `_validate_cache_id()` cases cannot reach it: a non-integer string (`"x"`,
    /// dropped with `_doing_it_wrong()`) and a negative id (`-1`, which passes the check and reaches the `IN (…)`
    /// statement and `update_postmeta_cache()`). The PHP binding drops both before calling, so its statement text
    /// can differ from WordPress's for such input (Task 8 lists it as a documented break).
    pub fn prime_post_caches(
        &self,
        ids: &[PostId],
        terms: bool,
        meta: bool,
    ) -> Result<(), PostError> {
        let given: Vec<PhpValue> = ids.iter().map(|id| PhpValue::Int(int(id.0))).collect();
        self.prime_post_caches_given(ids, &given, terms, meta)
    }

    /// [`PostsApi::prime_post_caches`] where the caller's `$ids` hold values the typed `ids` drop (a non-integer
    /// string `_validate_cache_id()` refused, a negative id): the rows are read for `ids`, while the meta and term
    /// steps take `given`, as WordPress passes them its unfiltered `$ids`. `update_postmeta_cache()` reads each
    /// given value as `intval` (a negative one as 0, since the metadata ids are unsigned), and `get_post_type()`
    /// reads each as `get_post()` does (an empty one, `0` or `''`, as the current post).
    pub fn prime_post_caches_given(
        &self,
        ids: &[PostId],
        given: &[PhpValue],
        terms: bool,
        meta: bool,
    ) -> Result<(), PostError> {
        let non_cached = self.non_cached_ids(ids, "posts")?;
        if !non_cached.is_empty() {
            let wanted: Vec<PostId> = non_cached
                .iter()
                .map(|i| PostId(u64::try_from(*i).unwrap_or(0)))
                .collect();
            let fresh = self.d.repository.find_many(&wanted)?;
            if !fresh.is_empty() {
                self.update_post_cache(&fresh)?;
            }
        }
        let intval: Vec<PostId> = given
            .iter()
            .map(|v| PostId(u64::try_from(to_php_int(v)).unwrap_or(0)))
            .collect();
        if meta {
            self.update_postmeta_cache(&intval)?;
        }
        if terms {
            let mut types: Vec<PhpValue> = Vec::new();
            for value in given {
                // get_post_type( $id ): an empty id is get_post()'s current post.
                let loaded = if truthy(value) {
                    self.resolve_value(value.clone())?
                } else {
                    self.resolve(PostRef::Current)?
                };
                let post_type = match loaded {
                    Some(loaded) => s(loaded.post.post_type.as_str()),
                    None => PhpValue::Bool(false),
                };
                // array_unique() compares string forms: false and '' are one value.
                if !types
                    .iter()
                    .any(|t| to_php_string(t) == to_php_string(&post_type))
                {
                    types.push(post_type);
                }
            }
            self.d.terms.update_object_term_cache(&intval, &types)?;
        }
        Ok(())
    }

    /// `update_post_caches( $posts, $post_type, $update_term_cache, $update_meta_cache )` (post.php 8240-8279): no
    /// posts, nothing; otherwise [`PostsApi::update_post_cache`], then `update_object_term_cache( $post_ids, $ptypes )`
    /// through the terms port when `terms` and `$ptypes` is not empty, then `update_postmeta_cache( $post_ids )` when
    /// `meta`. `$post_ids` keeps every post's id in order, repeats included.
    ///
    /// `post_type` is PHP's `$post_type`: a falsy value is `'any'`; an array gives its values; `'any'` (compared
    /// with `===`) gives the posts' own types, `array_unique()`d; any other value is the one type.
    pub fn update_post_caches(
        &self,
        posts: &[Post],
        post_type: &PhpValue,
        terms: bool,
        meta: bool,
    ) -> Result<(), PostError> {
        if posts.is_empty() {
            return Ok(());
        }
        self.update_post_cache(posts)?;
        let ids: Vec<PostId> = posts.iter().map(|p| p.id).collect();
        let post_type = if truthy(post_type) {
            post_type.clone()
        } else {
            s("any")
        };
        if terms {
            let types: Vec<PhpValue> = match &post_type {
                PhpValue::Array(items) => items.iter().map(|(_, v)| v.clone()).collect(),
                any if is_str(any, "any") => {
                    let mut types: Vec<PhpValue> = Vec::new();
                    for post in posts {
                        let t = s(post.post_type.as_str());
                        if !types.contains(&t) {
                            types.push(t);
                        }
                    }
                    types
                }
                one => vec![one.clone()],
            };
            if !types.is_empty() {
                self.d.terms.update_object_term_cache(&ids, &types)?;
            }
        }
        if meta {
            self.update_postmeta_cache(&ids)?;
        }
        Ok(())
    }

    /// [`PostsApi::update_post_caches`] for posts as PHP holds them (objects): the cache is written with
    /// [`PostsApi::update_post_cache_objects`], so each object is kept as it is (a `WP_Post` stays one), and the ids
    /// and the `'any'` types are read from each object's `ID` and `post_type` (`(int)` and as they are). A value
    /// that is not an object adds neither.
    pub fn update_post_caches_objects(
        &self,
        posts: Vec<PhpValue>,
        post_type: &PhpValue,
        terms: bool,
        meta: bool,
    ) -> Result<(), PostError> {
        if posts.is_empty() {
            return Ok(());
        }
        let ids: Vec<PostId> = posts
            .iter()
            .filter_map(|p| crate::read::api::prop(p, "ID"))
            .map(|id| PostId(u64::try_from(to_php_int(id)).unwrap_or(0)))
            .collect();
        let own_types: Vec<PhpValue> = posts
            .iter()
            .filter_map(|p| crate::read::api::prop(p, "post_type").cloned())
            .collect();
        self.update_post_cache_objects(posts)?;
        let post_type = if truthy(post_type) {
            post_type.clone()
        } else {
            s("any")
        };
        if terms {
            let types: Vec<PhpValue> = match &post_type {
                PhpValue::Array(items) => items.iter().map(|(_, v)| v.clone()).collect(),
                any if is_str(any, "any") => {
                    let mut types: Vec<PhpValue> = Vec::new();
                    for t in own_types {
                        if !types
                            .iter()
                            .any(|seen| to_php_string(seen) == to_php_string(&t))
                        {
                            types.push(t);
                        }
                    }
                    types
                }
                one => vec![one.clone()],
            };
            if !types.is_empty() {
                self.d.terms.update_object_term_cache(&ids, &types)?;
            }
        }
        if meta {
            self.update_postmeta_cache(&ids)?;
        }
        Ok(())
    }

    /// `update_post_parent_caches( $posts )` (post.php 8312-8320): the posts' `post_parent`s, `absint`ed, zeros
    /// dropped and `array_unique()`d in order, then `_prime_post_caches( $parent_ids, false )` (the parents and
    /// their meta, no terms). No parents, nothing.
    pub fn update_post_parent_caches(&self, posts: &[Post]) -> Result<(), PostError> {
        let parents: Vec<PostId> = posts
            .iter()
            .map(crate::hierarchy::parent_id)
            .filter(|p| p.0 != 0)
            .collect::<IndexSet<PostId>>()
            .into_iter()
            .collect();
        if parents.is_empty() {
            return Ok(());
        }
        self.prime_post_caches(&parents, false, true)
    }

    /// `update_postmeta_cache( $post_ids )`: `update_meta_cache( 'post', $post_ids )`.
    pub fn update_postmeta_cache(&self, ids: &[PostId]) -> Result<Option<PhpValue>, PostError> {
        let objects: Vec<ObjectId> = ids.iter().map(|id| ObjectId(id.0)).collect();
        self.d
            .metadata
            .update_cache(&Self::post_meta(), &objects)
            .map_err(meta_error)
    }

    /// `_prime_post_parent_id_caches( $ids )`: for each distinct id whose `post_parent:{ID}` entry is a miss,
    /// [`PostRepository::parent_ids`](speedyspec_wp_posts_contracts::PostRepository::parent_ids), and each answer
    /// added as `(int) post_parent` with `wp_cache_add_multiple()`.
    ///
    /// The ids are typed, so WordPress's `_validate_cache_id()` cases cannot reach it: a non-integer string (`"x"`,
    /// dropped with `_doing_it_wrong()`) and a negative id (`-1`, which passes the check and reaches the `IN (…)`
    /// statement and `update_postmeta_cache()`). The PHP binding drops both before calling, so its statement text
    /// can differ from WordPress's for such input (Task 8 lists it as a documented break).
    pub fn prime_post_parent_id_caches(&self, ids: &[PostId]) -> Result<(), PostError> {
        let ids = unique(ids);
        if ids.is_empty() {
            return Ok(());
        }
        let keys: Vec<HookValue> = ids
            .iter()
            .map(|id| HookValue::Str(format!("post_parent:{id}")))
            .collect();
        let cached = self.cache_get_multiple(&keys, "posts")?;
        let non_cached: Vec<PostId> = ids
            .iter()
            .filter(|id| {
                let key = Key::Str(format!("post_parent:{id}"));
                cached
                    .iter()
                    .find(|(k, _)| *k == key)
                    .is_none_or(|(_, v)| matches!(v, PhpValue::Bool(false)))
            })
            .map(|id| PostId(u64::try_from(*id).unwrap_or(0)))
            .collect();
        if non_cached.is_empty() {
            return Ok(());
        }
        let fresh = self.d.repository.parent_ids(&non_cached)?;
        if fresh.is_empty() {
            return Ok(());
        }
        let mut data: Vec<(Key, PhpValue)> = Vec::new();
        for (id, parent) in fresh {
            let key = Key::Str(format!("post_parent:{}", id.0));
            let value = PhpValue::Int(int(parent.0));
            match data.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => data.push((key, value)),
            }
        }
        self.cache_add_multiple(data, "posts")
    }
}

/// PHP's `===` on the scalar values a salt holds; arrays compare element by element.
fn identical(a: &PhpValue, b: &PhpValue) -> bool {
    use PhpValue::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Array(x), Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((kx, vx), (ky, vy))| kx == ky && identical(vx, vy))
        }
        _ => false,
    }
}
