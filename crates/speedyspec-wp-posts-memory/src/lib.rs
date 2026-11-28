//! An in-memory post repository, for tests and for callers with no database.
//!
//! Rows live in insert order, which is also the "engine's order" for statements with no `ORDER BY`. `LIMIT 1` without
//! `ORDER BY` answers the first matching row in that order. Names, types and mime types are compared byte for byte
//! (the SQL repository and `sanitize_title` handle case), and the auto-increment continues past an explicit ID, as MySQL's does.
//!
//! Writes behave as MySQL does under `wpdb::set_sql_mode()`'s mode: a negative parent is stored as `0`, a `NULL`
//! parent fails an insert and is stored as `0` by an update, and an update counts the rows it changed. `NOW()` is the
//! system clock in UTC.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError, PostId,
    PostParent, PostRepository, PostStatusName, PostTypeName, Slug, Timezone,
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

    /// `UPDATE ... WHERE ID = id`: `change` edits the row, and the answer is the rows changed (0 when the row is
    /// missing or `change` left it as it was).
    fn change_row(&self, id: PostId, change: impl FnOnce(&mut Post)) -> Option<u64> {
        let mut state = self.state.borrow_mut();
        let Some(row) = state.rows.iter_mut().find(|p| p.id == id) else {
            return Some(0);
        };
        let before = row.clone();
        change(row);
        Some(u64::from(*row != before))
    }

    /// `UPDATE ... SET post_parent = to WHERE post_parent = from AND post_type = post_type`: the rows changed.
    fn reparent(&self, from: PostId, to: PostParent, post_type: &str) -> Option<u64> {
        let to = to.stored();
        let mut state = self.state.borrow_mut();
        let mut changed = 0;
        for row in state
            .rows
            .iter_mut()
            .filter(|p| p.parent == PostParent::from(from) && p.post_type.as_str() == post_type)
        {
            if row.parent != to {
                row.parent = to;
                changed += 1;
            }
        }
        Some(changed)
    }

    /// The ids of the rows that `matches`, in insert order.
    fn ids_where(&self, matches: impl Fn(&Post) -> bool) -> Vec<PostId> {
        self.state
            .borrow()
            .rows
            .iter()
            .filter(|p| matches(p))
            .map(|p| p.id)
            .collect()
    }
}

/// A stored parent as the id the read statements return.
fn parent_id(parent: PostParent) -> PostId {
    parent.id().unwrap_or_default()
}

/// `DATE_SUB( NOW(), INTERVAL 7 DAY )` as `Y-m-d H:i:s`, `NOW()` being the system clock in UTC.
fn a_week_ago() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let at = now.saturating_sub(7 * 86_400);
    let (days, secs) = (at / 86_400, at % 86_400);
    // Howard Hinnant's civil_from_days, for days since 1970-01-01.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
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

    fn insert(&self, post: &Post, import_id: Option<i64>) -> Result<Option<PostId>, PostError> {
        // NULL in the NOT NULL column: error 1048, even outside strict mode.
        if post.parent == PostParent::Null {
            return Ok(None);
        }
        let mut state = self.state.borrow_mut();
        // 0 asks for the next id, and the unsigned column clamps a negative one to 0.
        let id = match import_id.and_then(|id| u64::try_from(id).ok()) {
            Some(id) if id > 0 => id,
            _ => state.last_id + 1,
        };
        if state.rows.iter().any(|p| p.id.0 == id) {
            return Ok(None);
        }
        state.last_id = state.last_id.max(id);
        state.rows.push(Post {
            id: PostId(id),
            parent: post.parent.stored(),
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
        parent: PostParent,
    ) -> Result<Option<Slug>, PostError> {
        // `post_parent = %d`: `null` prints as 0, and a negative value matches no stored (unsigned) parent.
        let parent = PostParent::Int(parent.prepared());
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
            .map(|p| (p.id, parent_id(p.parent)))
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
                parent: parent_id(p.parent),
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
                // MySQL's DATE_ADD answers NULL for a zero date, and text with six fractional digits otherwise.
                Timezone::Server => (!date.gmt.is_zero())
                    .then(|| PostDateTime::new(format!("{}.000000", date.gmt.as_str()))),
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

    fn update(&self, post: &Post) -> Result<Option<u64>, PostError> {
        Ok(self.change_row(post.id, |row| {
            *row = Post {
                id: row.id,
                parent: post.parent.stored(),
                comment_count: row.comment_count,
                ..post.clone()
            };
        }))
    }

    fn update_name(&self, id: PostId, name: &Slug) -> Result<Option<u64>, PostError> {
        Ok(self.change_row(id, |row| row.name = name.clone()))
    }

    fn update_guid(&self, id: PostId, guid: &[u8]) -> Result<Option<u64>, PostError> {
        Ok(self.change_row(id, |row| row.guid = guid.to_vec()))
    }

    fn update_status(&self, id: PostId, status: &PostStatusName) -> Result<Option<u64>, PostError> {
        Ok(self.change_row(id, |row| row.status = status.clone()))
    }

    fn update_pinged(&self, id: PostId, pinged: &[u8]) -> Result<Option<u64>, PostError> {
        Ok(self.change_row(id, |row| row.pinged = pinged.to_vec()))
    }

    fn update_post_type(
        &self,
        id: i64,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        // `WHERE ID = -3` matches no row.
        let Ok(id) = u64::try_from(id) else {
            return Ok(Some(0));
        };
        Ok(self.change_row(PostId(id), |row| row.post_type = post_type.clone()))
    }

    fn import_id_taken(&self, id: i64) -> Result<bool, PostError> {
        let Ok(id) = u64::try_from(id) else {
            return Ok(false);
        };
        Ok(id > 0 && self.state.borrow().rows.iter().any(|p| p.id.0 == id))
    }

    fn raw_row(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.find(id)
    }

    fn children_of_type(
        &self,
        parent: PostId,
        post_type: &PostTypeName,
    ) -> Result<Vec<Post>, PostError> {
        let parent = PostParent::from(parent);
        Ok(self
            .state
            .borrow()
            .rows
            .iter()
            .filter(|p| p.parent == parent && &p.post_type == post_type)
            .cloned()
            .collect())
    }

    fn reparent_children(
        &self,
        from: PostId,
        to: PostParent,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        Ok(self.reparent(from, to, post_type.as_str()))
    }

    fn revision_ids(&self, parent: PostId) -> Result<Vec<PostId>, PostError> {
        let parent = PostParent::from(parent);
        Ok(self.ids_where(|p| p.parent == parent && p.post_type.as_str() == "revision"))
    }

    fn reparent_attachments(&self, from: PostId, to: PostParent) -> Result<Option<u64>, PostError> {
        Ok(self.reparent(from, to, "attachment"))
    }

    fn remove(&self, id: PostId) -> Result<Option<u64>, PostError> {
        let mut state = self.state.borrow_mut();
        let before = state.rows.len();
        state.rows.retain(|p| p.id != id);
        Ok(Some((before - state.rows.len()) as u64))
    }

    fn auto_drafts_older_than_a_week(&self) -> Result<Vec<PostId>, PostError> {
        // `Y-m-d H:i:s` compares as text in date order, and the zero date is older than any.
        let cutoff = a_week_ago();
        Ok(self.ids_where(|p| {
            p.status.as_str() == "auto-draft" && p.date.local.as_str() < cutoff.as_str()
        }))
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
