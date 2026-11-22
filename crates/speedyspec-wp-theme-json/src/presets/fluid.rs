//! Fluid font sizes: `wp_get_typography_font_size_value()` and its helpers, copied from WordPress's block supports
//! (which keep their own copy) because preset variables and `font-size` declarations call it.

use std::rc::Rc;
use std::sync::LazyLock;

use fancy_regex::Regex;
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_theme_json_contracts::ports::{Diagnostics, GlobalSettings};

use crate::array_path::{self, child};
use crate::php::{float_to_string, round, to_number, to_php_string};
use crate::php_value::{empty, s};

static SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]*\.?[0-9]+)([a-zA-Z]+|%)$").expect("compiles"));

/// A size split into its number and unit (`wp_get_typography_value_and_unit`'s result).
#[derive(Clone, Debug, PartialEq)]
pub struct ValueAndUnit {
    /// The number, rounded to three places.
    pub value: f64,
    /// `px`, `rem` or `em`.
    pub unit: String,
}

impl ValueAndUnit {
    /// The size written back as `value . unit`.
    pub fn css(&self) -> String {
        format!("{}{}", float_to_string(self.value), self.unit)
    }

    /// As WordPress's array (`value`, `unit`).
    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            (Key::Str("value".into()), Value::Float(self.value)),
            (Key::Str("unit".into()), s(self.unit.clone())),
        ])
    }
}

/// `(float) $string`: the leading number of a string, as PHP reads it.
fn leading_float(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(0.0)
}

/// `wp_get_typography_value_and_unit( $raw_value, array( 'coerce_to' => $coerce_to ) )`.
pub fn typography_value_and_unit(
    raw: &Value,
    coerce_to: &str,
    diagnostics: &dyn Diagnostics,
) -> Option<ValueAndUnit> {
    if !matches!(
        raw,
        Value::Str(_) | Value::Bytes(_) | Value::Int(_) | Value::Float(_)
    ) {
        diagnostics.doing_it_wrong(
            "wp_get_typography_value_and_unit",
            b"Raw size value must be a string, integer, or float.",
            "6.1.0",
        );
        return None;
    }
    if empty(raw) {
        return None;
    }
    let mut text = String::from_utf8_lossy(&to_php_string(raw)).into_owned();
    if to_number(raw).is_some() {
        text.push_str("px");
    }
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let captures = SIZE.captures(body).ok().flatten()?;
    let mut value = leading_float(&captures[1]);
    let mut unit = captures[2].to_owned();
    if !["rem", "px", "em"].contains(&unit.as_str()) {
        return None;
    }
    let root_size = 16.0;
    if coerce_to == "px" && (unit == "em" || unit == "rem") {
        value *= root_size;
        unit = coerce_to.to_owned();
    }
    if unit == "px" && (coerce_to == "em" || coerce_to == "rem") {
        value /= root_size;
        unit = coerce_to.to_owned();
    }
    if (coerce_to == "em" || coerce_to == "rem") && (unit == "em" || unit == "rem") {
        unit = coerce_to.to_owned();
    }
    Some(ValueAndUnit {
        value: round(value, 3),
        unit,
    })
}

/// `wp_get_computed_fluid_typography_value( $args )`'s arguments.
pub struct FluidArgs<'a> {
    /// `minimum_viewport_width`.
    pub minimum_viewport_width: &'a Value,
    /// `maximum_viewport_width`.
    pub maximum_viewport_width: &'a Value,
    /// `minimum_font_size`.
    pub minimum_font_size: &'a Value,
    /// `maximum_font_size`.
    pub maximum_font_size: &'a Value,
    /// `scale_factor`.
    pub scale_factor: f64,
}

/// `wp_get_computed_fluid_typography_value( $args )`: a `clamp()` value, or `None` for WordPress's `null`.
pub fn computed_fluid_typography_value(
    args: &FluidArgs<'_>,
    diagnostics: &dyn Diagnostics,
) -> Option<String> {
    let minimum_font_size = typography_value_and_unit(args.minimum_font_size, "", diagnostics);
    let unit = minimum_font_size
        .as_ref()
        .map_or("rem".to_owned(), |m| m.unit.clone());
    let maximum_font_size = typography_value_and_unit(args.maximum_font_size, &unit, diagnostics);
    let (maximum_font_size, minimum_font_size) = (maximum_font_size?, minimum_font_size?);
    let minimum_font_size_rem =
        typography_value_and_unit(args.minimum_font_size, "rem", diagnostics);
    let maximum_viewport_width =
        typography_value_and_unit(args.maximum_viewport_width, &unit, diagnostics);
    let minimum_viewport_width =
        typography_value_and_unit(args.minimum_viewport_width, &unit, diagnostics);
    let (minimum_viewport_width, maximum_viewport_width) =
        (minimum_viewport_width?, maximum_viewport_width?);
    let denominator = maximum_viewport_width.value - minimum_viewport_width.value;
    if denominator == 0.0 {
        return None;
    }
    let offset = format!(
        "{}{unit}",
        float_to_string(round(minimum_viewport_width.value / 100.0, 3))
    );
    let linear_factor = 100.0 * ((maximum_font_size.value - minimum_font_size.value) / denominator);
    let scaled = round(linear_factor * args.scale_factor, 3);
    let scaled = if scaled == 0.0 {
        "1".to_owned()
    } else {
        float_to_string(scaled)
    };
    let rem = minimum_font_size_rem.map(|m| m.css()).unwrap_or_default();
    let target = format!("{rem} + ((1vw - {offset}) * {scaled})");
    Some(format!(
        "clamp({}, {target}, {})",
        String::from_utf8_lossy(&to_php_string(args.minimum_font_size)),
        String::from_utf8_lossy(&to_php_string(args.maximum_font_size))
    ))
}

/// `array_merge( $a, $b )` for two arrays: `b`'s string keys replace `a`'s, integer keys are appended renumbered.
fn array_merge(a: &Value, b: &Value) -> Value {
    let mut out: Vec<(Key, Value)> = Vec::new();
    let mut next = 0;
    for source in [a, b] {
        if let Value::Array(entries) = source {
            for (k, v) in entries {
                match k {
                    Key::Int(_) => {
                        out.push((Key::Int(next), v.clone()));
                        next += 1;
                    }
                    Key::Str(_) => match out.iter_mut().find(|(ok, _)| ok == k) {
                        Some(slot) => slot.1 = v.clone(),
                        None => out.push((k.clone(), v.clone())),
                    },
                }
            }
        }
    }
    Value::Array(out)
}

/// `wp_parse_args( $args, $defaults )` for an array or other `args`.
fn parse_args(args: &Value, defaults: &Value) -> Value {
    match args {
        Value::Array(_) => array_merge(defaults, args),
        _ => defaults.clone(),
    }
}

/// Fluid font sizes (a Strategy behind `PRESETS_METADATA`'s `value_func` and `font-size` declarations).
#[derive(Clone)]
pub struct FluidFontSize {
    /// The site's merged settings, merged under the settings given.
    pub global: Rc<dyn GlobalSettings>,
    /// Where notices go.
    pub diagnostics: Rc<dyn Diagnostics>,
}

impl FluidFontSize {
    /// `wp_get_typography_font_size_value( $preset, $settings )`.
    pub fn value(&self, preset: &Value, settings: &Value) -> Value {
        let d = &*self.diagnostics;
        let Some(size) = child(preset, &Key::Str("size".into()))
            .filter(|v| !matches!(v, Value::Null))
            .cloned()
        else {
            return Value::Null;
        };
        let fluid = child(preset, &Key::Str("fluid".into()))
            .cloned()
            .unwrap_or(Value::Null);
        if matches!(fluid, Value::Bool(false)) || empty(&size) {
            return size;
        }
        let settings = match settings {
            Value::Bool(b) => {
                d.deprecated_argument(
                    "wp_get_typography_font_size_value",
                    "6.6.0",
                    b"`boolean` type for second argument `$settings` is deprecated. Use `array()` instead.",
                );
                Value::Array(vec![(
                    Key::Str("typography".into()),
                    Value::Array(vec![(Key::Str("fluid".into()), Value::Bool(*b))]),
                )])
            }
            other => other.clone(),
        };
        let settings = parse_args(&settings, &self.global.settings());
        let typography = child(&settings, &Key::Str("typography".into()))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let fluid_setting = array_path::get(&typography, &["fluid"])
            .cloned()
            .unwrap_or(Value::Null);
        if empty(&fluid_setting) && empty(&fluid) {
            return size;
        }
        let fluid_settings = if matches!(fluid_setting, Value::Null) {
            Value::Array(Vec::new())
        } else {
            fluid_setting
        };
        let layout = child(&settings, &Key::Str("layout".into()))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let at = |v: &Value, k: &str| {
            array_path::get(v, &[k])
                .filter(|x| !matches!(x, Value::Null))
                .cloned()
        };

        let minimum_viewport_width =
            at(&fluid_settings, "minViewportWidth").unwrap_or_else(|| s("320px"));
        let mut maximum_viewport_width = match at(&layout, "wideSize") {
            Some(wide) if typography_value_and_unit(&wide, "", d).is_some() => wide,
            _ => s("1600px"),
        };
        if let Some(max) = at(&fluid_settings, "maxViewportWidth") {
            maximum_viewport_width = max;
        }
        let minimum_font_size_limit = match at(&fluid_settings, "minFontSize") {
            Some(min) if typography_value_and_unit(&min, "", d).is_some() => min,
            _ => s("14px"),
        };
        let mut minimum_font_size_raw = at(&fluid, "min").unwrap_or(Value::Null);
        let mut maximum_font_size_raw = at(&fluid, "max").unwrap_or(Value::Null);

        let Some(preferred) = typography_value_and_unit(&size, "", d) else {
            return size;
        };
        if preferred.unit.is_empty() || preferred.unit == "0" {
            return size;
        }
        let limit = typography_value_and_unit(&minimum_font_size_limit, &preferred.unit, d);
        if let Some(limit) = &limit
            && empty(&minimum_font_size_raw)
            && empty(&maximum_font_size_raw)
            && preferred.value <= limit.value
        {
            return size;
        }
        if empty(&maximum_font_size_raw) {
            maximum_font_size_raw = s(preferred.css());
        }
        if empty(&minimum_font_size_raw) {
            let px = if preferred.unit == "px" {
                preferred.value
            } else {
                preferred.value * 16.0
            };
            let factor = (1.0 - 0.075 * px.log2()).clamp(0.25, 0.75);
            let calculated = round(preferred.value * factor, 3);
            minimum_font_size_raw = match &limit {
                Some(limit) if calculated <= limit.value => s(limit.css()),
                _ => s(format!("{}{}", float_to_string(calculated), preferred.unit)),
            };
        }
        let args = FluidArgs {
            minimum_viewport_width: &minimum_viewport_width,
            maximum_viewport_width: &maximum_viewport_width,
            minimum_font_size: &minimum_font_size_raw,
            maximum_font_size: &maximum_font_size_raw,
            scale_factor: 1.0,
        };
        match computed_fluid_typography_value(&args, d) {
            Some(value) if !value.is_empty() => s(value),
            _ => size,
        }
    }
}
