//! [`Lexical`]: this crate's functions behind the [`KsesLexical`] port.

use std::borrow::Cow;

use speedyspec_wp_kses_contracts::{
    AllowedHtml, Attr, AttrRule, EntityContext, EntityNames, KsesError, KsesLexical, Protocols,
    UriAttributes,
};

/// The kses string functions as the [`KsesLexical`] port; holds nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lexical;

impl KsesLexical for Lexical {
    fn no_null<'a>(&self, content: &'a [u8], keep_slash_zero: bool) -> Cow<'a, [u8]> {
        crate::no_null(content, keep_slash_zero)
    }
    fn stripslashes<'a>(&self, content: &'a [u8]) -> Cow<'a, [u8]> {
        crate::stripslashes(content)
    }
    fn normalize_entities<'a>(
        &self,
        content: &'a [u8],
        ctx: EntityContext,
        html_names: &EntityNames,
        xml_names: &EntityNames,
    ) -> Cow<'a, [u8]> {
        crate::normalize_entities(content, ctx, html_names, xml_names)
    }
    fn decode_entities<'a>(&self, content: &'a [u8]) -> Cow<'a, [u8]> {
        crate::decode_entities(content)
    }
    fn bad_protocol<'a>(&self, content: &'a [u8], protocols: &Protocols) -> Cow<'a, [u8]> {
        crate::bad_protocol(content, protocols)
    }
    fn hair<'a>(
        &self,
        attr: &'a [u8],
        protocols: &Protocols,
        uris: &UriAttributes,
    ) -> Vec<Attr<'a>> {
        crate::hair(attr, protocols, uris)
    }
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        crate::attr_parse(element)
    }
    fn hair_parse<'a>(&self, attr: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        crate::hair_parse(attr)
    }
    fn html_error<'a>(&self, attr: &'a [u8]) -> &'a [u8] {
        crate::html_error(attr)
    }
    fn array_lc(&self, allowed: &AllowedHtml) -> AllowedHtml {
        crate::array_lc(allowed)
    }
    fn check_attr_val(
        &self,
        value: &[u8],
        vless: bool,
        rule: &AttrRule,
    ) -> Result<bool, KsesError> {
        crate::check_attr_val(value, vless, rule)
    }
}
