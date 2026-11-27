//! Counts and dates (WordPress 7.2-RC2): `_count_posts_cache_key()` (post.php 3511-3523), `wp_count_posts()`
//! (3545-3654), `wp_count_attachments()` (3677-3747), `get_lastpostdate()` (8006-8018), `get_lastpostmodified()`
//! (8040-8075), `_get_last_post_time()` (8096-8145) and `get_available_post_mime_types()` (8889-8909).

use speedyspec_php_serialize::semantics::{isset, set, to_array_key, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{LastPostField, MimeFilter, PostError, PostTypeName, Timezone};

use crate::hook::hv;
use crate::read::api::{PostsApi, is_std_class, prop, s, truthy};
use crate::{post_status_names, post_type_names, sanitize_key};

/// `(object) $value`: an array's entries become properties (keys as strings), an object stays, null is an empty
/// `stdClass`, and a scalar is held in `scalar`.
fn to_object(value: PhpValue) -> PhpValue {
    let properties = match value {
        object @ PhpValue::Object { .. } => return object,
        PhpValue::Array(items) => items
            .into_iter()
            .map(|(k, v)| {
                (
                    PhpKey::Str(
                        speedyspec_php_serialize::semantics::key_to_string(&k).into_bytes(),
                    ),
                    v,
                )
            })
            .collect(),
        PhpValue::Null => Vec::new(),
        scalar => vec![(PhpKey::Str(b"scalar".to_vec()), scalar)],
    };
    PhpValue::Object {
        class: "stdClass".to_owned(),
        properties,
    }
}

/// `foreach ( $names as $name ) if ( ! isset( $object->$name ) ) $object->$name = 0;` on an object; any other value
/// is left as it is (PHP would fail to write a property on it).
fn fill_zero(object: &mut PhpValue, names: &[Vec<u8>]) {
    if let PhpValue::Object { properties, .. } = object {
        for name in names {
            let name = String::from_utf8_lossy(name);
            if isset(properties, &name).is_none() {
                set(properties, &name, PhpValue::Int(0));
            }
        }
    }
}

/// `$value > $other` for the values `_get_last_post_time()` and its filter give: two numeric strings or numbers
/// compare as numbers, two other strings byte by byte, and a bool or null against anything as booleans. Other pairs
/// compare their string forms, an approximation of PHP 8's rules.
fn greater(a: &PhpValue, b: &PhpValue) -> bool {
    use PhpValue::*;
    let number = |v: &PhpValue| -> Option<f64> {
        match v {
            Int(i) => Some(*i as f64),
            Float(f) => Some(*f),
            Str(b) if crate::read::get::is_numeric(v) => {
                std::str::from_utf8(b).ok()?.trim().parse().ok()
            }
            _ => None,
        }
    };
    match (a, b) {
        (Bool(_) | Null, _) | (_, Bool(_) | Null) => truthy(a) && !truthy(b),
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => x > y,
            _ => to_php_string(a) > to_php_string(b),
        },
    }
}

impl PostsApi {
    /// `get_post_stati()`'s names, in registration order.
    fn status_names(&self) -> Vec<Vec<u8>> {
        post_status_names(&*self.d.statuses, &[], "and")
            .into_iter()
            .map(|(_, name)| to_php_string(&name))
            .collect()
    }

    /// `get_post_type_object( $type )->cap->read_private_posts`.
    fn read_private_cap(&self, post_type: &str) -> PhpValue {
        self.d
            .types
            .post_type(post_type)
            .and_then(|t| {
                t.get("cap")
                    .and_then(|cap| prop(cap, "read_private_posts"))
                    .cloned()
            })
            .unwrap_or(PhpValue::Null)
    }

    /// `_count_posts_cache_key( $type, $perm )`: `posts-{type}`, plus `_readable_{user id}` for a logged-in reader
    /// under `readable` who cannot read the type's private posts.
    pub fn count_posts_cache_key(&self, post_type: &str, perm: &str) -> Result<String, PostError> {
        let mut key = format!("posts-{post_type}");
        if perm == "readable"
            && self.d.users.is_user_logged_in()?
            && self.d.types.post_type(post_type).is_some()
        {
            let cap = self.read_private_cap(post_type);
            if !self.d.users.current_user_can(&cap)? {
                key.push_str(&format!("_{perm}_{}", self.d.users.get_current_user_id()?));
            }
        }
        Ok(key)
    }

    /// `wp_count_posts( $type, $perm )` as WordPress returns it, step by step:
    /// 1. an unregistered type gives an empty `stdClass`;
    /// 2. a `stdClass` from `pre_wp_count_posts` is copied, every registered status it lacks set to 0, and filtered;
    /// 3. the `counts` entry [`PostsApi::count_posts_cache_key`] names, when not `false`, likewise;
    /// 4. otherwise [`PostRepository::count_by_status`](speedyspec_wp_posts_contracts::PostRepository::count_by_status),
    ///    with the reader's id for a logged-in `readable` reader who cannot read private posts: every registered
    ///    status at 0 in `get_post_stati()` order, each counted status set to its count as a numeric string (as
    ///    `$wpdb` returns it), cached as a `stdClass` with `wp_cache_set()`, then the `wp_count_posts` filter with
    ///    `( $counts, $type, $perm )`.
    pub fn wp_count_posts(&self, post_type: &str, perm: &str) -> Result<PhpValue, PostError> {
        if self.d.types.post_type(post_type).is_none() {
            return Ok(to_object(PhpValue::Null));
        }
        let args = || {
            vec![
                HookValue::Str(post_type.to_owned()),
                HookValue::Str(perm.to_owned()),
            ]
        };
        let pre = self.filter("pre_wp_count_posts", PhpValue::Null, args)?;
        if is_std_class(&pre) {
            let mut counts = pre;
            fill_zero(&mut counts, &self.status_names());
            return self.filter("wp_count_posts", counts, args);
        }

        let cache_key = HookValue::Str(self.count_posts_cache_key(post_type, perm)?);
        if let Some(cached) = self.cache_get(&cache_key, "counts")?
            && !matches!(cached, PhpValue::Bool(false))
        {
            let mut counts = cached;
            fill_zero(&mut counts, &self.status_names());
            return self.filter("wp_count_posts", counts, args);
        }

        let reader = if perm == "readable" && self.d.users.is_user_logged_in()? {
            let cap = self.read_private_cap(post_type);
            if self.d.users.current_user_can(&cap)? {
                None
            } else {
                Some(self.d.users.get_current_user_id()?)
            }
        } else {
            None
        };
        let rows = self
            .d
            .repository
            .count_by_status(&PostTypeName::new(post_type), reader)?;
        let mut counts: Vec<(PhpKey, PhpValue)> = Vec::new();
        let mut put = |name: &[u8], value: PhpValue| {
            let key = to_array_key(&s(name)).expect("a string is a key");
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => counts.push((key, value)),
            }
        };
        for name in self.status_names() {
            put(&name, PhpValue::Int(0));
        }
        for (status, count) in rows {
            put(status.as_str().as_bytes(), s(count.to_string()));
        }
        let counts = to_object(PhpValue::Array(counts));
        self.cache_set(&cache_key, &counts, "counts")?;
        self.filter("wp_count_posts", counts, args)
    }

    /// `wp_count_attachments( $mime_type )` as WordPress returns it:
    /// 1. a `stdClass` from `pre_wp_count_attachments` is copied, `trash` set to 0 when missing, and filtered;
    /// 2. the `counts` entry `attachments` (plus `:` and `implode( '-', (array) $mime_type )` with `/` as `_`, for a
    ///    non-empty filter), when not `false`;
    /// 3. otherwise [`PostRepository::count_attachments`](speedyspec_wp_posts_contracts::PostRepository::count_attachments):
    ///    each mime type with its count, then `trash`, as numeric strings, cached as a `stdClass`;
    ///
    /// then the `wp_count_attachments` filter with `( (object) $counts, $mime_type )`. `None` is `''`, the default.
    pub fn wp_count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<PhpValue, PostError> {
        let original = match mime_type {
            None => s(""),
            Some(MimeFilter::One(text)) => s(text),
            Some(MimeFilter::Many(items)) => PhpValue::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (PhpKey::Int(i as i64), s(m)))
                    .collect(),
            ),
        };
        let args = || vec![hv(&original)];
        let pre = self.filter("pre_wp_count_attachments", PhpValue::Null, args)?;
        if is_std_class(&pre) {
            let mut counts = pre;
            fill_zero(&mut counts, &[b"trash".to_vec()]);
            return self.filter("wp_count_attachments", counts, args);
        }

        let suffix = if truthy(&original) {
            let joined = match mime_type {
                Some(MimeFilter::Many(items)) => items.join("-"),
                Some(MimeFilter::One(text)) => text.clone(),
                None => String::new(),
            };
            format!(":{}", joined.replace('/', "_"))
        } else {
            String::new()
        };
        let cache_key = HookValue::Str(format!("attachments{suffix}"));
        let counts = match self.cache_get(&cache_key, "counts")? {
            Some(cached) if !matches!(cached, PhpValue::Bool(false)) => cached,
            _ => {
                let (rows, trash) = self.d.repository.count_attachments(mime_type)?;
                let mut counts: Vec<(PhpKey, PhpValue)> = Vec::new();
                for (mime, count) in rows {
                    let key = to_array_key(&s(mime.as_bytes())).expect("a string is a key");
                    match counts.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = s(count.to_string()),
                        None => counts.push((key, s(count.to_string()))),
                    }
                }
                let key = PhpKey::Str(b"trash".to_vec());
                match counts.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = s(trash.to_string()),
                    None => counts.push((key, s(trash.to_string()))),
                }
                let counts = to_object(PhpValue::Array(counts));
                self.cache_set(&cache_key, &counts, "counts")?;
                counts
            }
        };
        self.filter("wp_count_attachments", to_object(counts), args)
    }

    /// `sanitize_key( $key )`: the rule, then the `sanitize_key` filter with `( $sanitized, $key )`.
    fn sanitize_key_filtered(&self, raw: &str) -> Result<PhpValue, PostError> {
        let clean = s(sanitize_key(raw.as_bytes()));
        self.filter("sanitize_key", clean, || {
            vec![HookValue::Str(raw.to_owned())]
        })
    }

    /// `_get_last_post_time( $timezone, $field, $post_type )`: the `timeinfo` entry
    /// `lastpost{field}:{timezone}[:{sanitize_key( $post_type )}]` when not `false`; otherwise the newest published
    /// date among the public post types (for `any`) or the one given, through
    /// [`PostRepository::last_post_time`](speedyspec_wp_posts_contracts::PostRepository::last_post_time), cached
    /// when truthy. An unknown timezone sends no statement. `false` when there is no date.
    pub fn last_post_time(
        &self,
        timezone: &str,
        field: LastPostField,
        post_type: &str,
    ) -> Result<PhpValue, PostError> {
        let timezone = timezone.to_ascii_lowercase();
        let mut key = format!("lastpost{}:{timezone}", field.as_str());
        if post_type != "any" {
            key.push(':');
            key.push_str(&String::from_utf8_lossy(&to_php_string(
                &self.sanitize_key_filtered(post_type)?,
            )));
        }
        let key = HookValue::Str(key);
        if let Some(date) = self.cache_get(&key, "timeinfo")?
            && !matches!(date, PhpValue::Bool(false))
        {
            return Ok(date);
        }
        let types: Vec<PostTypeName> = if post_type == "any" {
            let public = [(PhpKey::Str(b"public".to_vec()), PhpValue::Bool(true))];
            post_type_names(&*self.d.types, &public, "and")
                .into_iter()
                .map(|(_, name)| PostTypeName::new(String::from_utf8_lossy(&to_php_string(&name))))
                .collect()
        } else {
            let name = self.sanitize_key_filtered(post_type)?;
            vec![PostTypeName::new(String::from_utf8_lossy(&to_php_string(
                &name,
            )))]
        };
        let date = match Timezone::parse(&timezone) {
            Some(zone) => self
                .d
                .repository
                .last_post_time(field, &types, zone)?
                .map(|d| s(d.as_str())),
            None => None,
        };
        match date {
            Some(date) if truthy(&date) => {
                self.cache_set(&key, &date, "timeinfo")?;
                Ok(date)
            }
            _ => Ok(PhpValue::Bool(false)),
        }
    }

    /// `get_lastpostdate( $timezone, $post_type )`: [`PostsApi::last_post_time`] for `date`, then the
    /// `get_lastpostdate` filter with `( $date, $timezone, $post_type )`.
    pub fn get_lastpostdate(&self, timezone: &str, post_type: &str) -> Result<PhpValue, PostError> {
        let date = self.last_post_time(timezone, LastPostField::Date, post_type)?;
        self.filter("get_lastpostdate", date, || {
            vec![
                HookValue::Str(timezone.to_owned()),
                HookValue::Str(post_type.to_owned()),
            ]
        })
    }

    /// `get_lastpostmodified( $timezone, $post_type )`: a non-`false` `pre_get_lastpostmodified` answer is returned;
    /// otherwise the `modified` time, raised to `get_lastpostdate()` when that is greater, then the
    /// `get_lastpostmodified` filter.
    pub fn get_lastpostmodified(
        &self,
        timezone: &str,
        post_type: &str,
    ) -> Result<PhpValue, PostError> {
        let args = || {
            vec![
                HookValue::Str(timezone.to_owned()),
                HookValue::Str(post_type.to_owned()),
            ]
        };
        let pre = self.filter("pre_get_lastpostmodified", PhpValue::Bool(false), args)?;
        if !matches!(pre, PhpValue::Bool(false)) {
            return Ok(pre);
        }
        let mut modified = self.last_post_time(timezone, LastPostField::Modified, post_type)?;
        let date = self.get_lastpostdate(timezone, post_type)?;
        if greater(&date, &modified) {
            modified = date;
        }
        self.filter("get_lastpostmodified", modified, args)
    }

    /// `get_available_post_mime_types( $type )`: an array from `pre_get_available_post_mime_types` replaces the
    /// statement ([`PostRepository::available_mime_types`](speedyspec_wp_posts_contracts::PostRepository::available_mime_types));
    /// then the falsy values are dropped and the rest renumbered.
    pub fn get_available_post_mime_types(
        &self,
        post_type: &str,
    ) -> Result<Vec<PhpValue>, PostError> {
        let pre = self.filter("pre_get_available_post_mime_types", PhpValue::Null, || {
            vec![HookValue::Str(post_type.to_owned())]
        })?;
        let types: Vec<PhpValue> = match pre {
            PhpValue::Array(items) => items.into_iter().map(|(_, v)| v).collect(),
            _ => self
                .d
                .repository
                .available_mime_types(&PostTypeName::new(post_type))?
                .iter()
                .map(|m| s(m.as_bytes()))
                .collect(),
        };
        Ok(types.into_iter().filter(truthy).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_compare_as_php_does() {
        assert!(greater(
            &s("2026-01-02 00:00:00"),
            &s("2026-01-01 00:00:00")
        ));
        assert!(!greater(
            &s("2026-01-01 00:00:00"),
            &s("2026-01-02 00:00:00")
        ));
        assert!(greater(&s("2026-01-01 00:00:00"), &PhpValue::Bool(false)));
        assert!(!greater(&PhpValue::Bool(false), &s("2026-01-01 00:00:00")));
        assert!(greater(&s("10"), &s("9")));
    }

    #[test]
    fn object_cast() {
        let o = to_object(PhpValue::Array(vec![(PhpKey::Int(5), s("x"))]));
        assert_eq!(
            o,
            PhpValue::Object {
                class: "stdClass".into(),
                properties: vec![(PhpKey::Str(b"5".to_vec()), s("x"))]
            }
        );
    }
}
