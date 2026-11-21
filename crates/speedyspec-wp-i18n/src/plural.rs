//! Plural-Forms expressions (Interpreter): parsed once, with `Plural_Forms`' grammar and errors, into a postfix
//! program evaluated per call on a small stack.

use std::fmt;

/// Why an expression could not be compiled or evaluated; the text is `Plural_Forms`' own message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluralError {
    /// Thrown as PHP's `Exception`.
    Exception(String),
    /// `% 0`, thrown as PHP's `DivisionByZeroError` (`Modulo by zero`).
    DivisionByZero,
}

impl PluralError {
    /// The PHP class WordPress throws for this error.
    pub fn php_class(&self) -> &'static str {
        match self {
            PluralError::Exception(_) => "Exception",
            PluralError::DivisionByZero => "DivisionByZeroError",
        }
    }
}

impl fmt::Display for PluralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluralError::Exception(message) => f.write_str(message),
            PluralError::DivisionByZero => f.write_str("Modulo by zero"),
        }
    }
}

impl std::error::Error for PluralError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    /// `?` never matched by a `:`; `execute` rejects it.
    Question,
    Ternary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Var,
    Value(i64),
    Op(Op),
}

/// What the parse stack holds: an operator, or an open parenthesis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Open,
    Op(Op),
}

impl Op {
    fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "%" => Op::Mod,
            "<" => Op::Lt,
            "<=" => Op::Le,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "==" => Op::Eq,
            "!=" => Op::Ne,
            "&&" => Op::And,
            "||" => Op::Or,
            "?" => Op::Question,
            "?:" => Op::Ternary,
            _ => return None,
        })
    }

    fn precedence(self) -> u8 {
        match self {
            Op::Mod => 6,
            Op::Lt | Op::Le | Op::Gt | Op::Ge => 5,
            Op::Eq | Op::Ne => 4,
            Op::And => 3,
            Op::Or => 2,
            Op::Question | Op::Ternary => 1,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Op::Mod => "%",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::And => "&&",
            Op::Or => "||",
            Op::Question => "?",
            Op::Ternary => "?:",
        }
    }
}

fn precedence(p: Pending) -> u8 {
    match p {
        Pending::Open => 0,
        Pending::Op(op) => op.precedence(),
    }
}

/// A PHP value on the evaluation stack; `Null` is what `array_pop()` gives on an empty stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum V {
    Null,
    Bool(bool),
    Int(i64),
}

impl V {
    fn truthy(self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => b,
            V::Int(n) => n != 0,
        }
    }

    fn int(self) -> i64 {
        match self {
            V::Null => 0,
            V::Bool(b) => i64::from(b),
            V::Int(n) => n,
        }
    }

    /// PHP 8's `<=>` between these types: two ints compare as ints; anything with a bool or null compares as bools.
    fn cmp(self, other: V) -> std::cmp::Ordering {
        match (self, other) {
            (V::Int(a), V::Int(b)) => a.cmp(&b),
            (a, b) => a.truthy().cmp(&b.truthy()),
        }
    }
}

const OP_CHARS: &[u8] = b"|&><!=%?:";

/// A compiled `Plural-Forms` expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluralRule {
    tokens: Vec<Token>,
}

impl PluralRule {
    /// `new Plural_Forms( $expression )`: the shunting-yard parse, with its errors.
    pub fn compile(expression: &str) -> Result<PluralRule, PluralError> {
        let s = expression.as_bytes();
        let mut output = Vec::new();
        let mut stack: Vec<Pending> = Vec::new();
        let mut pos = 0;
        let err = |m: String| Err(PluralError::Exception(m));
        while pos < s.len() {
            match s[pos] {
                b' ' | b'\t' => pos += 1,
                b'n' => {
                    output.push(Token::Var);
                    pos += 1;
                }
                b'(' => {
                    stack.push(Pending::Open);
                    pos += 1;
                }
                b')' => {
                    let mut found = false;
                    while let Some(top) = stack.pop() {
                        match top {
                            Pending::Open => {
                                found = true;
                                break;
                            }
                            Pending::Op(op) => output.push(Token::Op(op)),
                        }
                    }
                    if !found {
                        return err("Mismatched parentheses".into());
                    }
                    pos += 1;
                }
                b'|' | b'&' | b'>' | b'<' | b'!' | b'=' | b'%' | b'?' => {
                    let len = s[pos..].iter().take_while(|c| OP_CHARS.contains(c)).count();
                    let text = &expression[pos..pos + len];
                    let Some(op) = Op::parse(text) else {
                        return err(format!("Unknown operator \"{text}\""));
                    };
                    while let Some(&top) = stack.last() {
                        let stop = if matches!(op, Op::Ternary | Op::Question) {
                            op.precedence() >= precedence(top)
                        } else {
                            op.precedence() > precedence(top)
                        };
                        if stop {
                            break;
                        }
                        stack.pop();
                        if let Pending::Op(o) = top {
                            output.push(Token::Op(o));
                        }
                    }
                    stack.push(Pending::Op(op));
                    pos += len;
                }
                b':' => {
                    let mut found = false;
                    while let Some(&top) = stack.last() {
                        if top == Pending::Op(Op::Question) {
                            *stack.last_mut().unwrap() = Pending::Op(Op::Ternary);
                            found = true;
                            break;
                        }
                        stack.pop();
                        if let Pending::Op(o) = top {
                            output.push(Token::Op(o));
                        }
                    }
                    if !found {
                        return err("Missing starting \"?\" ternary operator".into());
                    }
                    pos += 1;
                }
                c if c.is_ascii_digit() => {
                    let len = s[pos..].iter().take_while(|c| c.is_ascii_digit()).count();
                    // intval() saturates at PHP_INT_MAX.
                    let value = expression[pos..pos + len]
                        .parse::<i64>()
                        .unwrap_or(i64::MAX);
                    output.push(Token::Value(value));
                    pos += len;
                }
                _ => {
                    let ch = expression[pos..]
                        .chars()
                        .next()
                        .map(|_| &expression[pos..pos + 1])
                        .unwrap_or("");
                    return err(format!(
                        "Unknown symbol \"{}\"",
                        String::from_utf8_lossy(ch.as_bytes())
                    ));
                }
            }
        }
        while let Some(top) = stack.pop() {
            match top {
                Pending::Open => return err("Mismatched parentheses".into()),
                Pending::Op(op) => output.push(Token::Op(op)),
            }
        }
        Ok(PluralRule { tokens: output })
    }

    /// `Plural_Forms::get( $n )`: the plural form for `n`.
    pub fn eval(&self, n: i64) -> Result<i64, PluralError> {
        let mut stack: Vec<V> = Vec::with_capacity(8);
        let pop = |stack: &mut Vec<V>| stack.pop().unwrap_or(V::Null);
        for token in &self.tokens {
            let op = match *token {
                Token::Var => {
                    stack.push(V::Int(n));
                    continue;
                }
                Token::Value(v) => {
                    stack.push(V::Int(v));
                    continue;
                }
                Token::Op(op) => op,
            };
            if op == Op::Ternary {
                let v3 = pop(&mut stack);
                let v2 = pop(&mut stack);
                let v1 = pop(&mut stack);
                stack.push(if v1.truthy() { v2 } else { v3 });
                continue;
            }
            if op == Op::Question {
                return Err(PluralError::Exception(format!(
                    "Unknown operator \"{}\"",
                    op.symbol()
                )));
            }
            let v2 = pop(&mut stack);
            let v1 = pop(&mut stack);
            use std::cmp::Ordering::*;
            stack.push(match op {
                Op::Mod => {
                    let d = v2.int();
                    if d == 0 {
                        return Err(PluralError::DivisionByZero);
                    }
                    V::Int(v1.int().wrapping_rem(d))
                }
                Op::Or => V::Bool(v1.truthy() || v2.truthy()),
                Op::And => V::Bool(v1.truthy() && v2.truthy()),
                Op::Lt => V::Bool(v1.cmp(v2) == Less),
                Op::Le => V::Bool(v1.cmp(v2) != Greater),
                Op::Gt => V::Bool(v1.cmp(v2) == Greater),
                Op::Ge => V::Bool(v1.cmp(v2) != Less),
                Op::Eq => V::Bool(v1 == v2),
                Op::Ne => V::Bool(v1 != v2),
                Op::Question | Op::Ternary => unreachable!(),
            });
        }
        if stack.len() != 1 {
            return Err(PluralError::Exception(
                "Too many values remaining on the stack".into(),
            ));
        }
        Ok(stack[0].int())
    }

    /// English: `n != 1`.
    pub fn english() -> PluralRule {
        PluralRule::compile("n != 1").expect("the English rule compiles")
    }

    /// The rule for a `Plural-Forms` header, as `WP_Translation_File::get_plural_form()` builds it: the expression
    /// after `plural=` (English when the header doesn't match), with trailing semicolons removed; English when it
    /// does not compile.
    pub fn from_header(header: &str) -> PluralRule {
        let expression = expression_from_header(header).unwrap_or_else(|| "n != 1".to_owned());
        PluralRule::compile(expression.trim_end_matches(';'))
            .unwrap_or_else(|_| PluralRule::english())
    }
}

fn is_pcre_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')
}

/// `/^\s*nplurals\s*=\s*(\d+)\s*;\s+plural\s*=\s*(.+)$/`, then `trim()`.
fn expression_from_header(header: &str) -> Option<String> {
    let s = header.as_bytes();
    let mut i = 0;
    let skip = |i: &mut usize| {
        while *i < s.len() && is_pcre_space(s[*i]) {
            *i += 1;
        }
    };
    let lit = |i: &mut usize, l: &[u8]| -> bool {
        if s[*i..].starts_with(l) {
            *i += l.len();
            true
        } else {
            false
        }
    };
    skip(&mut i);
    if !lit(&mut i, b"nplurals") {
        return None;
    }
    skip(&mut i);
    if !lit(&mut i, b"=") {
        return None;
    }
    skip(&mut i);
    let digits = s[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    i += digits;
    skip(&mut i);
    if !lit(&mut i, b";") {
        return None;
    }
    let spaces = s[i..].iter().take_while(|c| is_pcre_space(**c)).count();
    if spaces == 0 {
        return None;
    }
    i += spaces;
    if !lit(&mut i, b"plural") {
        return None;
    }
    skip(&mut i);
    if !lit(&mut i, b"=") {
        return None;
    }
    // `\s*(.+)$`: `.` stops at a newline, and `$` also matches before one final newline.
    let mut rest = &s[i..];
    if rest.last() == Some(&b'\n') {
        rest = &rest[..rest.len() - 1];
    }
    if rest.is_empty() || rest.contains(&b'\n') {
        return None;
    }
    let trimmed = std::str::from_utf8(rest)
        .ok()?
        .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'));
    Some(trimmed.to_owned())
}
