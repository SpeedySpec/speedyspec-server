//! Option names, trimmed and checked as `option.php` does before any hook fires.

use std::fmt;

/// PHP's `trim()` default set.
const TRIM: &[char] = &[' ', '\t', '\n', '\r', '\0', '\x0B'];

/// An option's name: trimmed, never empty and never `"0"` (PHP's `empty()` rejects both).
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct OptionName(String);

impl OptionName {
    /// The trimmed name; `None` where WordPress returns false before any hook.
    pub fn new(raw: &str) -> Option<OptionName> {
        let name = raw.trim_matches(TRIM);
        (!name.is_empty() && name != "0").then(|| OptionName(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name that replaced a deprecated one; reading the old name raises a deprecation and reads the new.
    pub fn deprecated_replacement(&self) -> Option<&'static str> {
        match self.0.as_str() {
            "blacklist_keys" => Some("disallowed_keys"),
            "comment_whitelist" => Some("comment_previously_approved"),
            _ => None,
        }
    }
}

impl fmt::Display for OptionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
