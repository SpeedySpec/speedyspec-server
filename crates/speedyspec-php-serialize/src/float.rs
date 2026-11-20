//! Floats as PHP writes them with `serialize_precision = -1`: the shortest digits that read back the same.

/// `1.0E+25`, `0.1`, `100000000000000`, `-0`, `INF`, `-INF`, `NAN`.
pub fn php_float(f: f64) -> String {
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
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if f < 0.0 { "-" } else { "" };
    let n = digits.len() as i32;
    if (-4..15).contains(&exp) {
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
