use std::borrow::Cow;

/// One parsed attribute, as `wp_kses_hair()` returns it; borrowed from the input unless kses changed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr<'a> {
    pub name: Cow<'a, [u8]>,
    pub value: Cow<'a, [u8]>,
    pub whole: Cow<'a, [u8]>,
    /// PHP's `vless`: true for `y` (written without a value).
    pub vless: bool,
}
