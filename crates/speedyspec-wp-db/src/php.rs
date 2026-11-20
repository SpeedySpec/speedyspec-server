//! The few PHP conversions `wpdb`'s rules depend on: string and number casts, and a `sprintf` subset.

use speedyspec_hooks::Value;

/// `is_scalar()`.
pub(crate) fn is_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Bytes(_)
    )
}

/// PHP's float-to-string conversion (`serialize_precision = -1`).
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
    // Shortest round-trip digits and the decimal exponent.
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("exponent is an integer");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if f < 0.0 { "-" } else { "" };
    if (-5..15).contains(&exp) {
        let n = digits.len() as i32;
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
        let frac = if digits.len() > 1 { &digits[1..] } else { "0" };
        format!(
            "{sign}{}.{frac}E{}{}",
            &digits[..1],
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

/// `(string) $value` as bytes; arrays and objects give `None` (callers decide).
pub(crate) fn to_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Null => Some(Vec::new()),
        Value::Bool(b) => Some(if *b { b"1".to_vec() } else { Vec::new() }),
        Value::Int(i) => Some(i.to_string().into_bytes()),
        Value::Float(f) => Some(float_to_string(*f).into_bytes()),
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        Value::Array(_) | Value::Opaque(_) => None,
    }
}

/// The leading numeric part of a PHP string, as `(int)` and `(float)` read it.
fn numeric_prefix(bytes: &[u8]) -> (f64, bool) {
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let start = i;
    if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut is_float = false;
    let mut end = i;
    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 || i > digits_start {
            is_float = j > i + 1 || is_float;
            end = j;
            i = j;
        }
    }
    if end == digits_start {
        return (0.0, false);
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
            j += 1;
        }
        let exp_digits = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_digits {
            is_float = true;
            end = j;
        }
    }
    let text = std::str::from_utf8(&bytes[start..end]).unwrap_or("0");
    (text.parse::<f64>().unwrap_or(0.0), is_float)
}

/// `(int) $value`.
pub(crate) fn to_int(v: &Value) -> i64 {
    match v {
        Value::Null => 0,
        Value::Bool(b) => i64::from(*b),
        Value::Int(i) => *i,
        Value::Float(f) => float_to_int(*f),
        Value::Str(_) | Value::Bytes(_) => {
            let bytes = to_bytes(v).unwrap_or_default();
            let (n, is_float) = numeric_prefix(&bytes);
            if is_float {
                float_to_int(n)
            } else {
                // An integer string outside i64 saturates, as PHP's strtol does.
                let text = String::from_utf8_lossy(&bytes);
                let t = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
                let len = t
                    .bytes()
                    .enumerate()
                    .take_while(|(i, c)| {
                        c.is_ascii_digit() || (*i == 0 && (*c == b'+' || *c == b'-'))
                    })
                    .count();
                t[..len].parse::<i64>().unwrap_or(if n < 0.0 {
                    i64::MIN
                } else if n > 0.0 {
                    i64::MAX
                } else {
                    0
                })
            }
        }
        Value::Array(items) => i64::from(!items.is_empty()),
        Value::Opaque(_) => 1,
    }
}

fn float_to_int(f: f64) -> i64 {
    if f.is_finite() && f >= i64::MIN as f64 && f < i64::MAX as f64 {
        f as i64
    } else {
        0
    }
}

/// `(float) $value`.
pub(crate) fn to_float(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::Str(_) | Value::Bytes(_) => numeric_prefix(&to_bytes(v).unwrap_or_default()).0,
        Value::Array(items) => f64::from(u8::from(!items.is_empty())),
        Value::Opaque(_) => 1.0,
    }
}

/// `vsprintf()` for the conversions `prepare()` produces: `s`, `d`, `F`, `f` and `%%`, with argnum, flags, width
/// and precision. `None` where PHP throws (too few arguments, an unknown specifier).
pub(crate) fn vsprintf(format: &[u8], args: &[Value]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(format.len());
    let mut i = 0;
    let mut next_arg = 0usize;
    while i < format.len() {
        if format[i] != b'%' {
            out.push(format[i]);
            i += 1;
            continue;
        }
        i += 1;
        if format.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }
        // argnum
        let mut j = i;
        while j < format.len() && format[j].is_ascii_digit() {
            j += 1;
        }
        let mut argnum = None;
        if j > i && format.get(j) == Some(&b'$') {
            argnum = std::str::from_utf8(&format[i..j])
                .ok()?
                .parse::<usize>()
                .ok()?
                .checked_sub(1);
            argnum?;
            i = j + 1;
        }
        // flags
        let (mut left, mut plus, mut pad) = (false, false, b' ');
        loop {
            match format.get(i) {
                Some(b'-') => left = true,
                Some(b'+') => plus = true,
                Some(b'0') => pad = b'0',
                Some(b' ') => pad = b' ',
                Some(b'\'') => {
                    pad = *format.get(i + 1)?;
                    i += 1;
                }
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        while let Some(d) = format.get(i).filter(|c| c.is_ascii_digit()) {
            width = width * 10 + usize::from(d - b'0');
            i += 1;
        }
        let mut precision = None;
        if format.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0usize;
            while let Some(d) = format.get(i).filter(|c| c.is_ascii_digit()) {
                p = p * 10 + usize::from(d - b'0');
                i += 1;
            }
            precision = Some(p);
        }
        let spec = *format.get(i)?;
        i += 1;
        let index = match argnum {
            Some(n) => n,
            None => {
                next_arg += 1;
                next_arg - 1
            }
        };
        let arg = args.get(index)?;
        let (body, negative) = match spec {
            b's' => {
                let mut b = to_bytes(arg).unwrap_or_else(|| b"Array".to_vec());
                if let Some(p) = precision {
                    b.truncate(p);
                }
                (b, false)
            }
            b'd' => {
                let n = to_int(arg);
                (n.unsigned_abs().to_string().into_bytes(), n < 0)
            }
            b'F' | b'f' => {
                let f = to_float(arg);
                let p = precision.unwrap_or(6).min(53);
                (
                    format!("{:.*}", p, f.abs()).into_bytes(),
                    f.is_sign_negative() && f != 0.0,
                )
            }
            _ => return None,
        };
        let sign: &[u8] = if negative {
            b"-"
        } else if plus && spec != b's' {
            b"+"
        } else {
            b""
        };
        let len = sign.len() + body.len();
        let fill = width.saturating_sub(len);
        if left {
            out.extend_from_slice(sign);
            out.extend_from_slice(&body);
            // PHP pads a left-justified number with spaces, never zeros, except for strings.
            let right_pad = if pad == b'0' && spec != b's' {
                b' '
            } else {
                pad
            };
            out.extend(std::iter::repeat_n(right_pad, fill));
        } else if pad == b'0' && spec != b's' {
            out.extend_from_slice(sign);
            out.extend(std::iter::repeat_n(b'0', fill));
            out.extend_from_slice(&body);
        } else {
            out.extend(std::iter::repeat_n(pad, fill));
            out.extend_from_slice(sign);
            out.extend_from_slice(&body);
        }
    }
    Some(out)
}
