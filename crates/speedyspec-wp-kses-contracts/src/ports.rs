//! The ports: what a kses implementation offers (inbound) and what it needs from code not yet ported (outbound).

use std::borrow::Cow;
use std::rc::Rc;

use crate::{
    AllowedHtml, AllowedSpec, Attr, AttrRule, Context, EntityContext, EntityNames, KsesError,
    Protocols, UriAttributes,
};

/// kses.php's pure string functions, which need no hooks.
pub trait KsesLexical {
    fn no_null<'a>(&self, content: &'a [u8], keep_slash_zero: bool) -> Cow<'a, [u8]>;
    fn stripslashes<'a>(&self, content: &'a [u8]) -> Cow<'a, [u8]>;
    fn normalize_entities<'a>(
        &self,
        content: &'a [u8],
        ctx: EntityContext,
        html_names: &EntityNames,
        xml_names: &EntityNames,
    ) -> Cow<'a, [u8]>;
    fn decode_entities<'a>(&self, content: &'a [u8]) -> Cow<'a, [u8]>;
    fn bad_protocol<'a>(&self, content: &'a [u8], protocols: &Protocols) -> Cow<'a, [u8]>;
    fn hair<'a>(
        &self,
        attr: &'a [u8],
        protocols: &Protocols,
        uris: &UriAttributes,
    ) -> Vec<Attr<'a>>;
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>>;
    fn hair_parse<'a>(&self, attr: &'a [u8]) -> Option<Vec<&'a [u8]>>;
    fn html_error<'a>(&self, attr: &'a [u8]) -> &'a [u8];
    fn array_lc(&self, allowed: &AllowedHtml) -> AllowedHtml;
    fn check_attr_val(&self, value: &[u8], vless: bool, rule: &AttrRule)
    -> Result<bool, KsesError>;
}

/// The sanitizer itself.
pub trait Kses {
    /// `wp_kses()`; `None` or an empty list means `wp_allowed_protocols()`.
    fn kses(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: Option<&Protocols>,
    ) -> Result<Vec<u8>, KsesError>;
    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Result<Vec<u8>, KsesError>;
    fn post(&self, data: &[u8]) -> Result<Vec<u8>, KsesError>;
    fn data(&self, data: &[u8]) -> Result<Vec<u8>, KsesError>;
}

/// The named allowlists and URI attribute list, each through its filter.
pub trait AllowedHtmlSource {
    fn allowed_html(&self, ctx: &Context) -> Result<Rc<AllowedHtml>, KsesError>;
    fn uri_attributes(&self) -> Result<Rc<UriAttributes>, KsesError>;
}

/// `safecss_filter_attr()`.
pub trait SafeCss {
    fn safecss_filter_attr(&self, css: &[u8]) -> Result<Vec<u8>, KsesError>;
}

/// `esc_attr()`, owned by the text port, which fires `attribute_escape`.
pub trait AttributeEscaper {
    fn esc_attr(&self, value: &[u8]) -> Result<Vec<u8>, KsesError>;
}

/// `current_user_can( 'unfiltered_html' )`, owned by the identity port.
pub trait UnfilteredHtmlPolicy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError>;
}
