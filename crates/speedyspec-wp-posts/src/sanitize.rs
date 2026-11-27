//! `sanitize_post()` (post.php, 7.2-RC2 lines 3052-3079) and `sanitize_post_field()` (3106-3418).
//!
//! Patterns: Observer for the field filters (`edit_*`, `pre_*`, `*_save_pre`, `*_edit_pre`, the display filters),
//! all through [`HookApi`]; Adapter for the escaping, behind the outbound [`Formatting`] port, which the PHP binding
//! fills with WordPress's own `format_to_edit()`, `esc_attr()`, `esc_js()` and `user_can_richedit()` until the text
//! seam's Rust escaping is bound in their place.
//!
//! The surface is PHP-shaped: [`sanitize_post_value`], [`sanitize_post_properties`] and [`sanitize_post_field_word`]
//! take any value, any field name and any context word, as PHP does (an unknown word runs the display filters and is
//! passed to them as given). [`sanitize_post_field`] is the same field function for a Rust caller with a [`Filter`].
//!
//! There is no typed `sanitize_post( &mut Post )`: the [`Post`](speedyspec_wp_posts_contracts::Post) entity holds
//! raw values and has no `filter` property, so writing sanitized values back into it would lose filter output its
//! field types cannot hold and would escape twice on a repeated context. Callers sanitize `WP_Post`'s properties.

use speedyspec_php_serialize::semantics::{isset, key_to_string, set, to_array, to_php_int};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{HookApi, HookName, HookValue};
use speedyspec_wp_posts_contracts::{Filter, PostError, PostId};

use crate::hook::{byte_name, filter};

/// The formatting functions `sanitize_post_field()` calls (formatting.php and general-template.php): an outbound
/// port. Each method stands for the WordPress function in its doc, filters included, and a callback's failure comes
/// back as [`PostError::Hook`].
pub trait Formatting {
    /// `format_to_edit( $content, $rich_text )`: the `format_to_edit` filter, then `esc_textarea()` unless
    /// `rich_text`.
    fn format_to_edit(&self, content: PhpValue, rich_text: bool) -> Result<PhpValue, PostError>;

    /// `esc_attr( $text )`, with its `attribute_escape` filter.
    fn esc_attr(&self, text: PhpValue) -> Result<PhpValue, PostError>;

    /// `esc_js( $text )`, with its `js_escape` filter.
    fn esc_js(&self, text: PhpValue) -> Result<PhpValue, PostError>;

    /// `user_can_richedit()`, read for PHP truth as `format_to_edit()` reads it.
    fn user_can_richedit(&self) -> Result<bool, PostError>;
}

/// What the sanitize functions work through.
#[derive(Clone, Copy)]
pub struct SanitizePorts<'a> {
    /// The field filters.
    pub hooks: &'a dyn HookApi,
    /// `format_to_edit()`, `esc_attr()`, `esc_js()` and `user_can_richedit()`.
    pub formatting: &'a dyn Formatting,
}

/// `$int_fields`: cast with `(int)` before the filters and again after them.
const INT_FIELDS: [&[u8]; 3] = [b"ID", b"post_parent", b"menu_order"];

/// `$format_to_edit`: the fields the `edit` context runs through `format_to_edit()` instead of `esc_attr()`.
const FORMAT_TO_EDIT: [&[u8]; 4] = [
    b"post_content",
    b"post_excerpt",
    b"post_title",
    b"post_password",
];

/// `str_replace( 'post_', '', $field )`: every occurrence, left to right.
fn without_post_prefix(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        if field[i..].starts_with(b"post_") {
            i += 5;
        } else {
            out.push(field[i]);
            i += 1;
        }
    }
    out
}

/// A hook name from parts.
fn name(parts: &[&[u8]]) -> HookName {
    byte_name(parts.concat())
}

/// `absint()`: `abs( (int) $value )`. PHP's `abs( PHP_INT_MIN )` overflows to a float.
fn absint(value: &PhpValue) -> PhpValue {
    let int = to_php_int(value);
    match int.checked_abs() {
        Some(abs) => PhpValue::Int(abs),
        None => PhpValue::Float(-(int as f64)),
    }
}

/// `sanitize_post_field( $field, $value, $post_id, $context )` for a Rust caller: [`sanitize_post_field_word`]
/// with the context's word. A post ID above `i64::MAX` is PHP's saturated `PHP_INT_MAX`.
pub fn sanitize_post_field(
    ports: &SanitizePorts<'_>,
    field: &str,
    value: PhpValue,
    post_id: PostId,
    context: Filter,
) -> Result<PhpValue, PostError> {
    let post_id = i64::try_from(post_id.0).unwrap_or(i64::MAX);
    sanitize_post_field_word(ports, field.as_bytes(), value, post_id, context.as_str())
}

/// `sanitize_post_field( $field, $value, $post_id, $context )`, branch by branch:
/// 1. `ID`, `post_parent` and `menu_order` are cast with `(int)`;
/// 2. `ancestors` comes back as `array_map( 'absint', (array) $value )`, before any filter;
/// 3. `raw` returns the value;
/// 4. `edit`: a field containing `post_` runs `edit_{$field}` then `{$field_no_prefix}_edit_pre`, any other field
///    `edit_post_{$field}`, each with `( $value, $post_id )`; then `format_to_edit()` for the content, excerpt, title
///    and password (`user_can_richedit()` decides the content's rich text), else `esc_attr()`;
/// 5. `db`: `pre_{$field}` then `{$field_no_prefix}_save_pre`, or `pre_post_{$field}` then `{$field}_pre`, each
///    with the value alone;
/// 6. any other word: `{$field}` or `post_{$field}` with `( $value, $post_id, $context )`; then `esc_attr()` for
///    `attribute`, `esc_js()` for `js`;
/// 7. the int fields are cast with `(int)` again.
///
/// `$field_no_prefix` drops every `post_` in the name, as `str_replace()` does.
pub fn sanitize_post_field_word(
    ports: &SanitizePorts<'_>,
    field: &[u8],
    value: PhpValue,
    post_id: i64,
    context: &str,
) -> Result<PhpValue, PostError> {
    let hooks = ports.hooks;
    let is_int = INT_FIELDS.contains(&field);
    let mut value = if is_int {
        PhpValue::Int(to_php_int(&value))
    } else {
        value
    };

    if field == b"ancestors" {
        return Ok(PhpValue::Array(
            to_array(value)
                .into_iter()
                .map(|(key, item)| (key, absint(&item)))
                .collect(),
        ));
    }

    if "raw" == context {
        return Ok(value);
    }

    let prefixed = field.windows(5).any(|w| w == b"post_");
    let no_prefix = without_post_prefix(field);
    let id = || HookValue::Int(post_id);

    if "edit" == context {
        if prefixed {
            value = filter(hooks, &name(&[b"edit_", field]), value, || vec![id()])?;
            value = filter(hooks, &name(&[&no_prefix, b"_edit_pre"]), value, || {
                vec![id()]
            })?;
        } else {
            value = filter(hooks, &name(&[b"edit_post_", field]), value, || vec![id()])?;
        }

        if FORMAT_TO_EDIT.contains(&field) {
            let rich_text = field == b"post_content" && ports.formatting.user_can_richedit()?;
            value = ports.formatting.format_to_edit(value, rich_text)?;
        } else {
            value = ports.formatting.esc_attr(value)?;
        }
    } else if "db" == context {
        if prefixed {
            value = filter(hooks, &name(&[b"pre_", field]), value, Vec::new)?;
            value = filter(hooks, &name(&[&no_prefix, b"_save_pre"]), value, Vec::new)?;
        } else {
            value = filter(hooks, &name(&[b"pre_post_", field]), value, Vec::new)?;
            value = filter(hooks, &name(&[field, b"_pre"]), value, Vec::new)?;
        }
    } else {
        let hook = if prefixed {
            name(&[field])
        } else {
            name(&[b"post_", field])
        };
        value = filter(hooks, &hook, value, || {
            vec![id(), HookValue::Str(context.to_owned())]
        })?;

        if "attribute" == context {
            value = ports.formatting.esc_attr(value)?;
        } else if "js" == context {
            value = ports.formatting.esc_js(value)?;
        }
    }

    if is_int {
        value = PhpValue::Int(to_php_int(&value));
    }
    Ok(value)
}

/// The filters [`sanitize_post_field_word`] runs for `field` in `context`, in order, with the [`Formatting`] port's
/// own hooks (`user_can_richedit`, `format_to_edit`, `attribute_escape`, `js_escape`) where it calls them; for
/// [`crate::read_hooks`].
pub(crate) fn field_hooks(field: &[u8], context: &str) -> Vec<Vec<u8>> {
    if field == b"ancestors" || "raw" == context {
        return Vec::new();
    }
    let prefixed = field.windows(5).any(|w| w == b"post_");
    let no_prefix = without_post_prefix(field);
    let mut hooks: Vec<Vec<u8>> = Vec::new();
    if "edit" == context {
        if prefixed {
            hooks.push([b"edit_", field].concat());
            hooks.push([&no_prefix[..], b"_edit_pre"].concat());
        } else {
            hooks.push([b"edit_post_", field].concat());
        }
        if FORMAT_TO_EDIT.contains(&field) {
            if field == b"post_content" {
                hooks.push(b"user_can_richedit".to_vec());
            }
            hooks.push(b"format_to_edit".to_vec());
        } else {
            hooks.push(b"attribute_escape".to_vec());
        }
    } else if "db" == context {
        if prefixed {
            hooks.push([b"pre_", field].concat());
            hooks.push([&no_prefix[..], b"_save_pre"].concat());
        } else {
            hooks.push([b"pre_post_", field].concat());
            hooks.push([field, b"_pre"].concat());
        }
    } else {
        hooks.push(if prefixed {
            field.to_vec()
        } else {
            [b"post_", field].concat()
        });
        if "attribute" == context {
            hooks.push(b"attribute_escape".to_vec());
        } else if "js" == context {
            hooks.push(b"js_escape".to_vec());
        }
    }
    hooks
}

/// `sanitize_post()`'s object and array branches over the properties (or entries), in place.
///
/// When `filter` is set and equals `context`, nothing changes. Otherwise an unset (missing or null) `ID` becomes 0,
/// each property is sanitized in order with `(int) $post->ID` read afresh for each one (so a filtered `ID` is the
/// post ID of the fields after it), and `filter` is set to `context`. Returns whether anything ran.
pub fn sanitize_post_properties(
    ports: &SanitizePorts<'_>,
    properties: &mut Vec<(PhpKey, PhpValue)>,
    context: &str,
) -> Result<bool, PostError> {
    if let Some(PhpValue::Str(filter)) = isset(properties, "filter")
        && filter.as_slice() == context.as_bytes()
    {
        return Ok(false);
    }
    if isset(properties, "ID").is_none() {
        set(properties, "ID", PhpValue::Int(0));
    }
    for i in 0..properties.len() {
        let post_id = isset(properties, "ID").map_or(0, to_php_int);
        let field = match &properties[i].0 {
            PhpKey::Str(bytes) => bytes.clone(),
            key => key_to_string(key).into_bytes(),
        };
        let value = std::mem::replace(&mut properties[i].1, PhpValue::Null);
        properties[i].1 = sanitize_post_field_word(ports, &field, value, post_id, context)?;
    }
    set(
        properties,
        "filter",
        PhpValue::Str(context.as_bytes().to_vec()),
    );
    Ok(true)
}

/// `sanitize_post( $post, $context )` for any PHP value: an object's properties or an array's entries through
/// [`sanitize_post_properties`]; any other value as it is.
///
/// PHP sanitizes an object in place, through its handle; a caller holding a live object writes the returned
/// properties back to it.
pub fn sanitize_post_value(
    ports: &SanitizePorts<'_>,
    post: PhpValue,
    context: &str,
) -> Result<PhpValue, PostError> {
    Ok(match post {
        PhpValue::Object {
            class,
            mut properties,
        } => {
            sanitize_post_properties(ports, &mut properties, context)?;
            PhpValue::Object { class, properties }
        }
        PhpValue::Array(mut items) => {
            sanitize_post_properties(ports, &mut items, context)?;
            PhpValue::Array(items)
        }
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_every_post_prefix() {
        assert_eq!(without_post_prefix(b"post_title"), b"title");
        assert_eq!(without_post_prefix(b"post_post_x_post_"), b"x_");
        assert_eq!(without_post_prefix(b"guid"), b"guid");
    }

    #[test]
    fn absint_overflows_to_float() {
        assert_eq!(absint(&PhpValue::Str(b"-2".to_vec())), PhpValue::Int(2));
        assert_eq!(absint(&PhpValue::Str(b"x".to_vec())), PhpValue::Int(0));
        assert_eq!(
            absint(&PhpValue::Int(i64::MIN)),
            PhpValue::Float(9_223_372_036_854_775_808.0)
        );
    }
}
