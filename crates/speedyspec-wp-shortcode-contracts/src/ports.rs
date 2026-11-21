use std::rc::Rc;

use crate::{AttsParse, ParsedAtts, PhpKey, PhpValue, ShortcodeError, ShortcodeMatch, TagName, TagSet};

/// One registered tag's callback: `call_user_func( $callback, $attr, $content, $tag )`.
pub trait ShortcodeHandler {
    /// Runs the handler. `content` is `None` only when a direct caller of `do_shortcode_tag()` passed no `$m[5]`.
    ///
    /// # Errors
    ///
    /// [`ShortcodeError::Handler`] when the handler fails or throws.
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, tag: &[u8]) -> Result<PhpValue, ShortcodeError>;

    /// `is_callable()` at call time. Rust handlers are always callable.
    fn is_callable(&self) -> bool {
        true
    }

    /// The PHP value `$shortcode_tags` holds for this handler: a PHP callable, or the name of the native function a
    /// Rust handler is declared as. `None` for a handler PHP cannot see, which only an in-memory registry keeps.
    fn php_callable(&self) -> Option<PhpValue> {
        None
    }
}

/// The read-only view of the registered names that other areas depend on (`wpautop`, `shortcode_unautop`,
/// `wptexturize`).
pub trait ShortcodeTagNames {
    /// The names in registration order. Re-adding a tag keeps its place; removing then adding moves it to the end.
    fn tag_names(&self) -> Vec<TagName>;

    /// Changes whenever the set or the order of names changes, so callers can cache automata.
    fn generation(&self) -> u64;
}

/// `$shortcode_tags`: the Registry of shortcode handlers.
///
/// No method may run a handler or hold a borrow past its return, because handlers re-enter.
pub trait ShortcodeRegistry: ShortcodeTagNames {
    /// `$shortcode_tags[ $tag ] = $callback`.
    fn add(&self, tag: TagName, handler: Rc<dyn ShortcodeHandler>);
    /// `unset( $shortcode_tags[ $tag ] )`.
    fn remove(&self, tag: &[u8]);
    /// `$shortcode_tags = array()`.
    fn remove_all(&self);
    /// `array_key_exists( $tag, $shortcode_tags )`.
    fn exists(&self, tag: &[u8]) -> bool;
    /// The handler registered for `tag`.
    fn handler(&self, tag: &[u8]) -> Option<Rc<dyn ShortcodeHandler>>;
}

/// The matches one scan yields, left to right and without overlap.
pub type ShortcodeMatches<'h> = Box<dyn Iterator<Item = Result<ShortcodeMatch, ShortcodeError>> + 'h>;

/// A Strategy for finding matches of `get_shortcode_regex( $tagnames )`, as `preg_replace_callback()` finds them.
pub trait ShortcodeScanner {
    /// Scans `haystack` lazily. The iterator holds no borrow of the scanner, so handlers may re-enter it.
    fn scan<'h>(&self, haystack: &'h [u8], tags: &TagSet) -> ShortcodeMatches<'h>;
}

/// A Strategy for `shortcode_parse_atts()`.
pub trait AttsParser {
    /// Parses the inside of a shortcode tag.
    fn parse(&self, text: &[u8]) -> AttsParse;
}

/// Proof that `do_shortcode()` entered the image context, carried to the matching leave.
#[derive(Debug)]
pub struct ContextGuard {
    added: bool,
}

impl ContextGuard {
    /// Records whether this call added the filter.
    pub fn new(added: bool) -> Self {
        Self { added }
    }

    /// True when this call added the filter, so its leave must remove it.
    pub fn added(&self) -> bool {
        self.added
    }
}

/// The filters the Shortcode API applies, named for WordPress's hooks.
pub trait ShortcodeFilters {
    /// `apply_filters( 'pre_do_shortcode_tag', false, $tag, $attr, $m )`.
    ///
    /// # Errors
    ///
    /// [`ShortcodeError::Filter`] when a callback fails or throws; so for every method here.
    fn pre_do_shortcode_tag(&self, tag: &[u8], attr: &PhpValue, m: &PhpValue) -> Result<PhpValue, ShortcodeError>;
    /// `apply_filters( 'do_shortcode_tag', $output, $tag, $attr, $m )`.
    fn do_shortcode_tag(
        &self,
        output: PhpValue,
        tag: &[u8],
        attr: &PhpValue,
        m: &PhpValue,
    ) -> Result<PhpValue, ShortcodeError>;
    /// `apply_filters( "shortcode_atts_{$shortcode}", $out, $pairs, $atts, $shortcode )`.
    fn shortcode_atts(
        &self,
        shortcode: &[u8],
        out: PhpValue,
        pairs: &PhpValue,
        atts: &PhpValue,
    ) -> Result<PhpValue, ShortcodeError>;
    /// `apply_filters( 'strip_shortcodes_tagnames', $tags_to_remove, $content )`.
    fn strip_shortcodes_tagnames(&self, tags: PhpValue, content: &[u8]) -> Result<PhpValue, ShortcodeError>;
    /// Adds `_filter_do_shortcode_context` to `wp_get_attachment_image_context` unless it is already there.
    fn enter_do_shortcode_context(&self) -> Result<ContextGuard, ShortcodeError>;
    /// Removes the filter if `guard` added it. Not called when the scan fails, as WordPress leaves it attached.
    fn leave_do_shortcode_context(&self, guard: ContextGuard) -> Result<(), ShortcodeError>;
}

/// A message for `_doing_it_wrong()`: WordPress's untranslated text and its `sprintf` arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticMessage {
    pub template: &'static str,
    pub args: Vec<Vec<u8>>,
}

/// WordPress's developer notices.
pub trait Diagnostics {
    /// `_doing_it_wrong( $function, $message, $version )`.
    fn doing_it_wrong(&self, function: &str, message: DiagnosticMessage, version: &str);
    /// PHP's E_DEPRECATED for passing `null` as `$subject` to `function`.
    fn deprecated_null_subject(&self, function: &str);
    /// PHP's E_WARNING for reading a missing array key, where WordPress reads one without `??`.
    fn undefined_array_key(&self, key: &PhpKey);
}

/// `wp_html_split()`.
pub trait HtmlSplitter {
    /// Splits content into text and HTML elements, as WordPress's split regex does.
    fn split<'a>(&self, content: &'a [u8]) -> Vec<&'a [u8]>;
}

/// The kses functions `do_shortcodes_in_html_tags()` calls, bytes-first like kses's own contracts.
pub trait KsesAttributes {
    /// `wp_kses_attr_parse( $element )`: the element's front, attributes and back, or `None` for `false`.
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>>;
    /// `wp_kses_one_attr( $attr, $element )`.
    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Vec<u8>;
}

/// PHP's string conversion of a value, as concatenation does it.
pub trait PhpStrings {
    /// Converts `value` to bytes as PHP's `(string)` does.
    ///
    /// # Errors
    ///
    /// [`ShortcodeError::Conversion`] where PHP throws (an object without `__toString`).
    fn to_php_string(&self, value: &PhpValue) -> Result<Vec<u8>, ShortcodeError>;
}
