//! The `KsesAttributes` port onto the Rust kses port (Adapter).

use std::rc::Rc;

use speedyspec_wp_kses_contracts::{Kses, KsesError};
use speedyspec_wp_shortcode_contracts::{KsesAttributes, ShortcodeError, Thrown};

/// `wp_kses_attr_parse()` and `wp_kses_one_attr()` in Rust.
///
/// `wp_kses_attr_parse()` needs no hooks, so it is `speedyspec-wp-kses-lexical`'s directly. `wp_kses_one_attr()`
/// reads the allowlists and protocols through their filters, so it goes to the `Kses` given, which at runtime is the
/// kses area's own API: the one PHP's `wp_kses_one_attr()` runs, reading the same globals.
pub struct RustKsesAttributes {
    kses: Rc<dyn Kses>,
}

impl RustKsesAttributes {
    /// The adapter over `kses`, which answers `wp_kses_one_attr()`.
    pub fn new(kses: Rc<dyn Kses>) -> Self {
        Self { kses }
    }
}

impl KsesAttributes for RustKsesAttributes {
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        speedyspec_wp_kses_lexical::attr_parse(element)
    }

    /// A foreign exception from a kses filter is carried through unchanged, so `do_shortcode()` rethrows it.
    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Result<Vec<u8>, ShortcodeError> {
        self.kses.one_attr(attr, element).map_err(|err| match err {
            KsesError::Foreign { message, payload } => ShortcodeError::Thrown(Thrown { message, payload: payload.0 }),
            other => ShortcodeError::Filter(other.to_string()),
        })
    }
}
