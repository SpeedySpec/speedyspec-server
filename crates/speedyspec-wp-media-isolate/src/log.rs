//! One JSON object per line on stderr, as the runtime's workers log.

use std::io::Write;

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Logs `name` with `fields`, for example `{"event":"media_probe_timed_out","path":"…","elapsed_ms":"5250"}`.
pub fn event(name: &str, fields: &[(&str, &str)]) {
    let mut line = format!("{{\"event\":\"{}\"", escape(name));
    for (key, value) in fields {
        line.push_str(&format!(",\"{}\":\"{}\"", escape(key), escape(value)));
    }
    line.push('}');
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}
