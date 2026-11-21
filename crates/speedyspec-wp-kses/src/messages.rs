//! Every kses function as a use case on the buses (Command / Query), handled by [`KsesApi`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_hooks::Value;
use speedyspec_wp_kses_contracts::{
    AllowedHtml, AllowedSpec, Context, KsesError, Protocols, UriAttributes,
};

use crate::CheckedAttr;

use crate::KsesApi;

macro_rules! message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        message!(@impl $kind $name $result);
        message!(@handle $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpKses\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpKses\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for KsesApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for KsesApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, KsesError>;

message!(query KsesQuery { content: Vec<u8>, allowed: AllowedSpec, protocols: Option<Protocols> } -> R<Vec<u8>>,
    |a, m| a.kses(&m.content, &m.allowed, m.protocols.as_ref()));
message!(query KsesPost { data: Vec<u8> } -> R<Vec<u8>>, |a, m| a.post(&m.data));
message!(query KsesData { data: Vec<u8> } -> R<Vec<u8>>, |a, m| a.data(&m.data));
message!(query FilterKses { data: Vec<u8> } -> R<Vec<u8>>, |a, m| a.filter_kses(&m.data));
message!(query FilterPostKses { data: Vec<u8> } -> R<Vec<u8>>, |a, m| a.filter_post_kses(&m.data));
message!(query FilterNohtmlKses { data: Vec<u8> } -> R<Vec<u8>>, |a, m| a.filter_nohtml_kses(&m.data));
message!(query OneAttr { attr: Vec<u8>, element: Vec<u8> } -> R<Vec<u8>>, |a, m| a.one_attr(&m.attr, &m.element));
message!(query AllowedHtmlQuery { context: Context } -> R<Rc<AllowedHtml>>, |a, m| a.allowed_html(&m.context));
message!(query UriAttributesQuery {} -> R<Rc<UriAttributes>>, |a, m| a.uri_attributes());
message!(query AllowedProtocolsQuery {} -> R<Rc<Protocols>>, |a, m| a.allowed_protocols());
message!(query SafecssFilterAttr { css: Vec<u8> } -> R<Vec<u8>>, |a, m| a.safecss_filter_attr(&m.css));
message!(query AllowedHtmlValue { context: Context } -> R<Value>, |a, m| a.allowed_html_value(&m.context));
message!(query AllowedHtmlExplicitValue { allowed: Value } -> R<Value>, |a, m| a.allowed_html_explicit_value(&m.allowed));
message!(query AllowedFrom { allowed: Value } -> R<Rc<AllowedHtml>>, |a, m| a.allowed_from(&m.allowed));
message!(query KsesHook { content: Vec<u8>, allowed: AllowedSpec, protocols: Protocols } -> R<Vec<u8>>,
    |a, m| a.kses_hook(&m.content, &m.allowed, &m.protocols));
message!(query KsesSplit { content: Vec<u8>, allowed: AllowedSpec, protocols: Protocols } -> R<Vec<u8>>,
    |a, m| a.split(&m.content, &m.allowed, &m.protocols));
message!(query KsesSplit2 { content: Vec<u8>, allowed: AllowedSpec, protocols: Protocols } -> R<Vec<u8>>,
    |a, m| a.split2(&m.content, &m.allowed, &m.protocols));
message!(query KsesAttr { element: Vec<u8>, attr: Vec<u8>, allowed: AllowedSpec, protocols: Protocols } -> R<Vec<u8>>,
    |a, m| a.attr(&m.element, &m.attr, &m.allowed, &m.protocols));
message!(query KsesAttrCheck {
    name: Vec<u8>, value: Vec<u8>, whole: Vec<u8>, vless: bool, element: Vec<u8>, allowed: Rc<AllowedHtml>
} -> R<Option<CheckedAttr>>, |a, m| a.attr_check(&m.name, &m.value, &m.whole, m.vless, &m.element, &m.allowed));
message!(command KsesInitFilters {} -> R<()>, |a, m| a.kses_init_filters());
message!(command KsesRemoveFilters {} -> R<()>, |a, m| a.kses_remove_filters());
message!(command KsesInit {} -> R<()>, |a, m| a.kses_init());

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds every kses use case to `api` in the container.
pub fn register_kses_handlers(container: &Container, api: Rc<KsesApi>) {
    bind_all!(container, api;
        commands: KsesInitFilters, KsesRemoveFilters, KsesInit;
        queries: KsesQuery, KsesPost, KsesData, FilterKses, FilterPostKses, FilterNohtmlKses, OneAttr, AllowedHtmlQuery,
            UriAttributesQuery, AllowedProtocolsQuery, SafecssFilterAttr, AllowedHtmlValue, AllowedHtmlExplicitValue,
            AllowedFrom, KsesHook, KsesSplit, KsesSplit2, KsesAttr, KsesAttrCheck);
}
