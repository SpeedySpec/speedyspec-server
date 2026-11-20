//! The site and network options functions as use cases on the buses (Command / CQRS), handled by
//! [`NetworkOptionsApi`]. The `*_site_option` functions send the same messages with `network_id: HookValue::Null`.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_wp_hooks::HookValue;

use crate::{NetworkOptionsApi, OptionsError};

macro_rules! network_message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        network_message!(@impl $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for NetworkOptionsApi {
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@impl query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
        impl QueryHandler<$name> for NetworkOptionsApi {
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, OptionsError>;

network_message!(query GetNetworkOption { network_id: HookValue, option: String, default: HookValue } -> R<HookValue>,
    |a, m| a.get_network_option(&m.network_id, &m.option, m.default));
network_message!(command AddNetworkOption { network_id: HookValue, option: String, value: HookValue } -> R<bool>,
    |a, m| a.add_network_option(&m.network_id, &m.option, m.value));
network_message!(command UpdateNetworkOption { network_id: HookValue, option: String, value: HookValue } -> R<bool>,
    |a, m| a.update_network_option(&m.network_id, &m.option, m.value));
network_message!(command DeleteNetworkOption { network_id: HookValue, option: String } -> R<bool>,
    |a, m| a.delete_network_option(&m.network_id, &m.option));
network_message!(command PrimeNetworkOptionCaches { network_id: HookValue, options: Vec<String> } -> R<()>,
    |a, m| a.prime_network_option_caches(&m.network_id, &m.options));
network_message!(command LoadCoreSiteOptions { network_id: HookValue } -> R<()>,
    |a, m| a.load_core_site_options(&m.network_id));

/// Binds `api` as the handler of every network option message; PHP overrides one by binding the same key.
pub fn register_network_option_handlers(container: &Container, api: Rc<NetworkOptionsApi>) {
    macro_rules! bind {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( { let a = api.clone(); bind_command_handler::<$c>(container, move |_, _| Ok(a.clone())); } )*
            $( { let a = api.clone(); bind_query_handler::<$q>(container, move |_, _| Ok(a.clone())); } )*
        };
    }
    bind!(commands: AddNetworkOption, UpdateNetworkOption, DeleteNetworkOption, PrimeNetworkOptionCaches,
        LoadCoreSiteOptions; queries: GetNetworkOption);
}
