//! The rule every corpus and fuzz comparison applies to a difference from PHP.

/// How one comparison with PHP came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Byte-identical.
    Same,
    /// Different, and allowed: PHP's output can run script and the port's cannot.
    SecurityBreak(String),
    /// Different, and not allowed.
    Fail(String),
}

/// Judges one output against PHP's. A difference passes only when `exploit` (the security oracle) finds an exploit
/// in PHP's output and none in the port's; with no oracle every difference fails.
pub fn gate(php: &[u8], port: &[u8], exploit: Option<&dyn Fn(&[u8]) -> Option<String>>) -> Verdict {
    if php == port {
        return Verdict::Same;
    }
    let shown = |b: &[u8]| {
        let t = String::from_utf8_lossy(b);
        t.chars().take(300).collect::<String>()
    };
    let detail = format!("php: {:?}\nport: {:?}", shown(php), shown(port));
    match exploit {
        Some(find) => match (find(php), find(port)) {
            (Some(why), None) => Verdict::SecurityBreak(why),
            (_, Some(port_exploit)) => Verdict::Fail(format!(
                "the port's output has an exploit: {port_exploit}\n{detail}"
            )),
            (None, None) => Verdict::Fail(detail),
        },
        None => Verdict::Fail(detail),
    }
}
