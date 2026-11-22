use crate::Value;

/// The site's merged settings (`wp_get_global_settings()`), which fluid typography merges under the settings it
/// is given.
pub trait GlobalSettings {
    /// The settings array.
    fn settings(&self) -> Value;
}
