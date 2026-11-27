//! The pings, the enclosures, `is_sticky()` and `_page_traverse_name()` (WordPress 7.2-RC2): `is_sticky()`
//! (post.php 3000-3025), `get_enclosed()` (6298-6327), `get_pung()` (6338-6356), `get_to_ping()` (6367-6386) and
//! `_page_traverse_name()` (6680-6687), with `get_post_custom()` (2918-2929) and `get_the_ID()`
//! (post-template.php 27-30) as they read.

use indexmap::IndexMap;
use speedyspec_php_serialize::semantics::{get, to_array_key, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta_contracts::ObjectId;
use speedyspec_wp_posts_contracts::{PostError, PostRef};

use crate::hook::hv;
use crate::read::api::{PostsApi, meta_error, s};

/// PHP's `trim()` characters.
fn is_trimmed(b: &u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0b)
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !is_trimmed(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !is_trimmed(b))
        .map_or(start, |e| e + 1);
    &bytes[start..end.max(start)]
}

/// `preg_split( '/\s/', $text, -1, $no_empty ? PREG_SPLIT_NO_EMPTY : 0 )`: PCRE's `\s` is space, `\t`, `\n`, `\v`,
/// `\f` and `\r`.
fn split_space(text: &[u8], no_empty: bool) -> PhpValue {
    let parts = text
        .split(|b| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .filter(|p| !no_empty || !p.is_empty());
    list(parts.map(s))
}

fn list(items: impl Iterator<Item = PhpValue>) -> PhpValue {
    PhpValue::Array(
        items
            .enumerate()
            .map(|(i, v)| (PhpKey::Int(i as i64), v))
            .collect(),
    )
}

/// `absint( $value )` as an int.
fn absint(value: &PhpValue) -> i64 {
    to_php_int(value).checked_abs().unwrap_or(i64::MAX)
}

impl PostsApi {
    /// `get_the_ID()`: the current post's `ID`, `None` (false) when there is none.
    fn the_id(&self) -> Result<Option<i64>, PostError> {
        Ok(self.resolve(PostRef::Current)?.map(|l| l.id()))
    }

    /// A post's property as `$post->$name` reads a declared one: `''` when it is missing.
    fn post_text(&self, post: PostRef, name: &str) -> Result<Option<Vec<u8>>, PostError> {
        Ok(self.resolve(post)?.map(|l| {
            get(&l.properties, name)
                .map(to_php_string)
                .unwrap_or_default()
        }))
    }

    /// `get_pung( $post )`: `false` with no post, otherwise `preg_split( '/\s/', trim( $post->pinged ) )` through the
    /// `get_pung` filter.
    pub fn get_pung(&self, post: PostRef) -> Result<PhpValue, PostError> {
        let Some(pinged) = self.post_text(post, "pinged")? else {
            return Ok(PhpValue::Bool(false));
        };
        let pung = split_space(trim(&pinged), false);
        self.filter("get_pung", pung, Vec::new)
    }

    /// `get_to_ping( $post )`: `false` with no post, otherwise `sanitize_trackback_urls( $post->to_ping )` (the
    /// [`Formatting`](crate::Formatting) port) split on whitespace with no empty parts, through the `get_to_ping`
    /// filter.
    pub fn get_to_ping(&self, post: PostRef) -> Result<PhpValue, PostError> {
        let Some(to_ping) = self.post_text(post, "to_ping")? else {
            return Ok(PhpValue::Bool(false));
        };
        let sanitized = self.d.formatting.sanitize_trackback_urls(s(to_ping))?;
        let to_ping = split_space(&to_php_string(&sanitized), true);
        self.filter("get_to_ping", to_ping, Vec::new)
    }

    /// `get_enclosed( $post_id )`: the first line, trimmed, of each `enclosure` meta value of `get_post_custom(
    /// $post_id )` (the current post's when `absint( $post_id )` is 0), through the `get_enclosed` filter with
    /// `( $pung, $post_id )`. No post means no custom fields, so an empty list.
    pub fn get_enclosed(&self, post_id: &PhpValue) -> Result<PhpValue, PostError> {
        let mut id = absint(post_id);
        if id == 0 {
            id = self.the_id()?.unwrap_or(0);
        }
        let mut pung = Vec::new();
        if id > 0 {
            let custom = self
                .d
                .metadata
                .get(&Self::post_meta(), ObjectId(id as u64), None, false)
                .map_err(meta_error)?;
            if let Some(PhpValue::Array(values)) = match &custom {
                PhpValue::Array(fields) => get(fields, "enclosure"),
                _ => None,
            } {
                for (_, enclosure) in values {
                    let text = to_php_string(enclosure);
                    let first = text.split(|b| *b == b'\n').next().unwrap_or_default();
                    pung.push(s(trim(first)));
                }
            }
        }
        let post_id = post_id.clone();
        self.filter("get_enclosed", list(pung.into_iter()), || {
            vec![hv(&post_id)]
        })
    }

    /// `is_sticky( $post_id )`: `absint( $post_id )`, or `get_the_ID()` for 0 (`false` with no current post, which
    /// matches nothing); whether the `sticky_posts` option is an array holding it after `intval` (strict
    /// `in_array`), through the `is_sticky` filter with `( $is_sticky, $post_id )`.
    pub fn is_sticky(&self, post_id: &PhpValue) -> Result<PhpValue, PostError> {
        let mut id = Some(absint(post_id));
        if id == Some(0) {
            id = self.the_id()?;
        }
        let stickies = self.d.options.get_option("sticky_posts")?;
        let sticky = match (&stickies, id) {
            (PhpValue::Array(items), Some(id)) => items.iter().any(|(_, v)| to_php_int(v) == id),
            _ => false,
        };
        let post_id = id.map_or(PhpValue::Bool(false), PhpValue::Int);
        self.filter("is_sticky", PhpValue::Bool(sticky), || vec![hv(&post_id)])
    }

    /// `_page_traverse_name( $page_id, $children, $result )`: `$result` after `$result[ $child->ID ] =
    /// $child->post_name` for each child below `page_id` in `$children` (parent key => list of page objects), depth
    /// first. A child already on the path is listed but not descended into, where PHP recurses forever (as the page
    /// tree rules stop).
    pub fn page_traverse_name(
        &self,
        page_id: &PhpValue,
        children: &PhpValue,
        result: PhpValue,
    ) -> Result<PhpValue, PostError> {
        let mut out: IndexMap<PhpKey, PhpValue> = match result {
            PhpValue::Array(items) => items.into_iter().collect(),
            _ => IndexMap::new(),
        };
        let children: Vec<(PhpKey, PhpValue)> = match children {
            PhpValue::Array(items) => items.clone(),
            _ => Vec::new(),
        };
        let key = to_array_key(page_id).unwrap_or(PhpKey::Str(Vec::new()));
        let mut path = vec![key.clone()];
        traverse(&key, &children, &mut out, &mut path);
        Ok(PhpValue::Array(out.into_iter().collect()))
    }
}

fn traverse(
    key: &PhpKey,
    children: &[(PhpKey, PhpValue)],
    out: &mut IndexMap<PhpKey, PhpValue>,
    path: &mut Vec<PhpKey>,
) {
    let Some((_, PhpValue::Array(list))) = children.iter().find(|(k, _)| k == key) else {
        return;
    };
    for (_, child) in list {
        let PhpValue::Object { properties, .. } = child else {
            continue;
        };
        let id = get(properties, "ID").cloned().unwrap_or(PhpValue::Null);
        let name = get(properties, "post_name")
            .cloned()
            .unwrap_or(PhpValue::Null);
        let child_key = to_array_key(&id).unwrap_or(PhpKey::Str(Vec::new()));
        out.insert(child_key.clone(), name);
        if !path.contains(&child_key) {
            path.push(child_key.clone());
            traverse(&child_key, children, out, path);
            path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_as_preg_split() {
        assert_eq!(
            split_space(b"a  b", false),
            list([s("a"), s(""), s("b")].into_iter())
        );
        assert_eq!(split_space(b"", false), list([s("")].into_iter()));
        assert_eq!(
            split_space(b"a\n\tb", true),
            list([s("a"), s("b")].into_iter())
        );
        assert_eq!(trim(b" \0x\x0b\n"), b"x");
    }
}
