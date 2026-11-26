//! Every options function as a use case on the buses (Command / CQRS), handled by [`OptionsApi`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_wp_hooks_contracts::HookValue;

use crate::{
    AutoloadRequest, OptionsApi, OptionsError, filter_default_autoload_value_via_option_size,
};

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
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for OptionsApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for OptionsApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, OptionsError>;

message!(query GetOption { name: String, default: Option<HookValue> } -> R<HookValue>,
    |a, m| a.get_option(&m.name, m.default));
message!(query GetOptions { names: Vec<String> } -> R<Vec<(String, HookValue)>>, |a, m| a.get_options(&m.names));
message!(query LoadAlloptions { force_cache: bool } -> R<HookValue>, |a, m| a.load_alloptions(m.force_cache));
message!(query DetermineOptionAutoloadValue { name: String, value: HookValue, serialized: HookValue, autoload: HookValue }
    -> R<HookValue>,
    |a, m| a.d.autoload
        .determine(&m.name, &m.value, &m.serialized, &AutoloadRequest::from_value(&m.autoload.read()))
        .map(|decided| HookValue::Str(decided.as_str().to_string())));
message!(query FilterDefaultAutoloadValueViaOptionSize { autoload: HookValue, name: String, value: HookValue,
    serialized: HookValue } -> R<HookValue>,
    |a, m| filter_default_autoload_value_via_option_size(&*a.d.hooks, m.autoload, &m.name, &m.serialized));
message!(query AutoloadValuesToAutoload {} -> R<HookValue>, |a, m| a.d.autoload.values_to_autoload());
message!(command AddOption { name: String, value: HookValue, deprecated: HookValue, autoload: HookValue } -> R<bool>,
    |a, m| a.add_option(&m.name, m.value, m.deprecated, m.autoload));
message!(command UpdateOption { name: String, value: HookValue, autoload: HookValue } -> R<bool>,
    |a, m| a.update_option(&m.name, m.value, m.autoload));
message!(command DeleteOption { name: String } -> R<bool>, |a, m| a.delete_option(&m.name));
message!(command PrimeOptionCaches { names: Vec<String> } -> R<()>, |a, m| a.prime_option_caches(&m.names));
message!(command PrimeOptionCachesByGroup { group: String } -> R<()>, |a, m| a.prime_option_caches_by_group(&m.group));
message!(command SetOptionAutoloadValues { options: Vec<(String, HookValue)> } -> R<Vec<(String, bool)>>,
    |a, m| a.set_option_autoload_values(m.options));
message!(command SetOptionsAutoload { names: Vec<String>, autoload: HookValue } -> R<Vec<(String, bool)>>,
    |a, m| a.set_options_autoload(&m.names, m.autoload));
message!(command SetOptionAutoload { name: String, autoload: HookValue } -> R<bool>,
    |a, m| a.set_option_autoload(&m.name, m.autoload));
message!(command ProtectSpecialOption { name: String } -> R<()>, |a, m| a.protect_special_option(&m.name));

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every options message; PHP overrides one by binding the same key.
pub fn register_options_handlers(container: &Container, api: Rc<OptionsApi>) {
    bind_all!(container, api;
        commands: AddOption, UpdateOption, DeleteOption, PrimeOptionCaches, PrimeOptionCachesByGroup,
            SetOptionAutoloadValues, SetOptionsAutoload, SetOptionAutoload, ProtectSpecialOption;
        queries: GetOption, GetOptions, LoadAlloptions, DetermineOptionAutoloadValue,
            FilterDefaultAutoloadValueViaOptionSize, AutoloadValuesToAutoload);
}
