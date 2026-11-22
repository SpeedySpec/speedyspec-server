//! `WP_Duotone`'s private `colord_*` methods: CSS colour strings (hex, `rgb()`/`rgba()`, `hsl()`/`hsla()`) to RGBA,
//! exactly as WordPress computes them.

use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::php::{float_prefix, round};

/// A colour: red, green and blue from 0 to 255, alpha from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
    /// Alpha.
    pub a: f64,
}

/// PCRE's `$` (end, or before a final newline) and `\s` without `/u`.
const END: &str = r"\n?\z";
const S: &str = r"[ \t\n\x{0b}\x{0c}\r]";
const NUM: &str = r"([+-]?[0-9]*\.?[0-9]+)";

static HEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)^#([0-9a-f]{{3,8}}){END}")).expect("compiles"));
static RGBA_COMMAS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^rgba?\({S}*{NUM}(%)?{S}*,{S}*{NUM}(%)?{S}*,{S}*{NUM}(%)?{S}*(?:,{S}*{NUM}(%)?{S}*)?\){END}"
    ))
    .expect("compiles")
});
static RGBA_SPACES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^rgba?\({S}*{NUM}(%)?{S}+{NUM}(%)?{S}+{NUM}(%)?{S}*(?:/{S}*{NUM}(%)?{S}*)?\){END}"
    ))
    .expect("compiles")
});
static HSLA_COMMAS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^hsla?\({S}*{NUM}(deg|rad|grad|turn)?{S}*,{S}*{NUM}%{S}*,{S}*{NUM}%{S}*(?:,{S}*{NUM}(%)?{S}*)?\){END}"
    ))
    .expect("compiles")
});
static HSLA_SPACES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^hsla?\({S}*{NUM}(deg|rad|grad|turn)?{S}+{NUM}%{S}+{NUM}%{S}*(?:/{S}*{NUM}(%)?{S}*)?\){END}"
    ))
    .expect("compiles")
});

/// The capture groups 1..=n, `""` for those that did not take part (`preg_match` fills them so).
fn groups(re: &Regex, input: &str, n: usize) -> Option<Vec<String>> {
    let captures = re.captures(input).ok().flatten()?;
    Some(
        (1..=n)
            .map(|i| {
                captures
                    .get(i)
                    .map_or(String::new(), |m| m.as_str().to_owned())
            })
            .collect(),
    )
}

/// `colord_clamp( $number, $min, $max )`.
fn clamp(number: f64, min: f64, max: f64) -> f64 {
    if number > max {
        max
    } else if number > min {
        number
    } else {
        min
    }
}

/// `colord_clamp_hue( $degrees )`: PHP's `%` truncates to an integer first.
fn clamp_hue(degrees: f64) -> f64 {
    let degrees = if degrees.is_finite() {
        (php_float_to_int(degrees) % 360) as f64
    } else {
        0.0
    };
    if degrees > 0.0 {
        degrees
    } else {
        degrees + 360.0
    }
}

/// PHP 8's float to integer conversion for `%`: out-of-range values wrap modulo 2^64 instead of saturating.
fn php_float_to_int(value: f64) -> i64 {
    let truncated = value.trunc();
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&truncated) {
        return truncated as i64;
    }
    let two64 = 18_446_744_073_709_551_616.0_f64;
    let mut m = truncated % two64;
    if m >= 9_223_372_036_854_775_808.0 {
        m -= two64;
    } else if m < -9_223_372_036_854_775_808.0 {
        m += two64;
    }
    m as i64
}

/// `colord_parse_hue( $value, $unit )`.
fn parse_hue(value: &str, unit: &str) -> f64 {
    let factor = match unit {
        "grad" => 360.0 / 400.0,
        "turn" => 360.0,
        "rad" => 360.0 / (std::f64::consts::PI * 2.0),
        _ => 1.0,
    };
    float_prefix(value) * factor
}

fn hex_pair(digits: &str) -> f64 {
    i64::from_str_radix(digits, 16).unwrap_or(0) as f64
}

/// `colord_parse_hex( $hex )`.
fn parse_hex(input: &str) -> Option<Rgba> {
    let hex = groups(&HEX, input, 1)?.remove(0);
    let b = hex.as_bytes();
    let doubled = |i: usize| hex_pair(&format!("{0}{0}", b[i] as char));
    if hex.len() <= 4 {
        return Some(Rgba {
            r: doubled(0),
            g: doubled(1),
            b: doubled(2),
            a: if hex.len() == 4 {
                round(doubled(3) / 255.0, 2)
            } else {
                1.0
            },
        });
    }
    if hex.len() == 6 || hex.len() == 8 {
        return Some(Rgba {
            r: hex_pair(&hex[0..2]),
            g: hex_pair(&hex[2..4]),
            b: hex_pair(&hex[4..6]),
            a: if hex.len() == 8 {
                round(hex_pair(&hex[6..8]) / 255.0, 2)
            } else {
                1.0
            },
        });
    }
    None
}

/// `colord_parse_rgba_string( $input )`.
fn parse_rgba(input: &str) -> Option<Rgba> {
    let m = groups(&RGBA_COMMAS, input, 8).or_else(|| groups(&RGBA_SPACES, input, 8))?;
    if m[1] != m[3] || m[3] != m[5] {
        return None;
    }
    let channel =
        |v: &str, pct: &str| float_prefix(v) / if pct.is_empty() { 1.0 } else { 100.0 / 255.0 };
    Some(Rgba {
        r: clamp(channel(&m[0], &m[1]), 0.0, 255.0),
        g: clamp(channel(&m[2], &m[3]), 0.0, 255.0),
        b: clamp(channel(&m[4], &m[5]), 0.0, 255.0),
        a: clamp(
            if m[6].is_empty() {
                1.0
            } else {
                float_prefix(&m[6]) / if m[7].is_empty() { 1.0 } else { 100.0 }
            },
            0.0,
            1.0,
        ),
    })
}

/// `colord_hsva_to_rgba( colord_hsla_to_hsva( $hsla ) )`.
fn hsla_to_rgba(h: f64, s: f64, l: f64, a: f64) -> Rgba {
    // `$s *= ( … ) / 100;`: the division first, as PHP evaluates it.
    let s = s * ((if l < 50.0 { l } else { 100.0 - l }) / 100.0);
    let (h, s, v) = (
        h,
        if s > 0.0 {
            ((2.0 * s) / (l + s)) * 100.0
        } else {
            0.0
        },
        l + s,
    );
    let h = (h / 360.0) * 6.0;
    let s = s / 100.0;
    let v = v / 100.0;
    let hh = h.floor();
    let b = v * (1.0 - s);
    let c = v * (1.0 - (h - hh) * s);
    let d = v * (1.0 - (1.0 - h + hh) * s);
    let module = (hh as i64 % 6) as usize;
    Rgba {
        r: [v, c, b, b, d, v][module] * 255.0,
        g: [d, v, v, c, b, b][module] * 255.0,
        b: [b, b, d, v, v, c][module] * 255.0,
        a,
    }
}

/// `colord_parse_hsla_string( $input )`.
fn parse_hsla(input: &str) -> Option<Rgba> {
    let m = groups(&HSLA_COMMAS, input, 6).or_else(|| groups(&HSLA_SPACES, input, 6))?;
    let h = clamp_hue(parse_hue(&m[0], &m[1]));
    let s = clamp(float_prefix(&m[2]), 0.0, 100.0);
    let l = clamp(float_prefix(&m[3]), 0.0, 100.0);
    let a = clamp(
        if m[4].is_empty() {
            1.0
        } else {
            float_prefix(&m[4]) / if m[5].is_empty() { 1.0 } else { 100.0 }
        },
        0.0,
        1.0,
    );
    Some(hsla_to_rgba(h, s, l, a))
}

/// `colord_parse( $input )`: hex, then `rgb()`, then `hsl()`; `None` for anything else.
pub fn colord_parse(input: &str) -> Option<Rgba> {
    parse_hex(input)
        .or_else(|| parse_rgba(input))
        .or_else(|| parse_hsla(input))
}
