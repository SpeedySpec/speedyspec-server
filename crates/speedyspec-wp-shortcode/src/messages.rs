//! Every Shortcode API function as a use case on the buses (Command / CQRS), handled by [`ShortcodeApi`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler};
use speedyspec_wp_shortcode_contracts::{ParsedAtts, PhpValue, ShortcodeError, ShortcodeHandler, TagSet};

use crate::ShortcodeApi;

macro_rules! message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[doc = concat!("The `", stringify!($name), "` use case.")]
        #[derive(Clone)]
        pub struct $name { $(pub $field: $ty),* }
        message!(@impl $kind $name $result);
        message!(@handle $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpShortcode\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpShortcode\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for ShortcodeApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for ShortcodeApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, ShortcodeError>;

message!(command AddShortcode { tag: Vec<u8>, handler: Rc<dyn ShortcodeHandler> } -> (),
    |a, m| a.add_shortcode(&m.tag, m.handler));
message!(command RemoveShortcode { tag: Vec<u8> } -> (), |a, m| a.remove_shortcode(&m.tag));
message!(command RemoveAllShortcodes {} -> (), |a, m| a.remove_all_shortcodes());
message!(query ShortcodeExists { tag: Vec<u8> } -> bool, |a, m| a.shortcode_exists(&m.tag));
message!(query HasShortcode { content: Vec<u8>, tag: Vec<u8> } -> R<bool>, |a, m| a.has_shortcode(&m.content, &m.tag));
message!(query GetShortcodeTagsInContent { content: Vec<u8> } -> R<Vec<Vec<u8>>>,
    |a, m| a.get_shortcode_tags_in_content(&m.content));
message!(query ApplyShortcodes { content: Vec<u8>, ignore_html: bool } -> R<Vec<u8>>,
    |a, m| a.apply_shortcodes(&m.content, m.ignore_html));
message!(query DoShortcode { content: Vec<u8>, ignore_html: bool } -> R<Vec<u8>>,
    |a, m| a.do_shortcode(&m.content, m.ignore_html));
message!(query GetShortcodeRegex { tagnames: Option<Vec<Vec<u8>>> } -> Vec<u8>,
    |a, m| a.get_shortcode_regex(m.tagnames.as_deref()));
message!(query DoShortcodeTag { m: PhpValue } -> R<PhpValue>, |a, m| a.do_shortcode_tag(&m.m));
message!(query DoShortcodesInHtmlTags { content: Vec<u8>, ignore_html: bool, tags: TagSet } -> R<Vec<u8>>,
    |a, m| a.do_shortcodes_in_html_tags(&m.content, m.ignore_html, &m.tags));
message!(query UnescapeInvalidShortcodes { content: Vec<u8> } -> Vec<u8>, |a, m| a.unescape_invalid_shortcodes(&m.content));
message!(query ShortcodeParseAtts { text: Vec<u8> } -> R<ParsedAtts>, |a, m| a.shortcode_parse_atts(&m.text));
message!(query ShortcodeAtts { pairs: PhpValue, atts: PhpValue, shortcode: Vec<u8> } -> R<PhpValue>,
    |a, m| a.shortcode_atts(&m.pairs, &m.atts, &m.shortcode));
message!(query StripShortcodes { content: Vec<u8> } -> R<Vec<u8>>, |a, m| a.strip_shortcodes(&m.content));
message!(query StripShortcodeTag { m: PhpValue } -> R<PhpValue>, |a, m| a.strip_shortcode_tag(&m.m));

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every shortcode message; PHP overrides one by binding the same key.
pub fn register_shortcode_handlers(container: &Container, api: Rc<ShortcodeApi>) {
    bind_all!(container, api;
        commands: AddShortcode, RemoveShortcode, RemoveAllShortcodes;
        queries: ShortcodeExists, HasShortcode, GetShortcodeTagsInContent, ApplyShortcodes, DoShortcode,
            GetShortcodeRegex, DoShortcodeTag, DoShortcodesInHtmlTags, UnescapeInvalidShortcodes, ShortcodeParseAtts,
            ShortcodeAtts, StripShortcodes, StripShortcodeTag);
}
