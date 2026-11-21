//! `wp_kses_check_attr_val()`, with PHP 8's comparison rules for the values a filter may leave.

use std::cmp::Ordering;

use speedyspec_wp_kses_contracts::{AttrRule, KsesError, RuleValue};

use crate::scan::is_space;

/// A number read from a PHP numeric string (leading and trailing whitespace allowed, as PHP 8 allows).
fn numeric(s: &[u8]) -> Option<f64> {
    let is_ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
    let start = s.iter().position(|b| !is_ws(b))?;
    let end = s.iter().rposition(|b| !is_ws(b))? + 1;
    let t = std::str::from_utf8(&s[start..end]).ok()?;
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let digits_ok = {
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        (!int.is_empty() || !frac.is_empty())
            && int.bytes().all(|b| b.is_ascii_digit())
            && frac.bytes().all(|b| b.is_ascii_digit())
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
    });
    (digits_ok && exponent_ok)
        .then(|| t.parse::<f64>().ok())
        .flatten()
}

fn php_bool_of_str(s: &[u8]) -> bool {
    !(s.is_empty() || s == b"0")
}

/// A PHP scalar as kses compares it.
enum Php<'a> {
    Int(i64),
    Str(&'a [u8]),
}

/// PHP 8's `<=>` between a kses-side value and a filter's value.
fn compare(a: Php<'_>, b: &RuleValue) -> Ordering {
    let num = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    match (a, b) {
        (_, RuleValue::List(_)) => Ordering::Less,
        (Php::Int(i), RuleValue::Int(j)) => i.cmp(j),
        (Php::Int(i), RuleValue::Float(f)) => num(i as f64, *f),
        (Php::Int(i), RuleValue::Str(s)) => match numeric(s) {
            Some(f) => num(i as f64, f),
            None => i.to_string().as_bytes().cmp(s),
        },
        (Php::Int(i), RuleValue::Bool(b)) => (i != 0).cmp(b),
        (Php::Int(i), RuleValue::Null) => (i != 0).cmp(&false),
        (Php::Str(s), RuleValue::Int(j)) => match numeric(s) {
            Some(f) => num(f, *j as f64),
            None => s.cmp(j.to_string().as_bytes()),
        },
        (Php::Str(s), RuleValue::Float(f)) => numeric(s).map_or(Ordering::Less, |x| num(x, *f)),
        (Php::Str(s), RuleValue::Str(t)) => match (numeric(s), numeric(t)) {
            (Some(x), Some(y)) => num(x, y),
            _ => s.cmp(t),
        },
        (Php::Str(s), RuleValue::Bool(b)) => php_bool_of_str(s).cmp(b),
        (Php::Str(s), RuleValue::Null) => s.cmp(b"".as_slice()),
    }
}

/// `strtolower()` of a filter's value as PHP coerces it to a string.
fn lower_string(v: &RuleValue) -> Vec<u8> {
    match v {
        RuleValue::Null | RuleValue::Bool(false) | RuleValue::List(_) => Vec::new(),
        RuleValue::Bool(true) => b"1".to_vec(),
        RuleValue::Int(i) => i.to_string().into_bytes(),
        RuleValue::Float(f) => f.to_string().into_bytes(),
        RuleValue::Str(s) => s.to_ascii_lowercase(),
    }
}

/// `^\s{0,6}[0-9]{1,6}\s{0,6}$`.
fn small_number(v: &[u8]) -> bool {
    let lead = v.iter().take(7).take_while(|b| is_space(**b)).count();
    if lead > 6 {
        return false;
    }
    let digits = v[lead..].iter().take_while(|b| b.is_ascii_digit()).count();
    if !(1..=6).contains(&digits) {
        return false;
    }
    let rest = &v[lead + digits..];
    let trail = rest.iter().take_while(|b| is_space(**b)).count();
    (rest.len() == trail && trail <= 6)
        || (rest.len() == trail && rest.last() == Some(&b'\n') && trail <= 7)
}

/// `wp_kses_check_attr_val()`: whether `value` passes one rule.
pub fn check_attr_val(value: &[u8], vless: bool, rule: &AttrRule) -> Result<bool, KsesError> {
    Ok(match rule {
        AttrRule::MaxLen(limit) => {
            compare(Php::Int(value.len() as i64), limit) != Ordering::Greater
        }
        AttrRule::MinLen(limit) => compare(Php::Int(value.len() as i64), limit) != Ordering::Less,
        AttrRule::MaxVal(limit) => {
            small_number(value) && compare(Php::Str(value), limit) != Ordering::Greater
        }
        AttrRule::MinVal(limit) => {
            small_number(value) && compare(Php::Str(value), limit) != Ordering::Less
        }
        AttrRule::Valueless(expected) => lower_string(expected) == if vless { b"y" } else { b"n" },
        AttrRule::Values(list) => {
            let lower = value.to_ascii_lowercase();
            list.iter()
                .any(|v| matches!(v, RuleValue::Str(s) if **s == *lower))
        }
        AttrRule::ValueCallback(callback) => callback(value)?,
        AttrRule::Other(..) => true,
    })
}
