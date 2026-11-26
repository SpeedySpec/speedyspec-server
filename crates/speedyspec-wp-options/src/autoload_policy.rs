//! How an option's `autoload` column is decided, and which values autoload (Strategy), as `option.php` does.

use std::rc::{Rc, Weak};

use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_hooks_contracts::{Callback, Count, HookName, HookValue, Key};

use crate::{
    Autoload, AutoloadRequest, DEFAULT_AUTOLOAD_VALUES, OptionsError, php_empty, php_string,
};

/// The `autoload` decision and the list of values `wp_load_alloptions()` loads.
pub trait AutoloadPolicy {
    /// `wp_determine_option_autoload_value()`.
    fn determine(
        &self,
        name: &str,
        value: &HookValue,
        serialized: &HookValue,
        requested: &AutoloadRequest,
    ) -> Result<Autoload, OptionsError>;

    /// `wp_autoload_values_to_autoload()`: the filtered list, keys kept.
    fn values_to_autoload(&self) -> Result<HookValue, OptionsError>;
}

/// WordPress's policy, with its two filters.
pub struct WordPressAutoloadPolicy {
    hooks: Rc<HookApi>,
}

impl WordPressAutoloadPolicy {
    pub fn new(hooks: Rc<HookApi>) -> WordPressAutoloadPolicy {
        WordPressAutoloadPolicy { hooks }
    }
}

fn filter(
    hooks: &HookApi,
    name: &str,
    value: HookValue,
    args: Vec<HookValue>,
) -> Result<HookValue, OptionsError> {
    let hook = HookName::from(name);
    if hooks.skip_if_empty(&hook, Count::Filters)? {
        return Ok(value);
    }
    Ok(hooks.apply_filters(&hook, value, args)?.read().into_owned())
}

impl AutoloadPolicy for WordPressAutoloadPolicy {
    fn determine(
        &self,
        name: &str,
        value: &HookValue,
        serialized: &HookValue,
        requested: &AutoloadRequest,
    ) -> Result<Autoload, OptionsError> {
        match requested {
            AutoloadRequest::Bool(true) => return Ok(Autoload::On),
            AutoloadRequest::Bool(false) => return Ok(Autoload::Off),
            AutoloadRequest::Text(t) if t == "on" || t == "yes" => return Ok(Autoload::On),
            AutoloadRequest::Text(t) if t == "off" || t == "no" => return Ok(Autoload::Off),
            _ => {}
        }
        let answer = filter(
            &self.hooks,
            "wp_default_autoload_value",
            HookValue::Null,
            vec![
                HookValue::Str(name.to_string()),
                value.clone(),
                serialized.clone(),
            ],
        )?;
        Ok(match answer {
            HookValue::Bool(true) => Autoload::AutoOn,
            HookValue::Bool(false) => Autoload::AutoOff,
            _ => Autoload::Auto,
        })
    }

    fn values_to_autoload(&self) -> Result<HookValue, OptionsError> {
        let defaults = HookValue::Array(
            DEFAULT_AUTOLOAD_VALUES
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), HookValue::Str((*v).to_string())))
                .collect(),
        );
        let filtered = filter(
            &self.hooks,
            "wp_autoload_values_to_autoload",
            defaults,
            Vec::new(),
        )?;
        let HookValue::Array(items) = filtered else {
            // PHP's array_intersect() throws a TypeError for a non-array.
            return Err(OptionsError::Serialize(
                "wp_autoload_values_to_autoload must return an array".into(),
            ));
        };
        // array_intersect(): the filtered entries whose string form is a default value, keys kept.
        Ok(HookValue::Array(
            items
                .into_iter()
                .filter(|(_, v)| {
                    php_string(v).is_some_and(|text| {
                        DEFAULT_AUTOLOAD_VALUES.iter().any(|d| d.as_bytes() == text)
                    })
                })
                .collect(),
        ))
    }
}

/// The values of an autoload list, as strings.
pub fn autoload_values(values: &HookValue) -> Vec<String> {
    match values {
        HookValue::Array(items) => items
            .iter()
            .filter_map(|(_, v)| php_string(v))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// PHP's `(int)` cast.
fn php_int(value: &HookValue) -> i64 {
    match value {
        HookValue::Int(i) => *i,
        HookValue::Bool(b) => i64::from(*b),
        HookValue::Float(f) if f.is_finite() => *f as i64,
        HookValue::Str(_) | HookValue::Bytes(_) => {
            let bytes = php_string(value).unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
            // The longest numeric prefix: an integer, or a float that is truncated.
            let mut end = 0;
            let b = text.as_bytes();
            if end < b.len() && (b[end] == b'+' || b[end] == b'-') {
                end += 1;
            }
            let digits_start = end;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            let mut float_end = end;
            if float_end < b.len() && b[float_end] == b'.' {
                float_end += 1;
                while float_end < b.len() && b[float_end].is_ascii_digit() {
                    float_end += 1;
                }
            }
            if float_end > digits_start
                && float_end < b.len()
                && (b[float_end] == b'e' || b[float_end] == b'E')
            {
                let mut e = float_end + 1;
                if e < b.len() && (b[e] == b'+' || b[e] == b'-') {
                    e += 1;
                }
                if e < b.len() && b[e].is_ascii_digit() {
                    while e < b.len() && b[e].is_ascii_digit() {
                        e += 1;
                    }
                    float_end = e;
                }
            }
            if float_end > end {
                return text[..float_end]
                    .parse::<f64>()
                    .map(|f| f as i64)
                    .unwrap_or(0);
            }
            text[..end].parse::<i64>().unwrap_or_else(|_| {
                if end > digits_start {
                    if text.starts_with('-') {
                        i64::MIN
                    } else {
                        i64::MAX
                    }
                } else {
                    0
                }
            })
        }
        _ => 0,
    }
}

/// `wp_filter_default_autoload_value_via_option_size()`: a value larger than `wp_max_autoloaded_option_size`
/// (150000 bytes by default) is not autoloaded by default.
pub fn filter_default_autoload_value_via_option_size(
    hooks: &HookApi,
    autoload: HookValue,
    name: &str,
    serialized: &HookValue,
) -> Result<HookValue, OptionsError> {
    let max = php_int(&filter(
        hooks,
        "wp_max_autoloaded_option_size",
        HookValue::Int(150000),
        vec![HookValue::Str(name.to_string())],
    )?);
    let size = if php_empty(serialized) {
        0
    } else {
        php_string(serialized).map_or(0, |b| b.len() as i64)
    };
    Ok(if size > max {
        HookValue::Bool(false)
    } else {
        autoload
    })
}

/// The filter `default-filters.php` adds, for hosts with no PHP: the same id, so a PHP host's own `add_filter`
/// replaces it. The callback holds the Hook API weakly: the store that holds the callback must not keep the API.
pub fn register_default_filters(hooks: &Rc<HookApi>) -> Result<(), OptionsError> {
    let api: Weak<HookApi> = Rc::downgrade(hooks);
    let id = "wp_filter_default_autoload_value_via_option_size";
    let callback = Callback::rust(id, move |args: &[HookValue]| {
        let autoload = args.first().cloned().unwrap_or(HookValue::Null);
        let Some(api) = api.upgrade() else {
            return Ok(autoload);
        };
        let name = args.get(1).and_then(php_string).unwrap_or_default();
        let serialized = args.get(3).cloned().unwrap_or(HookValue::Null);
        filter_default_autoload_value_via_option_size(
            &api,
            autoload,
            &String::from_utf8_lossy(&name),
            &serialized,
        )
        .map_err(|e| match e {
            OptionsError::Hook(h) => h,
            other => speedyspec_wp_hooks_contracts::HookError::message(other.to_string()),
        })
    });
    hooks.add_filter(
        &HookName::from("wp_default_autoload_value"),
        callback,
        &HookValue::Int(5),
        4,
    )?;
    Ok(())
}
