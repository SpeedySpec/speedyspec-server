//! The PHP twin of the post repository (Repository), which PHP implements to replace `wp_posts`, and the adapter
//! that makes a PHP implementation the Rust port.
//!
//! `#[php_port]` declares the interface at the start of every request. A PHP object implementing
//! `SpeedySpec\WpPosts\PostRepository` and bound at that key in the container reaches the use cases as
//! [`PhpPostRepository`]. Values cross as PHP holds them:
//! - a post is a row array keyed by column (`ID`, `post_author`, … `comment_count`), as `$wpdb->get_row( …, ARRAY_A )`
//!   gives it, read with PHP's casts (Data Mapper: `PostEntry`);
//! - ids are ints, names and types strings, and "nothing" is `null`;
//! - `count_attachments()` answers `array( 'counts' => array( mime => n ), 'trash' => n )`;
//! - `insert()` and `update()` take `wp_insert_post()`'s `$data` (the 21 columns it writes, `post_parent` an int or
//!   `null`), `insert()` with the `import_id` (an int, or `null` for none) and `update()` with the post's id;
//! - an update or delete answers the rows changed, and `null` or `false` where `$wpdb` returns false.

use std::rc::Rc;

use speedyspec_php_ext::{Key, PhpObject, PortError, Value, php_port};
use speedyspec_php_serialize::semantics::{get, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::PostEntry;
use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime, PostError, PostId,
    PostParent, PostRepository, PostRow, PostStatusName, PostTypeName, RowValue, Slug, Timezone,
    WriteOutcome,
};

use crate::codec::{Thrown, copy_php, string};
use crate::ports::thrown_error;

/// `wp_posts`' twin: one method per `$wpdb` call `post.php` makes, as the `PostRepository` port names them.
#[php_port(name = "SpeedySpec\\WpPosts\\PostRepository")]
pub trait PostRepositoryTwin {
    fn engine(&self) -> Result<Value, PortError>;
    fn find(&self, id: i64) -> Result<Value, PortError>;
    fn insert(&self, row: Value) -> Result<Value, PortError>;
    fn attachment_slug_owner(&self, slug: Value, except: i64) -> Result<Value, PortError>;
    fn hierarchical_slug_owner(
        &self,
        slug: Value,
        post_type: Value,
        except: i64,
        parent: i64,
    ) -> Result<Value, PortError>;
    fn flat_slug_owner(
        &self,
        slug: Value,
        post_type: Value,
        except: i64,
    ) -> Result<Value, PortError>;
    fn find_many(&self, ids: Value) -> Result<Value, PortError>;
    fn parent_ids(&self, ids: Value) -> Result<Value, PortError>;
    fn pages_by_names(&self, names: Value, post_types: Value) -> Result<Value, PortError>;
    fn all_page_ids(&self) -> Result<Value, PortError>;
    fn count_by_status(&self, post_type: Value, reader: Value) -> Result<Value, PortError>;
    fn count_attachments(&self, mime_type: Value) -> Result<Value, PortError>;
    fn last_post_time(
        &self,
        field: Value,
        post_types: Value,
        timezone: Value,
    ) -> Result<Value, PortError>;
    fn available_mime_types(&self, post_type: Value) -> Result<Value, PortError>;
    fn update(&self, id: i64, row: Value) -> Result<Value, PortError>;
    fn update_name(&self, id: i64, name: Value) -> Result<Value, PortError>;
    fn update_guid(&self, id: i64, guid: Value) -> Result<Value, PortError>;
    fn update_status(&self, id: i64, status: Value) -> Result<Value, PortError>;
    fn update_pinged(&self, id: i64, pinged: Value) -> Result<Value, PortError>;
    fn update_post_type(&self, id: i64, post_type: Value) -> Result<Value, PortError>;
    fn import_id_taken(&self, id: i64) -> Result<Value, PortError>;
    fn raw_row(&self, id: i64) -> Result<Value, PortError>;
    fn children_of_type(&self, parent: i64, post_type: Value) -> Result<Value, PortError>;
    fn reparent_children(&self, from: i64, to: Value, post_type: Value)
    -> Result<Value, PortError>;
    fn revision_ids(&self, parent: i64) -> Result<Value, PortError>;
    fn reparent_attachments(&self, from: i64, to: Value) -> Result<Value, PortError>;
    fn remove(&self, id: i64) -> Result<Value, PortError>;
    fn auto_drafts_older_than_a_week(&self) -> Result<Value, PortError>;
}

/// A PHP implementation of `SpeedySpec\WpPosts\PostRepository`. An exception it throws stops the use case and
/// reaches the caller as the same object ([`PostError::Hook`] carries it).
pub struct PhpPostRepository(PhpObject);

impl PhpPostRepository {
    pub fn new(object: PhpObject) -> PhpPostRepository {
        PhpPostRepository(object)
    }

    fn call(&self, method: &str, args: Vec<Value>) -> Result<PhpValue, PostError> {
        let callable = Value::Array(vec![
            (Key::Int(0), self.0.value().clone()),
            (Key::Int(1), Value::Str(method.to_owned())),
        ]);
        let answer = speedyspec_php::call_catching(&callable, &args)
            .map_err(|failure| thrown_error(Thrown(failure.exception)))?;
        copy_php(&answer).map_err(thrown_error)
    }
}

fn id(id: PostId) -> Value {
    Value::Int(i64::try_from(id.0).unwrap_or(i64::MAX))
}

fn ids(list: &[PostId]) -> Value {
    Value::Array(
        list.iter()
            .enumerate()
            .map(|(i, p)| (Key::Int(i as i64), id(*p)))
            .collect(),
    )
}

fn texts<'a>(list: impl Iterator<Item = &'a [u8]>) -> Value {
    Value::Array(
        list.enumerate()
            .map(|(i, t)| (Key::Int(i as i64), string(t)))
            .collect(),
    )
}

fn items(value: PhpValue) -> Vec<(PhpKey, PhpValue)> {
    match value {
        PhpValue::Array(items) => items,
        _ => Vec::new(),
    }
}

fn number(value: &PhpValue) -> u64 {
    u64::try_from(to_php_int(value)).unwrap_or(0)
}

/// A row array as the entity; `None` for anything that is not an array.
fn row(value: PhpValue) -> Option<Post> {
    match value {
        PhpValue::Array(columns) => Some(PostEntry::from_properties(&columns)),
        _ => None,
    }
}

/// A string answer, `None` for `null` or `false`.
fn text(value: PhpValue) -> Option<Vec<u8>> {
    match value {
        PhpValue::Null | PhpValue::Bool(false) => None,
        other => Some(to_php_string(&other)),
    }
}

fn lossy(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A parent as `%d` sees it in a statement: `null` is 0.
fn parent(parent: PostParent) -> Value {
    Value::Int(parent.prepared())
}

/// A parent as `$data['post_parent']` holds it: the int, or `null`.
fn parent_value(parent: PostParent) -> Value {
    match parent {
        PostParent::Int(n) => Value::Int(n),
        PostParent::Null => Value::Null,
    }
}

/// `wp_insert_post()`'s filtered `$data` ([`PostRow`]) as the PHP array a twin hands to `$wpdb->insert` or
/// `$wpdb->update`: its keys in order, `null`, ints and strings as the row holds them, so the twin sends WordPress's
/// statement text.
fn data(row: &PostRow) -> Result<Value, PostError> {
    let mut columns = Vec::with_capacity(row.columns().len());
    for (column, value) in row.columns() {
        // Not an exhaustive match: the contract's value type may grow.
        let value = if let RowValue::Int(n) = value {
            Value::Int(*n)
        } else if let RowValue::Str(bytes) = value {
            string(bytes)
        } else if let RowValue::Php(php) = value {
            crate::codec::hook_in(php.clone()).map_err(thrown_error)?
        } else {
            Value::Null
        };
        // A key that is not UTF-8 is read lossily: the PHP array keys a twin receives are text here.
        columns.push((
            Key::Str(String::from_utf8_lossy(column).into_owned()),
            value,
        ));
    }
    Ok(Value::Array(columns))
}

/// The rows an update or delete changed; `None` for `null` or `false` (`$wpdb` returned false).
fn changed(value: PhpValue) -> Option<u64> {
    match value {
        PhpValue::Null | PhpValue::Bool(false) => None,
        other => Some(number(&other)),
    }
}

/// A list of ids.
fn id_list(value: PhpValue) -> Vec<PostId> {
    items(value)
        .into_iter()
        .map(|(_, v)| PostId(number(&v)))
        .collect()
}

impl PostRepository for PhpPostRepository {
    fn engine(&self) -> String {
        self.call("engine", Vec::new())
            .ok()
            .and_then(text)
            .map_or_else(|| "php".to_owned(), lossy)
    }

    fn find(&self, post: PostId) -> Result<Option<Post>, PostError> {
        Ok(row(self.call("find", vec![id(post)])?))
    }

    /// The twin answers the new id, or `false`/`0` for a failed insert; a string answer is the failure's
    /// `$wpdb->last_error`.
    fn insert(&self, row: &PostRow) -> Result<WriteOutcome<PostId>, PostError> {
        let answer = self.call("insert", vec![data(row)?])?;
        if let PhpValue::Str(error) = &answer {
            return Ok(WriteOutcome::failed(String::from_utf8_lossy(error)));
        }
        let new = number(&answer);
        Ok(if new > 0 {
            WriteOutcome::Written(PostId(new))
        } else {
            WriteOutcome::failed("")
        })
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        let answer = self.call(
            "attachment_slug_owner",
            vec![string(slug.as_bytes()), id(except)],
        )?;
        Ok(text(answer).map(Slug::new))
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostParent,
    ) -> Result<Option<Slug>, PostError> {
        let answer = self.call(
            "hierarchical_slug_owner",
            vec![
                string(slug.as_bytes()),
                Value::Str(post_type.as_str().to_owned()),
                id(except),
                self::parent(parent),
            ],
        )?;
        Ok(text(answer).map(Slug::new))
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        let answer = self.call(
            "flat_slug_owner",
            vec![
                string(slug.as_bytes()),
                Value::Str(post_type.as_str().to_owned()),
                id(except),
            ],
        )?;
        Ok(text(answer).map(Slug::new))
    }

    fn find_many(&self, list: &[PostId]) -> Result<Vec<Post>, PostError> {
        if list.is_empty() {
            return Ok(Vec::new());
        }
        Ok(items(self.call("find_many", vec![ids(list)])?)
            .into_iter()
            .filter_map(|(_, v)| row(v))
            .collect())
    }

    fn parent_ids(&self, list: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        if list.is_empty() {
            return Ok(Vec::new());
        }
        Ok(items(self.call("parent_ids", vec![ids(list)])?)
            .into_iter()
            .map(|(k, v)| {
                let child = match k {
                    PhpKey::Int(i) => u64::try_from(i).unwrap_or(0),
                    PhpKey::Str(s) => number(&PhpValue::Str(s)),
                };
                (PostId(child), PostId(number(&v)))
            })
            .collect())
    }

    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        if names.is_empty() || post_types.is_empty() {
            return Ok(Vec::new());
        }
        let answer = self.call(
            "pages_by_names",
            vec![
                texts(names.iter().map(Slug::as_bytes)),
                texts(post_types.iter().map(|t| t.as_str().as_bytes())),
            ],
        )?;
        Ok(items(answer)
            .into_iter()
            .filter_map(|(_, v)| match v {
                PhpValue::Array(columns) => {
                    let column =
                        |name: &str| get(&columns, name).cloned().unwrap_or(PhpValue::Null);
                    Some(PageRow {
                        id: PostId(number(&column("ID"))),
                        name: Slug::new(to_php_string(&column("post_name"))),
                        parent: PostId(number(&column("post_parent"))),
                        post_type: PostTypeName::new(lossy(to_php_string(&column("post_type")))),
                    })
                }
                _ => None,
            })
            .collect())
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        Ok(items(self.call("all_page_ids", Vec::new())?)
            .into_iter()
            .map(|(_, v)| PostId(number(&v)))
            .collect())
    }

    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        let reader = reader.map_or(Value::Null, |r| id(PostId(r)));
        let answer = self.call(
            "count_by_status",
            vec![Value::Str(post_type.as_str().to_owned()), reader],
        )?;
        Ok(items(answer)
            .into_iter()
            .map(|(k, v)| {
                let status = match k {
                    PhpKey::Int(i) => i.to_string(),
                    PhpKey::Str(s) => lossy(s),
                };
                (PostStatusName::new(status), number(&v))
            })
            .collect())
    }

    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        let mime = match mime_type {
            None => Value::Null,
            Some(MimeFilter::One(text)) => Value::Str(text.clone()),
            Some(MimeFilter::Many(list)) => texts(list.iter().map(String::as_bytes)),
        };
        let answer = items(self.call("count_attachments", vec![mime])?);
        let counts = get(&answer, "counts")
            .cloned()
            .map(items)
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| {
                let mime = match k {
                    PhpKey::Int(i) => i.to_string().into_bytes(),
                    PhpKey::Str(s) => s,
                };
                (MimeType::new(mime), number(&v))
            })
            .collect();
        let trash = get(&answer, "trash").map_or(0, number);
        Ok((counts, trash))
    }

    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        let answer = self.call(
            "last_post_time",
            vec![
                Value::Str(field.as_str().to_owned()),
                texts(post_types.iter().map(|t| t.as_str().as_bytes())),
                Value::Str(timezone.as_str().to_owned()),
            ],
        )?;
        Ok(text(answer).map(|t| PostDateTime::new(lossy(t))))
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        let answer = self.call(
            "available_mime_types",
            vec![Value::Str(post_type.as_str().to_owned())],
        )?;
        Ok(items(answer)
            .into_iter()
            .map(|(_, v)| MimeType::new(to_php_string(&v)))
            .collect())
    }

    /// The twin answers the rows changed, or `false`/`null` for a failed update; a string answer is the failure's
    /// `$wpdb->last_error`.
    fn update(&self, post: PostId, row: &PostRow) -> Result<WriteOutcome<u64>, PostError> {
        let answer = self.call("update", vec![id(post), data(row)?])?;
        if let PhpValue::Str(error) = &answer {
            return Ok(WriteOutcome::failed(String::from_utf8_lossy(error)));
        }
        Ok(match changed(answer) {
            Some(rows) => WriteOutcome::Written(rows),
            None => WriteOutcome::failed(""),
        })
    }

    fn update_name(&self, post: PostId, name: &Slug) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call(
            "update_name",
            vec![id(post), string(name.as_bytes())],
        )?))
    }

    fn update_guid(&self, post: PostId, guid: &[u8]) -> Result<Option<u64>, PostError> {
        Ok(changed(
            self.call("update_guid", vec![id(post), string(guid)])?,
        ))
    }

    fn update_status(
        &self,
        post: PostId,
        status: &PostStatusName,
    ) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call(
            "update_status",
            vec![id(post), Value::Str(status.as_str().to_owned())],
        )?))
    }

    fn update_pinged(&self, post: PostId, pinged: &[u8]) -> Result<Option<u64>, PostError> {
        Ok(changed(
            self.call("update_pinged", vec![id(post), string(pinged)])?,
        ))
    }

    fn update_post_type(
        &self,
        post: i64,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call(
            "update_post_type",
            vec![Value::Int(post), Value::Str(post_type.as_str().to_owned())],
        )?))
    }

    fn import_id_taken(&self, post: i64) -> Result<bool, PostError> {
        let answer = self.call("import_id_taken", vec![Value::Int(post)])?;
        Ok(!speedyspec_php_serialize::semantics::is_empty(&answer))
    }

    fn raw_row(&self, post: PostId) -> Result<Option<Post>, PostError> {
        Ok(row(self.call("raw_row", vec![id(post)])?))
    }

    fn children_of_type(
        &self,
        parent: PostId,
        post_type: &PostTypeName,
    ) -> Result<Vec<Post>, PostError> {
        Ok(items(self.call(
            "children_of_type",
            vec![id(parent), Value::Str(post_type.as_str().to_owned())],
        )?)
        .into_iter()
        .filter_map(|(_, v)| row(v))
        .collect())
    }

    fn reparent_children(
        &self,
        from: PostId,
        to: PostParent,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call(
            "reparent_children",
            vec![
                id(from),
                parent_value(to),
                Value::Str(post_type.as_str().to_owned()),
            ],
        )?))
    }

    fn revision_ids(&self, parent: PostId) -> Result<Vec<PostId>, PostError> {
        Ok(id_list(self.call("revision_ids", vec![id(parent)])?))
    }

    fn reparent_attachments(&self, from: PostId, to: PostParent) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call(
            "reparent_attachments",
            vec![id(from), parent_value(to)],
        )?))
    }

    fn remove(&self, post: PostId) -> Result<Option<u64>, PostError> {
        Ok(changed(self.call("remove", vec![id(post)])?))
    }

    fn auto_drafts_older_than_a_week(&self) -> Result<Vec<PostId>, PostError> {
        Ok(id_list(
            self.call("auto_drafts_older_than_a_week", Vec::new())?,
        ))
    }
}

/// The twin's [`Rc`] for the container port.
pub(crate) fn wrap(object: PhpObject) -> Rc<dyn PostRepository> {
    Rc::new(PhpPostRepository::new(object))
}
