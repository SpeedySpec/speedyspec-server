//! The `autoload` column's values, and what a caller asked for.

use speedyspec_wp_hooks_contracts::HookValue;

/// The values `wp_autoload_values_to_autoload()` treats as autoloaded by default.
pub const DEFAULT_AUTOLOAD_VALUES: [&str; 4] = ["yes", "on", "auto-on", "auto"];

/// A stored `autoload` value. Unknown text is kept as it is, as the column keeps it.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Autoload {
    On,
    Off,
    Auto,
    AutoOn,
    AutoOff,
    Yes,
    No,
    Other(String),
}

impl Autoload {
    pub fn parse(text: &str) -> Autoload {
        match text {
            "on" => Autoload::On,
            "off" => Autoload::Off,
            "auto" => Autoload::Auto,
            "auto-on" => Autoload::AutoOn,
            "auto-off" => Autoload::AutoOff,
            "yes" => Autoload::Yes,
            "no" => Autoload::No,
            other => Autoload::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Autoload::On => "on",
            Autoload::Off => "off",
            Autoload::Auto => "auto",
            Autoload::AutoOn => "auto-on",
            Autoload::AutoOff => "auto-off",
            Autoload::Yes => "yes",
            Autoload::No => "no",
            Autoload::Other(text) => text,
        }
    }
}

/// The `$autoload` argument of `add_option` and `update_option`, before `wp_determine_option_autoload_value`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AutoloadRequest {
    /// `null`, or a value PHP 8's `switch` in `wp_determine_option_autoload_value` matches no case for.
    Default,
    Bool(bool),
    Text(String),
}

impl AutoloadRequest {
    pub fn from_value(value: &HookValue) -> AutoloadRequest {
        match value {
            HookValue::Bool(b) => AutoloadRequest::Bool(*b),
            HookValue::Str(text) => AutoloadRequest::Text(text.clone()),
            HookValue::Bytes(bytes) => {
                AutoloadRequest::Text(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => AutoloadRequest::Default,
        }
    }
}
