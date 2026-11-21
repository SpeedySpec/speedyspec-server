//! The settings functions as use cases on the buses (Command / CQRS), handled by [`SettingsRegistry`].

use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_wp_hooks::{Callback, HookValue};

use crate::{OptionsError, SettingArgsInput, SettingsRegistry};

macro_rules! settings_message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        settings_message!(@impl $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for SettingsRegistry {
            #[allow(unused_variables)]
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
        impl QueryHandler<$name> for SettingsRegistry {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, OptionsError>;

settings_message!(command RegisterSetting { group: String, name: String, args: SettingArgsInput } -> R<()>,
    |a, m| a.register_setting(&m.group, &m.name, m.args));
settings_message!(command UnregisterSetting { group: String, name: String, deprecated: Option<Callback> } -> R<()>,
    |a, m| a.unregister_setting(&m.group, &m.name, m.deprecated));
settings_message!(query GetRegisteredSettings {} -> IndexMap<String, HookValue>, |a, m| a.registered_settings());
settings_message!(query FilterDefaultOption { default: HookValue, option: String, passed_default: bool } -> HookValue,
    |a, m| a.filter_default_option(m.default, &m.option, m.passed_default));

/// Binds `api` as the handler of every settings message; PHP overrides one by binding the same key.
pub fn register_settings_handlers(container: &Container, api: Rc<SettingsRegistry>) {
    macro_rules! bind {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( { let a = api.clone(); bind_command_handler::<$c>(container, move |_, _| Ok(a.clone())); } )*
            $( { let a = api.clone(); bind_query_handler::<$q>(container, move |_, _| Ok(a.clone())); } )*
        };
    }
    bind!(commands: RegisterSetting, UnregisterSetting; queries: GetRegisteredSettings, FilterDefaultOption);
}
