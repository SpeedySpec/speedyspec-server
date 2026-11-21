//! `.l10n.php` files in the grammar GlotPress, `wp-cli i18n make-php` and `WP_Translation_File_PHP::export()`
//! write, parsed without running PHP. Anything outside that grammar is [`ReadError::Unsupported`], so the PHP side
//! can `include` it as WordPress does.

use std::collections::HashMap;
use std::path::Path;

use speedyspec_wp_i18n_contracts::{RawEntry, RawTranslations, ReadError, TranslationFileReader};

use crate::headers::{assign, text};

/// Reads `.l10n.php` files written by the translation tools.
#[derive(Clone, Copy, Debug, Default)]
pub struct L10nPhpReader;

/// A parsed PHP value. Strings are byte ranges into the decoded buffer.
enum Value {
    Str(u32, u32),
    Int(i64),
    /// `true`, `false` or `null`: valid PHP, never a string WordPress can use here.
    Scalar,
    Array(Vec<(Key, Value)>),
}

#[derive(Clone, PartialEq)]
enum Key {
    Int(i64),
    Str(u32, u32),
}

/// A key with its bytes, for the duplicate index.
#[derive(PartialEq, Eq, Hash)]
enum OwnedKey {
    Int(i64),
    Str(Vec<u8>),
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    out: Vec<u8>,
}

type P<T> = Result<T, ReadError>;

const UNSUPPORTED: ReadError = ReadError::Unsupported;

impl Parser<'_> {
    fn ws(&mut self) {
        while self.pos < self.s.len() && matches!(self.s[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        self.ws();
        if self.s[self.pos..].starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.pos).copied()
    }

    fn value(&mut self) -> P<Value> {
        match self.peek().ok_or(UNSUPPORTED)? {
            b'[' => {
                self.pos += 1;
                self.array(b']')
            }
            b'\'' | b'"' => {
                let start = self.out.len() as u32;
                loop {
                    self.string()?;
                    if !self.eat(b".") {
                        break;
                    }
                    if !matches!(self.peek(), Some(b'\'' | b'"')) {
                        return Err(UNSUPPORTED);
                    }
                }
                Ok(Value::Str(start, self.out.len() as u32))
            }
            b'0'..=b'9' => Ok(Value::Int(self.int()?)),
            _ => {
                if self.eat_word(b"array") {
                    if !self.eat(b"(") {
                        return Err(UNSUPPORTED);
                    }
                    self.array(b')')
                } else if self.eat_word(b"true")
                    || self.eat_word(b"false")
                    || self.eat_word(b"null")
                {
                    Ok(Value::Scalar)
                } else {
                    Err(UNSUPPORTED)
                }
            }
        }
    }

    /// A keyword, case-insensitive as PHP's are, not followed by an identifier character.
    fn eat_word(&mut self, word: &[u8]) -> bool {
        self.ws();
        let end = self.pos + word.len();
        let ok = self
            .s
            .get(self.pos..end)
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
            && !self
                .s
                .get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        if ok {
            self.pos = end;
        }
        ok
    }

    fn int(&mut self) -> P<i64> {
        let digits = self.s[self.pos..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let text =
            std::str::from_utf8(&self.s[self.pos..self.pos + digits]).map_err(|_| UNSUPPORTED)?;
        // Leading zeros mean octal in PHP; the generators never write them.
        if digits > 1 && text.starts_with('0') {
            return Err(UNSUPPORTED);
        }
        self.pos += digits;
        text.parse().map_err(|_| UNSUPPORTED)
    }

    /// One quoted string, decoded onto the end of `out`.
    fn string(&mut self) -> P<()> {
        let quote = self.s[self.pos];
        self.pos += 1;
        loop {
            let c = *self.s.get(self.pos).ok_or(UNSUPPORTED)?;
            self.pos += 1;
            if c == quote {
                return Ok(());
            }
            if quote == b'"' && c == b'$' {
                // Interpolation.
                return Err(UNSUPPORTED);
            }
            if c != b'\\' {
                self.out.push(c);
                continue;
            }
            let e = *self.s.get(self.pos).ok_or(UNSUPPORTED)?;
            if quote == b'\'' {
                if e == b'\'' || e == b'\\' {
                    self.out.push(e);
                    self.pos += 1;
                } else {
                    self.out.push(b'\\');
                }
                continue;
            }
            self.pos += 1;
            match e {
                b'n' => self.out.push(b'\n'),
                b'r' => self.out.push(b'\r'),
                b't' => self.out.push(b'\t'),
                b'v' => self.out.push(0x0B),
                b'e' => self.out.push(0x1B),
                b'f' => self.out.push(0x0C),
                b'\\' | b'$' | b'"' => self.out.push(e),
                b'0'..=b'7' => {
                    let mut n = u32::from(e - b'0');
                    for _ in 0..2 {
                        match self.s.get(self.pos) {
                            Some(d @ b'0'..=b'7') => {
                                n = n * 8 + u32::from(d - b'0');
                                self.pos += 1;
                            }
                            _ => break,
                        }
                    }
                    self.out.push(n as u8);
                }
                b'x' if self.s.get(self.pos).is_some_and(u8::is_ascii_hexdigit) => {
                    let mut n = 0u32;
                    for _ in 0..2 {
                        match self.s.get(self.pos) {
                            Some(d) if d.is_ascii_hexdigit() => {
                                n = n * 16 + (*d as char).to_digit(16).unwrap();
                                self.pos += 1;
                            }
                            _ => break,
                        }
                    }
                    self.out.push(n as u8);
                }
                b'u' => return Err(UNSUPPORTED),
                other => {
                    self.out.push(b'\\');
                    self.out.push(other);
                }
            }
        }
    }

    fn array(&mut self, close: u8) -> P<Value> {
        let mut items: Vec<(Key, Value)> = Vec::new();
        // Where each key already is, so a repeated key replaces its value in place, as PHP's array assignment does.
        let mut positions: HashMap<OwnedKey, usize, foldhash::fast::RandomState> =
            HashMap::default();
        loop {
            if self.peek() == Some(close) {
                self.pos += 1;
                return Ok(Value::Array(items));
            }
            let key = match self.value()? {
                Value::Str(a, b) => Key::Str(a, b),
                Value::Int(n) => Key::Int(n),
                _ => return Err(UNSUPPORTED),
            };
            if !self.eat(b"=>") {
                // A list entry without a key: never written by the generators.
                return Err(UNSUPPORTED);
            }
            let value = self.value()?;
            let key = self.normalize(key);
            let owned = match &key {
                Key::Int(n) => OwnedKey::Int(*n),
                Key::Str(a, b) => OwnedKey::Str(self.bytes(*a, *b).to_vec()),
            };
            match positions.get(&owned) {
                Some(&i) => items[i].1 = value,
                None => {
                    positions.insert(owned, items.len());
                    items.push((key, value));
                }
            }
            if self.eat(b",") {
                continue;
            }
            if self.peek() != Some(close) {
                return Err(UNSUPPORTED);
            }
        }
    }

    fn bytes(&self, a: u32, b: u32) -> &[u8] {
        &self.out[a as usize..b as usize]
    }

    /// PHP turns a string key that is a canonical decimal integer into an integer key.
    fn normalize(&self, key: Key) -> Key {
        if let Key::Str(a, b) = key {
            let s = self.bytes(a, b);
            if let Ok(t) = std::str::from_utf8(s)
                && let Ok(n) = t.parse::<i64>()
                && n.to_string() == t
            {
                return Key::Int(n);
            }
        }
        key
    }
}

impl L10nPhpReader {
    /// Parses the file's bytes; see the module documentation for the grammar.
    pub fn parse(source: &[u8]) -> Result<RawTranslations, ReadError> {
        let mut p = Parser {
            s: source,
            pos: 0,
            out: Vec::with_capacity(source.len()),
        };
        if !source.starts_with(b"<?php") {
            return Err(UNSUPPORTED);
        }
        p.pos = 5;
        if !p.s.get(p.pos).is_some_and(|c| c.is_ascii_whitespace()) || !p.eat_word(b"return") {
            return Err(UNSUPPORTED);
        }
        let value = p.value()?;
        if !p.eat(b";") {
            return Err(UNSUPPORTED);
        }
        p.ws();
        if p.pos != source.len() {
            return Err(UNSUPPORTED);
        }
        // `! $result || ! is_array( $result )`: a non-array, or an empty array, is invalid.
        let Value::Array(items) = value else {
            return Err(ReadError::Invalid("Invalid data"));
        };
        if items.is_empty() {
            return Err(ReadError::Invalid("Invalid data"));
        }
        let mut raw = RawTranslations::default();
        let key_text = |p: &Parser<'_>, k: &Key| match k {
            Key::Int(n) => n.to_string().into_bytes(),
            Key::Str(a, b) => p.bytes(*a, *b).to_vec(),
        };
        let mut messages: Option<&Vec<(Key, Value)>> = None;
        for (k, v) in &items {
            let name = key_text(&p, k);
            if name == b"messages" {
                match v {
                    Value::Array(m) => {
                        messages = Some(m);
                        continue;
                    }
                    // A non-array `messages` stays among the headers in WordPress; leave such a file to PHP.
                    _ => return Err(UNSUPPORTED),
                }
            }
            let Value::Str(a, b) = v else {
                return Err(UNSUPPORTED);
            };
            assign(
                &mut raw.headers,
                text(&name.to_ascii_lowercase()),
                text(p.bytes(*a, *b)),
            );
        }
        let mut bytes = Vec::with_capacity(p.out.len());
        bytes.extend_from_slice(&p.out);
        for (k, v) in messages.into_iter().flatten() {
            let Value::Str(a, b) = v else {
                return Err(UNSUPPORTED);
            };
            let key = match k {
                Key::Str(ka, kb) => *ka..*kb,
                Key::Int(n) => {
                    let start = bytes.len() as u32;
                    bytes.extend_from_slice(n.to_string().as_bytes());
                    start..bytes.len() as u32
                }
            };
            raw.entries.push(RawEntry { key, value: *a..*b });
        }
        raw.bytes = bytes;
        Ok(raw)
    }
}

impl TranslationFileReader for L10nPhpReader {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError> {
        let source = std::fs::read(path).map_err(|_| ReadError::Unreadable)?;
        L10nPhpReader::parse(&source)
    }
}
