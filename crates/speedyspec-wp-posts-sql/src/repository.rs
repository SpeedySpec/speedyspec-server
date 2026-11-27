//! `SqlPostRepository` (Repository): `wp_posts` through the `wpdb` port, one `$wpdb` call per call `post.php` makes.
//!
//! Every statement is `post.php`'s own text (WordPress 7.2-RC2), as the `PostRepository` contract documents it byte
//! for byte, because the `query` filter sees it:
//!
//! - the statements WordPress sends through `$wpdb->prepare()` go through `Wpdb::prepare` with the same template;
//! - the ones it builds with `sprintf()` or concatenation (`_prime_post_caches()`, `get_page_by_path()`,
//!   `wp_count_attachments()`, `_get_last_post_time()`, `get_all_page_ids()`) are built the same way here, quoted
//!   items through `esc_sql()`, and sent with `Wpdb::query_bytes`;
//! - `wp_insert_post()`'s write goes through `$wpdb->insert` with no formats, so `$wpdb->field_types` and charset
//!   stripping are `wpdb`'s.
//!
//! Rows become entities through [`PostMapper`] (Data Mapper). There is no query builder: the seam's adapters speak
//! only the `wpdb` contract. The table is `wpdb.table_name("posts")`, read on every call, so `switch_to_blog` is
//! followed. Statements without `ORDER BY` keep WordPress's text and answer in the engine's order.

use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_db_contracts::{QueryOutcome, WpDbError, WpRow, WpValue, Wpdb, take_hook_error};
use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError, PostId,
    PostRepository, PostStatusName, PostTypeName, Slug, Timezone,
};

use crate::PostMapper;

/// `wp_posts` over `wpdb`.
pub struct SqlPostRepository {
    wpdb: Rc<dyn Wpdb>,
    engine: String,
}

/// A hook's failure (a PHP exception from a `query` callback) is the one failure `$wpdb` does not hide; it travels
/// whole in [`PostError::Hook`], and its payload stays in `wpdb`'s side channel (`take_hook_error()`) for the PHP
/// bridge to rethrow. Every other failure is the empty answer, as `$wpdb` gives it.
fn surfaced(e: WpDbError) -> Result<(), PostError> {
    match e {
        WpDbError::Hook(e) => Err(PostError::Hook(e)),
        _ => Ok(()),
    }
}

/// Puts a taken hook failure back in `wpdb`'s side channel (`take_hook_error()`) for the PHP bridge. The
/// `WpDbError::from` conversion is what records it there; the converted error itself is not needed.
fn restash(hook: impl Into<WpDbError>) {
    let _: WpDbError = hook.into();
}

/// `%d` as PHP's `(int)` gives it: an id past `PHP_INT_MAX` is `PHP_INT_MAX`.
fn int(id: u64) -> WpValue {
    WpValue::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

/// A string argument: text when UTF-8, bytes otherwise.
fn php_string(raw: &[u8]) -> WpValue {
    match String::from_utf8(raw.to_vec()) {
        Ok(text) => WpValue::Str(text),
        Err(e) => WpValue::Bytes(e.into_bytes()),
    }
}

/// The mapper's insert value as `wp_insert_post()`'s `$data` holds it.
fn wp_value(value: DbValue) -> WpValue {
    match value {
        DbValue::Null => WpValue::Null,
        DbValue::Int(i) => WpValue::Int(i),
        DbValue::UInt(u) => int(u),
        DbValue::Text(text) => WpValue::Str(text),
        DbValue::Bytes(bytes) => php_string(&bytes),
        other => other
            .to_mysql_text()
            .map_or(WpValue::Null, |bytes| php_string(&bytes)),
    }
}

/// An id or count column's bytes as a number; anything else is 0, as `(int)` reads it.
fn number(bytes: Option<&Option<Vec<u8>>>) -> u64 {
    bytes
        .and_then(Option::as_ref)
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// The `n`th column's bytes; a `NULL` is `""`, as PHP's `(string) null` is.
fn text(row: &WpRow, n: usize) -> Vec<u8> {
    row.fields
        .get(n)
        .and_then(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn column(row: &WpRow, n: usize) -> Option<&Option<Vec<u8>>> {
    row.fields.get(n).map(|(_, v)| v)
}

/// `wpdb`'s row (text bytes or `NULL`) as the mapper's row.
fn to_row(row: &WpRow) -> Row {
    let columns: Rc<[String]> = row.fields.iter().map(|(n, _)| n.clone()).collect();
    let values = row
        .fields
        .iter()
        .map(|(_, v)| v.clone().map_or(DbValue::Null, DbValue::Bytes))
        .collect();
    Row::new(columns, values)
}

/// `implode( ',', $ids )`, the ids as integers.
fn id_list(ids: &[PostId]) -> String {
    ids.iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// PCRE's `\s` without UTF mode: tab, newline, vertical tab, form feed, carriage return and space.
fn is_pcre_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

/// `preg_replace( '/[^{allowed}]/', '', $text )` for an ASCII class.
fn keep(text: &[u8], allowed: impl Fn(u8) -> bool) -> Vec<u8> {
    text.iter().copied().filter(|&b| allowed(b)).collect()
}

fn group_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'*' | b'.')
}

fn subgroup_byte(b: u8) -> bool {
    group_byte(b) || b == b'+'
}

/// `wp_post_mime_type_where( $post_mime_types )` (post.php, WordPress 7.2-RC2) with no table alias: `''`, or
/// ` AND (` + the clauses joined with ` OR ` + `) `. The rule is the one the `PostRepository::count_attachments`
/// contract states.
fn mime_type_where(filter: Option<&MimeFilter>) -> Vec<u8> {
    let items: Vec<Vec<u8>> = match filter {
        None => Vec::new(),
        Some(MimeFilter::One(text)) => text
            .split(',')
            // PHP's `trim()`: space, tab, newline, carriage return, NUL and vertical tab.
            .map(|item| item.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']))
            .filter(|item| !item.is_empty())
            .map(|item| item.as_bytes().to_vec())
            .collect(),
        Some(MimeFilter::Many(items)) => items.iter().map(|i| i.as_bytes().to_vec()).collect(),
    };
    let mut clauses: Vec<Vec<u8>> = Vec::new();
    for item in items {
        let mime_type = keep(&item, |b| !is_pcre_space(b));
        let pattern = match mime_type.iter().position(|&b| b == b'/') {
            Some(slash) => {
                let group = keep(&mime_type[..slash], group_byte);
                let mut subgroup = keep(&mime_type[slash + 1..], subgroup_byte);
                // PHP's `empty()`: `''` and `'0'` are both empty.
                if subgroup.is_empty() || subgroup == b"0" {
                    subgroup = b"*".to_vec();
                }
                [group.as_slice(), b"/", subgroup.as_slice()].concat()
            }
            None => {
                let mut pattern = keep(&mime_type, group_byte);
                if !pattern.contains(&b'*') {
                    pattern.extend_from_slice(b"/*");
                }
                pattern
            }
        };
        // `preg_replace( '/\*+/', '%', $mime_pattern )`: each run of `*` becomes one `%`.
        let mut collapsed = Vec::with_capacity(pattern.len());
        let mut previous = None;
        for b in pattern {
            if b != b'*' {
                collapsed.push(b);
            } else if previous != Some(b'*') {
                collapsed.push(b'%');
            }
            previous = Some(b);
        }
        if matches!(mime_type.as_slice(), b"" | b"%" | b"%/%") {
            return Vec::new();
        }
        let operator: &[u8] = if collapsed.contains(&b'%') {
            b"post_mime_type LIKE '"
        } else {
            b"post_mime_type = '"
        };
        clauses.push([operator, collapsed.as_slice(), b"'"].concat());
    }
    if clauses.is_empty() {
        return Vec::new();
    }
    [
        b" AND (".as_slice(),
        &clauses.join(b" OR ".as_slice()),
        b") ",
    ]
    .concat()
}

impl SqlPostRepository {
    /// The repository for `$wpdb->posts`. `engine` names the engine behind `wpdb`, as `engine()` reports it.
    pub fn new(wpdb: Rc<dyn Wpdb>, engine: &str) -> SqlPostRepository {
        SqlPostRepository {
            wpdb,
            engine: engine.to_string(),
        }
    }

    /// `$wpdb->posts`, read now.
    fn table(&self) -> String {
        self.wpdb.table_name("posts")
    }

    /// `esc_sql( $text )`: `wpdb::_real_escape()`. The `wpdb` port has no escape method, so it is `prepare( '%s' )`
    /// without the quotes `prepare` adds around the escaped text; a `%` comes back as `wpdb`'s placeholder escape,
    /// which `query` removes before the `query` filter, as WordPress's does.
    fn esc_sql(&self, text: &[u8]) -> Vec<u8> {
        let quoted = self
            .wpdb
            .prepare(Some(b"%s"), &[php_string(text)])
            .expect("prepare answers a one-placeholder template with its one argument");
        match quoted.as_slice() {
            [b'\'', inner @ .., b'\''] => inner.to_vec(),
            _ => quoted,
        }
    }

    /// `"'" . implode( $glue, array_map( 'esc_sql', $items ) ) . "'"`.
    fn quoted_list<'a>(&self, items: impl Iterator<Item = &'a [u8]>, glue: &[u8]) -> Vec<u8> {
        let escaped: Vec<Vec<u8>> = items.map(|item| self.esc_sql(item)).collect();
        [b"'".as_slice(), &escaped.join(glue), b"'"].concat()
    }

    /// `$wpdb->get_results( $wpdb->prepare( $template, ...$args ) )`; no rows where `wpdb` would answer null.
    fn select(&self, template: &str, args: &[WpValue]) -> Result<Rc<Vec<WpRow>>, PostError> {
        let Some(sql) = self.wpdb.prepare(Some(template.as_bytes()), args) else {
            return Ok(Rc::new(Vec::new()));
        };
        self.select_text(&sql)
    }

    /// `$wpdb->get_results( $sql )` for text built without `prepare`.
    fn select_text(&self, sql: &[u8]) -> Result<Rc<Vec<WpRow>>, PostError> {
        match self.wpdb.query_bytes(sql) {
            Ok(QueryOutcome::Rows(rows)) => Ok(rows),
            Ok(_) => Ok(Rc::new(Vec::new())),
            Err(e) => surfaced(e).map(|()| Rc::new(Vec::new())),
        }
    }

    /// `$wpdb->get_var()`: the first row's first column, `None` for no row or `NULL`.
    fn var(rows: &[WpRow]) -> Option<Vec<u8>> {
        rows.first()
            .and_then(|row| column(row, 0))
            .and_then(Clone::clone)
    }

    /// Runs a call, then fails it when a hook failure hid behind its answer.
    ///
    /// `$wpdb` hides a `query` callback's exception raised during a write's charset lookup and answers false or 0,
    /// where WordPress would have thrown and stopped. The call then answers [`PostError::Hook`], so the use case
    /// stops; the failure stays in `take_hook_error()` for the bridge to rethrow. A failure already pending before
    /// the call stays pending too (the first one wins).
    fn hidden_checked<T>(
        &self,
        run: impl FnOnce() -> Result<T, PostError>,
    ) -> Result<T, PostError> {
        let before = take_hook_error();
        let result = run();
        let during = take_hook_error();
        if let Some(before) = before {
            restash(before);
        }
        match during {
            Some(hook) => {
                restash(hook.clone());
                Err(PostError::Hook(hook))
            }
            None => result,
        }
    }

    fn written(&self, result: Result<u64, WpDbError>) -> Result<u64, PostError> {
        match result {
            Ok(n) => Ok(n),
            Err(e) => surfaced(e).map(|()| 0),
        }
    }

    /// The first `post_name` a `wp_unique_post_slug()` statement selects.
    fn slug_owner(&self, template: &str, args: &[WpValue]) -> Result<Option<Slug>, PostError> {
        self.hidden_checked(|| {
            let rows = self.select(template, args)?;
            Ok(Self::var(&rows).map(Slug::new))
        })
    }
}

impl PostRepository for SqlPostRepository {
    fn engine(&self) -> String {
        self.engine.clone()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        let template = format!("SELECT * FROM {} WHERE ID = %d LIMIT 1", self.table());
        self.hidden_checked(|| {
            let rows = self.select(&template, &[int(id.0)])?;
            rows.first()
                .map(|row| PostMapper::to_entity(&to_row(row)))
                .transpose()
        })
    }

    /// `$wpdb->insert( $wpdb->posts, $data )` with no formats, as `wp_insert_post()` sends it, then
    /// `$wpdb->insert_id`. A `query` callback's failure that `$wpdb->insert`'s charset lookup hides is
    /// [`PostError::Hook`]; the payload stays in `take_hook_error()`.
    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError> {
        let data: Vec<(String, WpValue)> = PostMapper::insert_values(post)
            .into_iter()
            .map(|(column, value)| (column, wp_value(value)))
            .collect();
        self.hidden_checked(|| {
            let inserted = self.written(self.wpdb.insert(&self.table(), &data, None))?;
            if inserted == 0 {
                return Ok(None);
            }
            Ok(Some(PostId(self.wpdb.insert_id())))
        })
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        let template = format!(
            "SELECT post_name FROM {} WHERE post_name = %s AND ID != %d LIMIT 1",
            self.table()
        );
        self.slug_owner(&template, &[php_string(slug.as_bytes()), int(except.0)])
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        let template = format!(
            "SELECT post_name FROM {} WHERE post_name = %s AND post_type IN ( %s, 'attachment' ) AND ID != %d \
             AND post_parent = %d LIMIT 1",
            self.table()
        );
        self.slug_owner(
            &template,
            &[
                php_string(slug.as_bytes()),
                WpValue::Str(post_type.as_str().to_string()),
                int(except.0),
                int(parent.0),
            ],
        )
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        let template = format!(
            "SELECT post_name FROM {} WHERE post_name = %s AND post_type = %s AND ID != %d LIMIT 1",
            self.table()
        );
        self.slug_owner(
            &template,
            &[
                php_string(slug.as_bytes()),
                WpValue::Str(post_type.as_str().to_string()),
                int(except.0),
            ],
        )
    }

    fn find_many(&self, ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let table = self.table();
        let sql = format!(
            "SELECT {table}.* FROM {table} WHERE ID IN ({})",
            id_list(ids)
        );
        self.hidden_checked(|| {
            self.select_text(sql.as_bytes())?
                .iter()
                .map(|row| PostMapper::to_entity(&to_row(row)))
                .collect()
        })
    }

    fn parent_ids(&self, ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let table = self.table();
        let sql = format!(
            "SELECT {table}.ID, {table}.post_parent FROM {table} WHERE ID IN ({})",
            id_list(ids)
        );
        self.hidden_checked(|| {
            Ok(self
                .select_text(sql.as_bytes())?
                .iter()
                .map(|row| {
                    (
                        PostId(number(column(row, 0))),
                        PostId(number(column(row, 1))),
                    )
                })
                .collect())
        })
    }

    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        if names.is_empty() || post_types.is_empty() {
            return Ok(Vec::new());
        }
        let names = self.quoted_list(names.iter().map(Slug::as_bytes), b"','");
        let types = self.quoted_list(post_types.iter().map(|t| t.as_str().as_bytes()), b"','");
        let sql = [
            b"\n\t\tSELECT ID, post_name, post_parent, post_type\n\t\tFROM ".as_slice(),
            self.table().as_bytes(),
            b"\n\t\tWHERE post_name IN (",
            &names,
            b")\n\t\tAND post_type IN (",
            &types,
            b")\n\t\tORDER BY\n\t\t\tpost_status = 'publish' DESC,\n\t\t\tpost_status IN ('draft', 'pending', \
              'auto-draft') ASC, ID ASC\n\t",
        ]
        .concat();
        self.hidden_checked(|| {
            Ok(self
                .select_text(&sql)?
                .iter()
                .map(|row| PageRow {
                    id: PostId(number(column(row, 0))),
                    name: Slug::new(text(row, 1)),
                    parent: PostId(number(column(row, 2))),
                    post_type: PostTypeName::new(String::from_utf8_lossy(&text(row, 3))),
                })
                .collect())
        })
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        let sql = format!("SELECT ID FROM {} WHERE post_type = 'page'", self.table());
        self.hidden_checked(|| {
            Ok(self
                .select_text(sql.as_bytes())?
                .iter()
                .map(|row| PostId(number(column(row, 0))))
                .collect())
        })
    }

    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        let table = self.table();
        let kind = WpValue::Str(post_type.as_str().to_string());
        let (template, args) = match reader {
            None => (
                format!(
                    "\n\t\t\tSELECT post_status, COUNT(*) AS num_posts\n\t\t\tFROM {table}\n\t\t\t\
                     WHERE post_type = %s\n\t\t GROUP BY post_status"
                ),
                vec![kind],
            ),
            Some(user) => (
                format!(
                    "\n\t\t\tSELECT post_status, COUNT(*) AS num_posts\n\t\t\tFROM (\n\t\t\t\tSELECT post_status\
                     \n\t\t\t\tFROM {table}\n\t\t\t\tWHERE post_type = %s AND post_status != 'private'\n\t\t\t\t\
                     UNION ALL\n\t\t\t\tSELECT post_status\n\t\t\t\tFROM {table}\n\t\t\t\tWHERE post_type = %s AND \
                     post_status = 'private' AND post_author = %d\n\t\t\t) AS filtered_posts\n\t\t GROUP BY \
                     post_status"
                ),
                vec![kind.clone(), kind, int(user)],
            ),
        };
        self.hidden_checked(|| {
            Ok(self
                .select(&template, &args)?
                .iter()
                .map(|row| {
                    (
                        PostStatusName::new(String::from_utf8_lossy(&text(row, 0))),
                        number(column(row, 1)),
                    )
                })
                .collect())
        })
    }

    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        let table = self.table();
        let and = mime_type_where(mime_type);
        let by_type = [
            b"SELECT post_mime_type, COUNT( * ) AS num_posts FROM ".as_slice(),
            table.as_bytes(),
            b" WHERE post_type = 'attachment' AND post_status != 'trash' ",
            &and,
            b" GROUP BY post_mime_type",
        ]
        .concat();
        let trashed = [
            b"SELECT COUNT( * ) FROM ".as_slice(),
            table.as_bytes(),
            b" WHERE post_type = 'attachment' AND post_status = 'trash' ",
            &and,
        ]
        .concat();
        self.hidden_checked(|| {
            let counts = self
                .select_text(&by_type)?
                .iter()
                .map(|row| (MimeType::new(text(row, 0)), number(column(row, 1))))
                .collect();
            let rows = self.select_text(&trashed)?;
            let trash = rows.first().map_or(0, |row| number(column(row, 0)));
            Ok((counts, trash))
        })
    }

    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        let field = field.as_str();
        let types = self.quoted_list(post_types.iter().map(|t| t.as_str().as_bytes()), b"', '");
        let selected = match timezone {
            Timezone::Gmt => format!("post_{field}_gmt"),
            Timezone::Blog => format!("post_{field}"),
            // `gmdate( 'Z' )` is always `0`.
            Timezone::Server => format!("DATE_ADD(post_{field}_gmt, INTERVAL '0' SECOND)"),
        };
        let sql = [
            format!(
                "SELECT {selected} FROM {} WHERE post_status = 'publish' AND post_type IN (",
                self.table()
            )
            .as_bytes(),
            &types,
            format!(") ORDER BY post_{field}_gmt DESC LIMIT 1").as_bytes(),
        ]
        .concat();
        self.hidden_checked(|| {
            let rows = self.select_text(&sql)?;
            Ok(Self::var(&rows).map(|d| {
                let mut at = String::from_utf8_lossy(&d).into_owned();
                // MySQL's DATE_ADD answers six fractional digits; the other engines' translations of it do not, and the
                // contracts' answer is MySQL's text.
                if matches!(timezone, Timezone::Server) && !at.contains('.') {
                    at.push_str(".000000");
                }
                PostDateTime::new(at)
            }))
        })
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        let template = format!(
            "SELECT DISTINCT post_mime_type FROM {} WHERE post_type = %s AND post_mime_type != ''",
            self.table()
        );
        self.hidden_checked(|| {
            Ok(self
                .select(&template, &[WpValue::Str(post_type.as_str().to_string())])?
                .iter()
                .map(|row| MimeType::new(text(row, 0)))
                .collect())
        })
    }
}
