//! `unserialize()`: a hand-written reader that checks every length before it allocates.

use std::fmt;

use crate::{PhpKey, PhpValue};

/// PHP's `unserialize()` notice: where reading stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnserializeError {
    pub offset: usize,
    pub length: usize,
}

impl fmt::Display for UnserializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Error at offset {} of {} bytes",
            self.offset, self.length
        )
    }
}

impl std::error::Error for UnserializeError {}

/// `unserialize_max_depth`.
const MAX_DEPTH: usize = 4096;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

type R<T> = Result<T, usize>;

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, b: u8) -> R<()> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.pos)
        }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Digits with an optional sign, up to `end`.
    fn number_text(&mut self, end: u8) -> R<&str> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == end {
                break;
            }
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| start)?;
        self.expect(end)?;
        Ok(text)
    }

    fn int(&mut self, end: u8) -> R<i64> {
        let start = self.pos;
        let text = self.number_text(end)?;
        let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return Err(start);
        }
        text.parse::<i64>().map_err(|_| start)
    }

    fn length(&mut self, end: u8) -> R<usize> {
        let start = self.pos;
        let n = self.int(end)?;
        usize::try_from(n).map_err(|_| start)
    }

    fn quoted(&mut self, len: usize) -> R<&[u8]> {
        self.expect(b'"')?;
        if len > self.remaining() {
            return Err(self.pos);
        }
        let s = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        self.expect(b'"')?;
        Ok(s)
    }

    fn class_name(&mut self) -> R<String> {
        let start = self.pos;
        let len = self.length(b':')?;
        let name = self.quoted(len)?;
        String::from_utf8(name.to_vec()).map_err(|_| start)
    }

    /// A container's header (`<count>:{`), checked against the input before anything is allocated.
    fn open(&mut self) -> R<usize> {
        let count = self.length(b':')?;
        self.expect(b'{')?;
        // Each member takes at least 4 bytes; a larger count cannot be real.
        if count > self.remaining() / 4 + 1 {
            return Err(self.pos);
        }
        Ok(count)
    }

    /// One token: a complete scalar, or the start of an array or object.
    fn token(&mut self) -> R<Token> {
        let start = self.pos;
        let tag = self.peek().ok_or(start)?;
        self.pos += 1;
        if tag == b'N' {
            self.expect(b';')?;
            return Ok(Token::Value(PhpValue::Null));
        }
        self.expect(b':')?;
        Ok(Token::Value(match tag {
            b'b' => match self.number_text(b';')? {
                "0" => PhpValue::Bool(false),
                "1" => PhpValue::Bool(true),
                _ => return Err(start),
            },
            b'i' => PhpValue::Int(self.int(b';')?),
            b'd' => {
                let text = self.number_text(b';')?;
                PhpValue::Float(match text {
                    "INF" => f64::INFINITY,
                    "-INF" => f64::NEG_INFINITY,
                    "NAN" => f64::NAN,
                    t if !t.is_empty()
                        && t.bytes()
                            .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c)) =>
                    {
                        t.parse::<f64>().map_err(|_| start)?
                    }
                    _ => return Err(start),
                })
            }
            b's' => {
                let len = self.length(b':')?;
                let s = self.quoted(len)?.to_vec();
                self.expect(b';')?;
                PhpValue::Str(s)
            }
            b'a' => {
                return Ok(Token::Open {
                    class: None,
                    count: self.open()?,
                });
            }
            b'O' => {
                let class = self.class_name()?;
                self.expect(b':')?;
                return Ok(Token::Open {
                    class: Some(class),
                    count: self.open()?,
                });
            }
            b'C' => {
                let class = self.class_name()?;
                self.expect(b':')?;
                let len = self.length(b':')?;
                self.expect(b'{')?;
                if len > self.remaining() {
                    return Err(self.pos);
                }
                let data = self.bytes[self.pos..self.pos + len].to_vec();
                self.pos += len;
                self.expect(b'}')?;
                PhpValue::Custom { class, data }
            }
            b'E' => {
                let len = self.length(b':')?;
                let text = self.quoted(len)?.to_vec();
                self.expect(b';')?;
                let text = std::str::from_utf8(&text).map_err(|_| start)?;
                let (class, case) = text.split_once(':').ok_or(start)?;
                PhpValue::Enum {
                    class: class.into(),
                    case: case.into(),
                }
            }
            b'R' | b'r' => {
                let n = u32::try_from(self.int(b';')?).map_err(|_| start)?;
                if tag == b'R' {
                    PhpValue::Ref(n)
                } else {
                    PhpValue::ObjectRef(n)
                }
            }
            _ => return Err(start),
        }))
    }

    /// The whole value, nesting kept on a heap stack (hostile depth cannot exhaust the thread's stack).
    fn value(&mut self) -> R<PhpValue> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            // A finished container closes and becomes its parent's value.
            let mut done: Option<PhpValue> = None;
            if let Some(frame) =
                stack.pop_if(|top| top.key.is_none() && top.items.len() == top.count)
            {
                self.expect(b'}')?;
                done = Some(match frame.class {
                    Some(class) => PhpValue::Object {
                        class,
                        properties: frame.items,
                    },
                    None => PhpValue::Array(frame.items),
                });
            }
            let value = match done {
                Some(v) => v,
                None => {
                    let at = self.pos;
                    match self.token()? {
                        Token::Value(v) => v,
                        Token::Open { class, count } => {
                            if stack.last().is_some_and(|f| f.key.is_none()) {
                                return Err(at);
                            }
                            if stack.len() >= MAX_DEPTH {
                                return Err(at);
                            }
                            stack.push(Frame {
                                class,
                                count,
                                items: Vec::with_capacity(count.min(1024)),
                                key: None,
                            });
                            continue;
                        }
                    }
                }
            };
            let Some(top) = stack.last_mut() else {
                return Ok(value);
            };
            match top.key.take() {
                Some(key) => top.items.push((key, value)),
                None => {
                    let object = top.class.is_some();
                    top.key = Some(match value {
                        PhpValue::Int(i) if object => PhpKey::Str(i.to_string().into_bytes()),
                        PhpValue::Int(i) => PhpKey::Int(i),
                        PhpValue::Str(s) if !object => match canonical_int(&s) {
                            Some(i) => PhpKey::Int(i),
                            None => PhpKey::Str(s),
                        },
                        PhpValue::Str(s) => PhpKey::Str(s),
                        _ => return Err(self.pos),
                    });
                }
            }
        }
    }
}

enum Token {
    Value(PhpValue),
    Open { class: Option<String>, count: usize },
}

struct Frame {
    class: Option<String>,
    count: usize,
    items: Vec<(PhpKey, PhpValue)>,
    key: Option<PhpKey>,
}

/// PHP's symbol-table rule: a string key that is a canonical decimal `i64` is an integer key.
pub(crate) fn canonical_int(s: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(s).ok()?;
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if (digits.len() > 1 && digits.starts_with('0')) || text == "-0" {
        return None;
    }
    text.parse().ok()
}

/// PHP's `unserialize()`: bytes after the first complete value are ignored.
pub fn unserialize(bytes: &[u8]) -> Result<PhpValue, UnserializeError> {
    let mut reader = Reader { bytes, pos: 0 };
    reader.value().map_err(|offset| UnserializeError {
        offset,
        length: bytes.len(),
    })
}
