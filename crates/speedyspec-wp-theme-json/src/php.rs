//! The PHP numeric rules theme.json's arithmetic depends on: numeric strings, loose arithmetic, `round()` and the
//! string cast of floats (`precision = 14`).

use speedyspec_hooks::Value;

/// A PHP number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
}

impl Num {
    /// The number as a float.
    pub fn as_f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    fn op(self, other: Num, int: fn(i64, i64) -> Option<i64>, float: fn(f64, f64) -> f64) -> Num {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => int(a, b)
                .map(Num::Int)
                .unwrap_or_else(|| Num::Float(float(a as f64, b as f64))),
            _ => Num::Float(float(self.as_f64(), other.as_f64())),
        }
    }
}

impl std::ops::Add for Num {
    type Output = Num;

    /// `$a + $b`.
    fn add(self, other: Num) -> Num {
        self.op(other, i64::checked_add, |a, b| a + b)
    }
}

impl std::ops::Sub for Num {
    type Output = Num;

    /// `$a - $b`.
    fn sub(self, other: Num) -> Num {
        self.op(other, i64::checked_sub, |a, b| a - b)
    }
}

impl std::ops::Mul for Num {
    type Output = Num;

    /// `$a * $b`.
    fn mul(self, other: Num) -> Num {
        self.op(other, i64::checked_mul, |a, b| a * b)
    }
}

impl std::ops::Div for Num {
    type Output = Num;

    /// `$a / $b`: an integer when both are integers and the division is exact.
    fn div(self, other: Num) -> Num {
        self.op(
            other,
            |a, b| {
                if b != 0 && a % b == 0 {
                    a.checked_div(b)
                } else {
                    None
                }
            },
            |a, b| a / b,
        )
    }
}

/// PHP's whitespace for numeric strings.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

/// PHP 8's numeric strings: optional whitespace, an optional sign, digits with an optional fraction and exponent,
/// optional whitespace.
pub fn numeric_string(s: &[u8]) -> Option<Num> {
    let start = s.iter().position(|c| !is_space(*c))?;
    let end = s.iter().rposition(|c| !is_space(*c))? + 1;
    let body = &s[start..end];
    let mut i = usize::from(matches!(body.first(), Some(b'+' | b'-')));
    let digits_start = i;
    while i < body.len() && body[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - digits_start;
    let mut float = false;
    let mut frac_digits = 0;
    if i < body.len() && body[i] == b'.' {
        float = true;
        i += 1;
        let f = i;
        while i < body.len() && body[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - f;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if i < body.len() && matches!(body[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < body.len() && matches!(body[j], b'+' | b'-') {
            j += 1;
        }
        let e = j;
        while j < body.len() && body[j].is_ascii_digit() {
            j += 1;
        }
        if j > e {
            float = true;
            i = j;
        }
    }
    if i != body.len() {
        return None;
    }
    let text = std::str::from_utf8(body).ok()?;
    if !float && let Ok(n) = text.parse::<i64>() {
        return Some(Num::Int(n));
    }
    text.parse::<f64>().ok().map(Num::Float)
}

/// `is_numeric( $value )` as a number.
pub fn to_number(value: &Value) -> Option<Num> {
    match value {
        Value::Int(i) => Some(Num::Int(*i)),
        Value::Float(f) => Some(Num::Float(*f)),
        Value::Str(s) => numeric_string(s.as_bytes()),
        Value::Bytes(b) => numeric_string(b),
        _ => None,
    }
}

/// `round( $value, $places )` with PHP 8.4's algorithm: a value that sits on the decimal edge, as its decimal
/// representation shows (`0.285`), rounds away from zero.
pub fn round(value: f64, places: i32) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let exponent = 10f64.powi(places.abs());
    let scaled = if places > 0 {
        value * exponent
    } else {
        value / exponent
    };
    let (mut integral, next) = if value >= 0.0 {
        (scaled.floor(), scaled.floor() + 1.0)
    } else {
        (scaled.ceil(), scaled.ceil() - 1.0)
    };
    let back = |x: f64| {
        if places > 0 {
            x / exponent
        } else {
            x * exponent
        }
    };
    if back(next) == value {
        integral = next;
    }
    if integral.abs() >= 1e16 {
        return value;
    }
    let edge = back(integral + 0.5f64.copysign(integral)).abs();
    if value.abs() >= edge {
        integral += 1.0f64.copysign(integral);
    }
    if places.abs() < 23 {
        back(integral)
    } else {
        value
    }
}

/// A float as PHP's string cast writes it (`precision = 14`): 14 significant digits, trailing zeros dropped, the
/// exponent form outside decimal exponents -4 to 13 (`1.0E+25`).
pub fn float_to_string(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let sci = format!("{:.13e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let n = digits.len() as i32;
    let sign = if f < 0.0 { "-" } else { "" };
    if (-4..14).contains(&exp) {
        let body = if exp >= 0 {
            if n > exp + 1 {
                format!(
                    "{}.{}",
                    &digits[..(exp + 1) as usize],
                    &digits[(exp + 1) as usize..]
                )
            } else {
                format!("{digits}{}", "0".repeat((exp + 1 - n) as usize))
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
        };
        format!("{sign}{body}")
    } else {
        let frac = if n > 1 { &digits[1..] } else { "0" };
        format!(
            "{sign}{}.{frac}E{}{}",
            &digits[..1],
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

/// A number as PHP's string cast writes it.
pub fn num_to_string(n: Num) -> String {
    match n {
        Num::Int(i) => i.to_string(),
        Num::Float(f) => float_to_string(f),
    }
}

/// A scalar as PHP's string cast writes it (`true` is `"1"`, `false` and `null` are `""`); arrays are `"Array"`.
pub fn to_php_string(value: &Value) -> Vec<u8> {
    match value {
        Value::Null | Value::Bool(false) => Vec::new(),
        Value::Bool(true) => b"1".to_vec(),
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Float(f) => float_to_string(*f).into_bytes(),
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.clone(),
        Value::Array(_) => b"Array".to_vec(),
        Value::Opaque(_) => Vec::new(),
    }
}
