//! Every `EscapeApi` function as a query on the bus (Command / Query), bound in the container, as kses does.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Query, QueryHandler, bind_query_handler};
use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_text_contracts::TextError;

use crate::api::EscapeApi;
use crate::decode::QuoteStyle;

macro_rules! message {
    ($name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[doc = concat!("The `", stringify!($name), "` use case: one formatting function as a query, so a container binding can")]
        #[doc = "replace its handler."]
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpTextEscape\\", stringify!($name), "Handler");
        }
        impl QueryHandler<$name> for EscapeApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, TextError>;

message!(WpSpecialchars { text: Vec<u8>, style: QuoteStyle, charset: Option<String>, double_encode: bool } -> R<Vec<u8>>,
    |a, m| a.specialchars(&m.text, m.style, m.charset.as_deref(), m.double_encode));
message!(WpCheckInvalidUtf8 { text: Vec<u8>, strip: bool } -> R<Vec<u8>>, |a, m| a.check_invalid_utf8(&m.text, m.strip));
message!(EscHtml { text: Vec<u8> } -> R<HookValue>, |a, m| a.esc_html(&m.text));
message!(EscAttr { text: Vec<u8> } -> R<HookValue>, |a, m| a.esc_attr(&m.text));
message!(EscTextarea { text: Vec<u8> } -> R<HookValue>, |a, m| a.esc_textarea(&m.text));
message!(EscXml { text: Vec<u8> } -> R<HookValue>, |a, m| a.esc_xml(&m.text));
message!(EscJs { text: Vec<u8> } -> R<HookValue>, |a, m| a.esc_js(&m.text));
message!(TagEscape { text: Vec<u8> } -> R<HookValue>, |a, m| a.tag_escape(&m.text));
message!(SeemsUtf8 { text: Vec<u8> } -> R<bool>, |a, m| a.seems_utf8(&m.text));
message!(AddslashesGpc { value: HookValue } -> R<HookValue>, |a, m| a.addslashes_gpc(&m.value));

macro_rules! bind_all {
    ($container:expr, $api:expr; $($q:ty),*) => {
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds every escaping use case of `EscapeApi` to `api` in the container.
///
/// @param container
///     Where to bind them.
/// @param api
///     The handler of each.
pub fn register_escape_handlers(container: &Container, api: Rc<EscapeApi>) {
    bind_all!(container, api;
        WpSpecialchars, WpCheckInvalidUtf8, EscHtml, EscAttr, EscTextarea, EscXml, EscJs, TagEscape, SeemsUtf8,
        AddslashesGpc);
}
