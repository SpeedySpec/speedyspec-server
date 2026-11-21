/// `wp_sanitize_redirect( $location )`: spaces become `%20`, UTF-8 characters are percent-encoded, every other
/// character outside `a-z A-Z 0-9 - ~ + _ . ? # = & ; , / : % ! * [ ] ( ) @` is dropped, and `%0d`/`%0a` (either
/// case) are removed until none is left (`_deep_replace`).
///
/// `wp_kses_no_null()` runs between those steps in WordPress; it changes nothing here, as the character filter has
/// already dropped every control character and backslash it would remove.
pub fn wp_sanitize_redirect(location: &str) -> String {
    let mut out = String::with_capacity(location.len());
    for c in location.chars() {
        if c == ' ' {
            out.push_str("%20");
        } else if !c.is_ascii() {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else if c.is_ascii_alphanumeric() || "-~+_.?#=&;,/:%!*[]()@".contains(c) {
            out.push(c);
        }
    }
    loop {
        let mut changed = false;
        for strip in ["%0d", "%0a", "%0D", "%0A"] {
            if out.contains(strip) {
                out = out.replace(strip, "");
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}
