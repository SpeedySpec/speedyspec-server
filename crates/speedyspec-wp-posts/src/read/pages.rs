//! `get_all_page_ids()` (post.php 6430-6440), `get_page_by_path()` (6497-6599) and `get_page_uri()` (6700-6727),
//! WordPress 7.2-RC2.

use md5::{Digest, Md5};
use speedyspec_php_serialize::semantics::to_php_int;
use speedyspec_php_serialize::{PhpKey, PhpValue, serialize};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{PageRow, Post, PostError, PostId, PostRef, PostTypeArg, Slug};

use crate::hook::hv;
use crate::page_uri;
use crate::read::api::{PostsApi, s};
use crate::read::get::Reader;

/// The bytes' MD5 as 32 lowercase hexadecimal characters.
fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// PHP's `urldecode()`: `+` is a space and each valid `%XX` its byte; an invalid escape stays as it is.
pub(crate) fn urldecode(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    out
}

/// PHP's `rawurlencode()` (RFC 3986): every byte but `A-Z a-z 0-9 - _ . ~` as `%XX`, upper-case.
pub(crate) fn rawurlencode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 3);
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b);
        } else {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        }
    }
    out
}

/// `str_replace( $from, $to, $subject )` on bytes.
fn replace(subject: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(subject.len());
    let mut i = 0;
    while i < subject.len() {
        if subject[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(subject[i]);
            i += 1;
        }
    }
    out
}

/// `trim( $value, '/' )`.
fn trim_slashes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| *b != b'/').unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| *b != b'/')
        .map_or(start, |e| e + 1);
    &bytes[start..end.max(start)]
}

/// `serialize( $post_type )`: a string, or a list of strings.
fn serialized_types(post_type: &PostTypeArg) -> Vec<u8> {
    match post_type {
        PostTypeArg::One(name) => serialize(&s(name.as_str())),
        PostTypeArg::Many(names) => serialize(&PhpValue::Array(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| (PhpKey::Int(i as i64), s(n.as_str())))
                .collect(),
        )),
    }
}

/// `$row->post_name === $part`: only a string part can be identical to the stored name.
fn name_is(row: &PageRow, part: Option<&PhpValue>) -> bool {
    matches!(part, Some(PhpValue::Str(b)) if b.as_slice() == row.name.as_bytes())
}

impl PostsApi {
    /// `get_all_page_ids()`: the `posts` group's `all_page_ids` entry when it is an array, otherwise
    /// `SELECT ID … WHERE post_type = 'page'` as `$wpdb->get_col()` returns it (decimal strings), added to the
    /// cache with `wp_cache_add()`.
    pub fn get_all_page_ids(&self) -> Result<Vec<PhpValue>, PostError> {
        let key = HookValue::Str("all_page_ids".into());
        if let Some(PhpValue::Array(items)) = self.cache_get(&key, "posts")? {
            return Ok(items.into_iter().map(|(_, v)| v).collect());
        }
        let ids: Vec<PhpValue> = self
            .d
            .repository
            .all_page_ids()?
            .iter()
            .map(|id| s(id.0.to_string()))
            .collect();
        let list = PhpValue::Array(
            ids.iter()
                .enumerate()
                .map(|(i, v)| (PhpKey::Int(i as i64), v.clone()))
                .collect(),
        );
        self.cache_add(&key, &list, "posts")?;
        Ok(ids)
    }

    /// `sanitize_title_for_query( $title )`: `sanitize_title( $title, '', 'query' )`, which is the `sanitize_title`
    /// filter with `( $title, $title, 'query' )`, then the fallback `''` for an answer of `''` or `false`.
    fn sanitize_title_for_query(&self, title: &[u8]) -> Result<PhpValue, PostError> {
        let answer = self.filter("sanitize_title", s(title), || {
            vec![hv(&s(title)), HookValue::Str("query".into())]
        })?;
        Ok(match answer {
            PhpValue::Bool(false) => s(""),
            other => other,
        })
    }

    /// `get_page_by_path( $page_path, OBJECT, $post_type )`, step by step:
    /// 1. `wp_cache_get_last_changed( 'posts' )` is the salt;
    /// 2. the `post-queries` entry `get_page_by_path:` + `md5( $page_path . serialize( $post_type ) )`, salted:
    ///    `'0'` or `0` is a known miss (`None`), any other value is read with `get_post( (int) $cached )`;
    /// 3. the path is `rawurlencode( urldecode() )`d, `%2F` and `%20` put back, trimmed of `/` and split on it, and
    ///    each part goes through `sanitize_title_for_query()`;
    /// 4. [`PostRepository::pages_by_names`](speedyspec_wp_posts_contracts::PostRepository::pages_by_names);
    /// 5. the rows are walked in the statement's order: a row whose name is the last part (`===`, so case counts
    ///    here and the `sanitize_title` filter decides what matches) and whose parents, found among the rows, name
    ///    the other parts up to a top-level page, is the answer; [`PostTypeArg`] decides when the walk stops;
    /// 6. the answer, the row's `ID` as `$wpdb` returns it (a string), or `0` for none, is cached salted.
    pub fn get_page_by_path(
        &self,
        page_path: &[u8],
        post_type: &PostTypeArg,
    ) -> Result<Option<Post>, PostError> {
        let last_changed = self.wp_cache_get_last_changed("posts")?;
        let hash = md5_hex(&[page_path, &serialized_types(post_type)].concat());
        let cache_key = format!("get_page_by_path:{hash}");
        if let Some(cached) = self.wp_cache_get_salted(&cache_key, "post-queries", &last_changed)? {
            if matches!(&cached, PhpValue::Int(0))
                || matches!(&cached, PhpValue::Str(b) if b == b"0")
            {
                return Ok(None);
            }
            return self.get_post(PostRef::Id(id_of(to_php_int(&cached))));
        }

        let path = rawurlencode(&urldecode(page_path));
        let path = replace(&path, b"%2F", b"/");
        let path = replace(&path, b"%20", b" ");
        let mut parts = Vec::new();
        for part in trim_slashes(&path).split(|b| *b == b'/') {
            parts.push(self.sanitize_title_for_query(part)?);
        }
        let names: Vec<Slug> = parts
            .iter()
            .map(|p| Slug::new(speedyspec_php_serialize::semantics::to_php_string(p)))
            .collect();
        let pages = self
            .d
            .repository
            .pages_by_names(&names, &post_type.types())?;

        let revparts: Vec<&PhpValue> = parts.iter().rev().collect();
        let by_id = |id: u64| pages.iter().find(|p| p.id.0 == id);
        let mut found_id: Option<u64> = None;
        for page in &pages {
            if !name_is(page, revparts.first().copied()) {
                continue;
            }
            let mut count = 0;
            let mut p = page;
            while p.parent.0 != 0 {
                let Some(parent) = by_id(p.parent.0) else {
                    break;
                };
                count += 1;
                if !name_is(parent, revparts.get(count).copied()) {
                    break;
                }
                p = parent;
            }
            if p.parent.0 == 0
                && revparts.len() == count + 1
                && name_is(p, revparts.get(count).copied())
            {
                found_id = Some(page.id.0);
                let stop = match post_type {
                    PostTypeArg::Many(_) => true,
                    PostTypeArg::One(name) => page.post_type == *name,
                };
                if stop {
                    break;
                }
            }
        }

        let data = match found_id {
            Some(id) => s(id.to_string()),
            None => PhpValue::Int(0),
        };
        self.wp_cache_set_salted(&cache_key, data, "post-queries", &last_changed)?;
        match found_id {
            Some(id) => self.get_post(PostRef::Id(PostId(id))),
            None => Ok(None),
        }
    }

    /// `get_page_uri( $page )` as WordPress returns it: `false` with no post, otherwise the `get_page_uri` filter's
    /// answer over the path of names ([`crate::page_uri`]).
    pub fn get_page_uri(&self, page: PostRef) -> Result<PhpValue, PostError> {
        let Some(loaded) = self.resolve(page)? else {
            return Ok(PhpValue::Bool(false));
        };
        let uri = page_uri(&Reader(self), &loaded.post)?;
        self.filter("get_page_uri", s(uri), || vec![hv(&loaded.object())])
    }
}

/// `(int) $value` as a post id: a negative id is 0, which `get_post()` treats as empty.
fn id_of(value: i64) -> PostId {
    PostId(u64::try_from(value).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_round_trip_keeps_php_rules() {
        assert_eq!(urldecode(b"caf%C3%A9+x%2"), b"caf\xc3\xa9 x%2");
        assert_eq!(urldecode(b"%zz%41"), b"%zzA");
        assert_eq!(rawurlencode("café/a b~".as_bytes()), b"caf%C3%A9%2Fa%20b~");
        assert_eq!(trim_slashes(b"//about/team/"), b"about/team");
        assert_eq!(trim_slashes(b"/"), b"");
    }

    #[test]
    fn cache_key_matches_wordpress() {
        // Recorded in compat/posts-read page-path-cache.
        let one = PostTypeArg::One(speedyspec_wp_posts_contracts::PostTypeName::new("page"));
        assert_eq!(
            md5_hex(&[b"about/team".as_slice(), &serialized_types(&one)].concat()),
            "291347b13b493a4537ef2fcd911be425"
        );
        assert_eq!(
            md5_hex(&[b"nope".as_slice(), &serialized_types(&one)].concat()),
            "89b57f44d88ecc0737e96622a191d046"
        );
    }
}
