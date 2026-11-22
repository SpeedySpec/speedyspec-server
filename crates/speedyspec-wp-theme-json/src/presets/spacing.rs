//! Spacing sizes generated from `settings.spacing.spacingScale` (`compute_spacing_sizes`, `merge_spacing_sizes`).

use speedyspec_hooks::{Key, Value};

use crate::ThemeJsonDeps;
use crate::array_path::{child, php_key};
use crate::codec::string_value;
use crate::php::{Num, float_to_string, round, to_number, to_php_string};

fn field<'a>(scale: &'a Value, name: &str) -> Option<&'a Value> {
    child(scale, &Key::Str(name.into())).filter(|v| !matches!(v, Value::Null))
}

fn str_is(value: Option<&Value>, s: &str) -> bool {
    matches!(value, Some(Value::Str(v)) if v == s)
}

fn size(name: Vec<u8>, slug: String, size: Vec<u8>) -> Value {
    Value::Array(vec![
        (Key::Str("name".into()), string_value(&name)),
        (Key::Str("slug".into()), Value::Str(slug)),
        (Key::Str("size".into()), string_value(&size)),
    ])
}

/// `sprintf( __( '%sX-Small' ), (string) $count )`.
fn counted(deps: &ThemeJsonDeps, format: &str, count: Option<i64>) -> Vec<u8> {
    let template = deps.gettext.translate(format, "default");
    let count = count.map(|c| c.to_string()).unwrap_or_default();
    let mut out = Vec::new();
    let mut rest = template.as_slice();
    while let Some(i) = rest.windows(2).position(|w| w == b"%s") {
        out.extend_from_slice(&rest[..i]);
        out.extend_from_slice(count.as_bytes());
        rest = &rest[i + 2..];
    }
    out.extend_from_slice(rest);
    out
}

/// `WP_Theme_JSON::compute_spacing_sizes( $spacing_scale )`: the sizes around the medium step, as WordPress
/// computes them, with PHP's arithmetic, rounding and number formatting.
pub fn compute_spacing_sizes(scale: &Value, deps: &ThemeJsonDeps) -> Value {
    let steps_value = field(scale, "steps");
    let steps = steps_value.and_then(to_number);
    let medium_value = field(scale, "mediumStep");
    let medium = medium_value.and_then(to_number);
    let operator = field(scale, "operator");
    let increment = field(scale, "increment").and_then(to_number);
    let unit_value = field(scale, "unit");
    let (Some(steps), Some(medium), Some(increment), Some(unit_value), Some(medium_value)) =
        (steps, medium, increment, unit_value, medium_value)
    else {
        return Value::Array(Vec::new());
    };
    let plus = str_is(operator, "+");
    if matches!(steps_value, Some(Value::Int(0))) || !(plus || str_is(operator, "*")) {
        return Value::Array(Vec::new());
    }
    let unit = if str_is(Some(unit_value), "%") {
        b"%".to_vec()
    } else {
        deps.text.sanitize_title(&to_php_string(unit_value))
    };
    let with_unit = |n: Num| {
        let mut out = float_to_string(round(n.as_f64(), 2)).into_bytes();
        out.extend_from_slice(&unit);
        out
    };

    let mid = round((steps / Num::Int(2)).as_f64(), 0);
    let mut current = medium;
    let mut x_small: Option<i64> = None;
    let mut below = Vec::new();
    let mut slug: i64 = 40;
    let mut remainder = 0.0;
    let mut count = mid - 1.0;
    while steps.as_f64() > 1.0 && slug > 0 && count > 0.0 {
        current = if plus {
            current - increment
        } else if increment.as_f64() > 1.0 {
            current / increment
        } else {
            current * increment
        };
        if current.as_f64() <= 0.0 {
            remainder = count;
            break;
        }
        let name = if count == mid - 1.0 {
            deps.gettext.translate("Small", "default")
        } else {
            counted(deps, "%sX-Small", x_small)
        };
        below.push(size(name, slug.to_string(), with_unit(current)));
        if count == mid - 2.0 {
            x_small = Some(2);
        }
        if count < mid - 2.0 {
            x_small = Some(x_small.unwrap_or(0) + 1);
        }
        slug -= 10;
        count -= 1.0;
    }
    below.reverse();
    let mut medium_size = to_php_string(medium_value);
    medium_size.extend_from_slice(&unit);
    below.push(size(
        deps.gettext.translate("Medium", "default"),
        "50".into(),
        medium_size,
    ));

    let mut current = medium;
    let mut x_large: Option<i64> = None;
    let mut slug: i64 = 60;
    let above = (steps - Num::Float(mid)).as_f64() + remainder;
    let mut index: i64 = 0;
    while (index as f64) < above {
        current = if plus {
            current + increment
        } else if increment.as_f64() >= 1.0 {
            current * increment
        } else {
            current / increment
        };
        let name = if index == 0 {
            deps.gettext.translate("Large", "default")
        } else {
            counted(deps, "%sX-Large", x_large)
        };
        below.push(size(name, slug.to_string(), with_unit(current)));
        if index == 1 {
            x_large = Some(2);
        }
        if index > 1 {
            x_large = Some(x_large.unwrap_or(0) + 1);
        }
        slug += 10;
        index += 1;
    }
    Value::Array(
        below
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

/// The array key `$merged[ $item['slug'] ]` uses.
fn slug_key(item: &Value) -> Key {
    match child(item, &Key::Str("slug".into())) {
        Some(Value::Int(i)) => Key::Int(*i),
        Some(Value::Float(f)) => Key::Int(*f as i64),
        Some(Value::Bool(b)) => Key::Int(i64::from(*b)),
        Some(v @ (Value::Str(_) | Value::Bytes(_))) => {
            php_key(&String::from_utf8_lossy(&to_php_string(v)))
        }
        _ => Key::Str(String::new()),
    }
}

/// `ksort( …, SORT_NUMERIC )`'s view of a key.
fn numeric_key(key: &Key) -> f64 {
    match key {
        Key::Int(i) => *i as f64,
        Key::Str(s) => {
            let lead: String = s
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
                .collect();
            (1..=lead.len())
                .rev()
                .find_map(|n| lead[..n].parse::<f64>().ok())
                .unwrap_or(0.0)
        }
    }
}

/// `WP_Theme_JSON::merge_spacing_sizes( $base, $incoming )`: incoming sizes replace generated ones of the same slug,
/// sorted by slug as numbers. Without generated sizes the incoming ones are returned as they are.
pub fn merge_spacing_sizes(base: &Value, incoming: &Value) -> Value {
    let base_items = match base {
        Value::Array(e) if !e.is_empty() => e,
        _ => return incoming.clone(),
    };
    let mut merged: Vec<(Key, Value)> = Vec::new();
    let items = base_items.iter().chain(match incoming {
        Value::Array(e) => e.iter(),
        _ => [].iter(),
    });
    for (_, item) in items {
        let key = slug_key(item);
        match merged.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = item.clone(),
            None => merged.push((key, item.clone())),
        }
    }
    merged.sort_by(|(a, _), (b, _)| {
        numeric_key(a)
            .partial_cmp(&numeric_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Value::Array(
        merged
            .into_iter()
            .enumerate()
            .map(|(i, (_, v))| (Key::Int(i as i64), v))
            .collect(),
    )
}
