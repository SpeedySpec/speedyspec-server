//! An in-memory post repository, for tests and for callers with no database.
//!
//! Rows live in insert order, which is also the "engine's order" for statements with no `ORDER BY`. `LIMIT 1` without
//! `ORDER BY` answers the first matching row in that order. Names, types and mime types are compared byte for byte
//! (the SQL repository and `sanitize_title` handle case), and the auto-increment continues past an explicit ID, as MySQL's does.

use std::cell::RefCell;

use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError, PostId,
    PostRepository, PostStatusName, PostTypeName, Slug, Timezone,
};

/// `wp_posts` held in memory (Repository, Strategy for the engine).
pub struct MemoryPostRepository {
    state: RefCell<State>,
}

struct State {
    rows: Vec<Post>,
    last_id: u64,
}

impl MemoryPostRepository {
    /// An empty table whose first id is 1.
    pub fn new() -> MemoryPostRepository {
        MemoryPostRepository {
            state: RefCell::new(State {
                rows: Vec::new(),
                last_id: 0,
            }),
        }
    }

    /// The `post_name` of the first row in insert order that `matches`.
    fn first_name(&self, matches: impl Fn(&Post) -> bool) -> Option<Slug> {
        self.state
            .borrow()
            .rows
            .iter()
            .find(|p| matches(p))
            .map(|p| p.name.clone())
    }
}

impl Default for MemoryPostRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl PostRepository for MemoryPostRepository {
    fn engine(&self) -> String {
        "memory".to_owned()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError> {
        let mut state = self.state.borrow_mut();
        let id = if post.id == PostId(0) {
            state.last_id + 1
        } else {
            post.id.0
        };
        if state.rows.iter().any(|p| p.id.0 == id) {
            return Ok(None);
        }
        state.last_id = state.last_id.max(id);
        state.rows.push(Post {
            id: PostId(id),
            comment_count: 0,
            ..post.clone()
        });
        Ok(Some(PostId(id)))
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| &p.name == slug && p.id != except))
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| {
            &p.name == slug
                && (&p.post_type == post_type || p.post_type.as_str() == "attachment")
                && p.id != except
                && p.parent == parent
        }))
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        Ok(self.first_name(|p| &p.name == slug && &p.post_type == post_type && p.id != except))
    }

    fn find_many(&self, ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .filter(|p| ids.contains(&p.id))
            .cloned()
            .collect())
    }

    fn parent_ids(&self, ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .filter(|p| ids.contains(&p.id))
            .map(|p| (p.id, p.parent))
            .collect())
    }

    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        let state = self.state.borrow();
        let mut rows: Vec<&Post> = state
            .rows
            .iter()
            .filter(|p| names.contains(&p.name) && post_types.contains(&p.post_type))
            .collect();
        // ORDER BY post_status = 'publish' DESC, post_status IN ( 'draft', 'pending', 'auto-draft' ) ASC, ID ASC
        rows.sort_by_key(|p| {
            let status = p.status.as_str();
            (
                status != "publish",
                matches!(status, "draft" | "pending" | "auto-draft"),
                p.id,
            )
        });
        Ok(rows
            .into_iter()
            .map(|p| PageRow {
                id: p.id,
                name: p.name.clone(),
                parent: p.parent,
                post_type: p.post_type.clone(),
            })
            .collect())
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .filter(|p| p.post_type.as_str() == "page")
            .map(|p| p.id)
            .collect())
    }

    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        let state = self.state.borrow();
        let mut counts: Vec<(PostStatusName, u64)> = Vec::new();
        for post in state.rows.iter().filter(|p| &p.post_type == post_type) {
            // The private branch is hard-coded in WordPress 7.2-RC2's statement.
            if let Some(user) = reader
                && post.status.as_str() == "private"
                && post.author != user
            {
                continue;
            }
            match counts.iter_mut().find(|(status, _)| status == &post.status) {
                Some((_, n)) => *n += 1,
                None => counts.push((post.status.clone(), 1)),
            }
        }
        Ok(counts)
    }

    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        let clauses = mime_clauses(mime_type);
        let state = self.state.borrow();
        let mut counts: Vec<(MimeType, u64)> = Vec::new();
        let mut trashed = 0;
        for post in state
            .rows
            .iter()
            .filter(|p| p.post_type.as_str() == "attachment")
            .filter(|p| clauses.matches(p.mime_type.as_bytes()))
        {
            if post.status.as_str() == "trash" {
                trashed += 1;
            } else {
                match counts.iter_mut().find(|(mime, _)| mime == &post.mime_type) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((post.mime_type.clone(), 1)),
                }
            }
        }
        Ok((counts, trashed))
    }

    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        let state = self.state.borrow();
        let mut newest: Option<&Post> = None;
        for post in state
            .rows
            .iter()
            .filter(|p| p.status.as_str() == "publish" && post_types.contains(&p.post_type))
        {
            let date = |p: &Post| match field {
                LastPostField::Date => p.date.gmt.clone(),
                LastPostField::Modified => p.modified.gmt.clone(),
            };
            // ORDER BY post_{field}_gmt DESC LIMIT 1: the first of equal dates stays.
            if newest.is_none_or(|best| date(post) > date(best)) {
                newest = Some(post);
            }
        }
        Ok(newest.and_then(|post| {
            let date = match field {
                LastPostField::Date => &post.date,
                LastPostField::Modified => &post.modified,
            };
            match timezone {
                Timezone::Blog => Some(date.local.clone()),
                Timezone::Gmt => Some(date.gmt.clone()),
                // MySQL's DATE_ADD answers NULL for a zero date.
                Timezone::Server => (!date.gmt.is_zero()).then(|| date.gmt.clone()),
            }
        }))
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        let state = self.state.borrow();
        let mut types: Vec<MimeType> = Vec::new();
        for post in state
            .rows
            .iter()
            .filter(|p| &p.post_type == post_type && !p.mime_type.as_bytes().is_empty())
        {
            if !types.contains(&post.mime_type) {
                types.push(post.mime_type.clone());
            }
        }
        Ok(types)
    }
}

/// What `wp_post_mime_type_where()` builds, as a matcher.
enum MimeClauses {
    /// An empty `$and`: every attachment matches.
    All,
    /// ` AND ( clause OR clause )`: a pattern each, `%` being `LIKE`'s wildcard.
    Any(Vec<Vec<u8>>),
}

impl MimeClauses {
    fn matches(&self, mime: &[u8]) -> bool {
        match self {
            MimeClauses::All => true,
            MimeClauses::Any(patterns) => patterns.iter().any(|p| like(p, mime)),
        }
    }
}

/// `wp_post_mime_type_where()`'s rule, item by item.
fn mime_clauses(filter: Option<&MimeFilter>) -> MimeClauses {
    let items: Vec<String> = match filter {
        None => return MimeClauses::All,
        Some(MimeFilter::One(list)) => list
            .split(',')
            // PHP's `trim()`: space, tab, newline, carriage return, NUL and vertical tab.
            .map(|item| {
                item.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                    .to_owned()
            })
            .filter(|item| !item.is_empty())
            .collect(),
        Some(MimeFilter::Many(items)) => items.clone(),
    };
    let mut patterns = Vec::new();
    for item in items {
        let item: String = item.chars().filter(|c| !is_php_space(*c)).collect();
        if matches!(item.as_str(), "" | "%" | "%/%") {
            return MimeClauses::All;
        }
        let keep = |s: &str, extra: bool| -> String {
            s.chars()
                .filter(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, '-' | '*' | '.')
                        || (extra && *c == '+')
                })
                .collect()
        };
        let pattern = match item.split_once('/') {
            Some((group, subgroup)) => {
                let group = keep(group, false);
                let subgroup = keep(subgroup, true);
                // PHP's `empty()`: `''` and `'0'` are both empty.
                let subgroup = if subgroup.is_empty() || subgroup == "0" {
                    "*".to_owned()
                } else {
                    subgroup
                };
                format!("{group}/{subgroup}")
            }
            None => {
                let mut pattern = keep(&item, false);
                if !pattern.contains('*') {
                    pattern.push_str("/*");
                }
                pattern
            }
        };
        patterns.push(collapse_stars(&pattern).into_bytes());
    }
    if patterns.is_empty() {
        MimeClauses::All
    } else {
        MimeClauses::Any(patterns)
    }
}

/// PHP's `\s` in a non-Unicode regex.
fn is_php_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `preg_replace( '/\*+/', '%', ... )`.
fn collapse_stars(pattern: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for c in pattern.chars() {
        if c == '*' {
            if !in_run {
                out.push('%');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// `post_mime_type LIKE pattern` when the pattern holds `%`, else `post_mime_type = pattern`. The sanitized patterns
/// hold no `_`, so `%` is the only wildcard.
fn like(pattern: &[u8], text: &[u8]) -> bool {
    if !pattern.contains(&b'%') {
        return pattern == text;
    }
    let parts: Vec<&[u8]> = pattern.split(|b| *b == b'%').collect();
    let (first, rest) = parts.split_first().expect("split yields one part");
    let Some(mut tail) = text.strip_prefix(*first) else {
        return false;
    };
    let (last, middle) = rest.split_last().expect("a % gives at least two parts");
    for part in middle {
        match find(tail, part) {
            Some(at) => tail = &tail[at + part.len()..],
            None => return false,
        }
    }
    tail.ends_with(last)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
