//! `add_ping( $post, $uri )` (post.php 6257-6287, WordPress 7.2-RC2): the post's `pinged` list gains the URIs,
//! through the `add_ping` filter (Observer) and one ``UPDATE … SET `pinged` `` (Repository).

use speedyspec_php_serialize::semantics::{array_merge, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{PostArg, PostError};

use crate::write::api::{PostWritesApi, written};

/// PHP's `trim()` with its default characters.
fn trim(bytes: &[u8]) -> &[u8] {
    let strip = |b: &u8| b" \t\n\r\0\x0B".contains(b);
    let start = bytes.iter().position(|b| !strip(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !strip(b))
        .map_or(start, |e| e + 1);
    &bytes[start..end]
}

/// `preg_split( '/\s/', $text )`: split at every single whitespace byte (PCRE2's `\s`: space, tab, line feed,
/// vertical tab, form feed, carriage return), empty pieces kept.
fn split_whitespace(text: &[u8]) -> Vec<(PhpKey, PhpValue)> {
    text.split(|b| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .enumerate()
        .map(|(i, piece)| (PhpKey::Int(i as i64), PhpValue::Str(piece.to_vec())))
        .collect()
}

/// `implode( "\n", $pieces )`, each piece as `(string)` gives it.
fn implode(pieces: &[(PhpKey, PhpValue)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, (_, piece)) in pieces.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend(match piece {
            PhpValue::Array(_) => b"Array".to_vec(),
            other => to_php_string(other),
        });
    }
    out
}

impl PostWritesApi {
    /// `add_ping( $post, $uri )`: `None` (false) for no post. Otherwise `trim( $post->pinged )` split at each
    /// whitespace byte, with `uri` appended (an array merged in), joined with line feeds, through the `add_ping`
    /// filter, written to `pinged`, and `clean_post_cache()`. The answer is the rows the update changed, `None` where
    /// `$wpdb->update` returns false.
    pub fn add_ping(&self, post: PostArg, uri: &PhpValue) -> Result<Option<u64>, PostError> {
        let Some(object) = self.post_arg(post)? else {
            return Ok(None);
        };
        let mut pung = split_whitespace(trim(object.post.pinged.as_slice()));
        match uri {
            PhpValue::Array(items) => pung = array_merge(pung, items.clone()),
            other => {
                let next = pung
                    .iter()
                    .filter_map(|(k, _)| match k {
                        PhpKey::Int(n) => Some(*n + 1),
                        PhpKey::Str(_) => None,
                    })
                    .max()
                    .unwrap_or(0);
                pung.push((PhpKey::Int(next), other.clone()));
            }
        }
        let new = PhpValue::Str(implode(&pung));
        let new = self.filter("add_ping", new, Vec::new)?;
        let id = object.post.id;
        let rows = written(|| self.repository().update_pinged(id, &to_php_string(&new)))?;
        self.clean_post_cache(id)?;
        Ok(rows)
    }
}
