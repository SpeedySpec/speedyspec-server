//! `json_decode( $json, true )` as PHP 8 does it: objects become arrays in key order (a later duplicate key replaces
//! the value in place), canonical integer keys become integer keys, integers that fit stay integers, and errors carry
//! PHP's `json_last_error_msg()` text.

use speedyspec_hooks::{Key, Value};

use crate::array_path::php_key;

/// Why decoding failed, as `json_last_error_msg()` words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError(pub &'static str);

const SYNTAX: JsonError = JsonError("Syntax error");
const UTF8: JsonError = JsonError("Malformed UTF-8 characters, possibly incorrectly encoded");
const DEPTH: JsonError = JsonError("Maximum stack depth exceeded");
const UTF16: JsonError = JsonError("Single unpaired UTF-16 surrogate in unicode escape");
const CONTROL: JsonError = JsonError("Control character error, possibly incorrectly encoded");

/// One decoded JSON value, keeping objects apart from arrays (`json_decode( $json )` without `associative`).
#[derive(Clone, Debug, PartialEq)]
pub enum JsonTree {
    /// A scalar or list.
    Value(Value),
    /// An object's properties, in order.
    Object(Vec<(String, JsonTree)>),
    /// A list.
    List(Vec<JsonTree>),
}

/// `json_decode( $json, true )`.
pub fn decode(json: &[u8]) -> Result<Value, JsonError> {
    decode_tree(json).map(into_value)
}

/// `json_decode( $json )`: objects stay objects.
pub fn decode_tree(json: &[u8]) -> Result<JsonTree, JsonError> {
    if std::str::from_utf8(json).is_err() {
        return Err(UTF8);
    }
    let mut parser = Parser {
        b: json,
        at: 0,
        depth: 0,
    };
    parser.space();
    let value = parser.value()?;
    parser.space();
    if parser.at != json.len() {
        return Err(SYNTAX);
    }
    Ok(value)
}

/// A tree as `json_decode( $json, true )` returns it.
pub fn into_value(tree: JsonTree) -> Value {
    match tree {
        JsonTree::Value(v) => v,
        JsonTree::List(items) => Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), into_value(v)))
                .collect(),
        ),
        JsonTree::Object(props) => {
            let mut out: Vec<(Key, Value)> = Vec::with_capacity(props.len());
            for (k, v) in props {
                let k = php_key(&k);
                let v = into_value(v);
                match out.iter_mut().find(|(ok, _)| *ok == k) {
                    Some(slot) => slot.1 = v,
                    None => out.push((k, v)),
                }
            }
            Value::Array(out)
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.at < self.b.len() && matches!(self.b[self.at], b' ' | b'\t' | b'\n' | b'\r') {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn value(&mut self) -> Result<JsonTree, JsonError> {
        match self.peek().ok_or(SYNTAX)? {
            b'{' => self.object(),
            b'[' => self.list(),
            b'"' => Ok(JsonTree::Value(crate::codec::string_value(&self.string()?))),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(SYNTAX),
        }
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > 512 { Err(DEPTH) } else { Ok(()) }
    }

    fn object(&mut self) -> Result<JsonTree, JsonError> {
        self.enter()?;
        self.at += 1;
        let mut props = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(JsonTree::Object(props));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(SYNTAX);
            }
            let key = String::from_utf8_lossy(&self.string()?).into_owned();
            self.space();
            if self.peek() != Some(b':') {
                return Err(SYNTAX);
            }
            self.at += 1;
            self.space();
            let value = self.value()?;
            props.push((key, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(SYNTAX),
            }
        }
        self.depth -= 1;
        Ok(JsonTree::Object(props))
    }

    fn list(&mut self) -> Result<JsonTree, JsonError> {
        self.enter()?;
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(JsonTree::List(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(SYNTAX),
            }
        }
        self.depth -= 1;
        Ok(JsonTree::List(items))
    }

    fn word(&mut self, word: &str, value: Value) -> Result<JsonTree, JsonError> {
        if self.b[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(JsonTree::Value(value))
        } else {
            Err(SYNTAX)
        }
    }

    fn number(&mut self) -> Result<JsonTree, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
            }
            _ => return Err(SYNTAX),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(SYNTAX);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(SYNTAX);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.at]).map_err(|_| SYNTAX)?;
        if !is_float && let Ok(i) = text.parse::<i64>() {
            return Ok(JsonTree::Value(Value::Int(i)));
        }
        Ok(JsonTree::Value(Value::Float(
            text.parse::<f64>().map_err(|_| SYNTAX)?,
        )))
    }

    fn string(&mut self) -> Result<Vec<u8>, JsonError> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek().ok_or(SYNTAX)?;
            match c {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let e = self.peek().ok_or(SYNTAX)?;
                    self.at += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code)
                                && self.b[self.at..].starts_with(b"\\u")
                            {
                                let save = self.at;
                                self.at += 2;
                                let low = self.hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                } else {
                                    self.at = save;
                                }
                            }
                            if (0xD800..0xE000).contains(&code) {
                                return Err(UTF16);
                            }
                            let ch = char::from_u32(code).ok_or(UTF8)?;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(SYNTAX),
                    }
                }
                0..=0x1f => return Err(CONTROL),
                _ => {
                    out.push(c);
                    self.at += 1;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self.b.get(self.at..self.at + 4).ok_or(SYNTAX)?;
        let text = std::str::from_utf8(digits).map_err(|_| SYNTAX)?;
        let code = u32::from_str_radix(text, 16).map_err(|_| SYNTAX)?;
        self.at += 4;
        Ok(code)
    }
}
