//! The transient functions as use cases on the buses (Command / CQRS), handled by [`TransientsApi`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_wp_hooks::HookValue;

use crate::{OptionsError, TransientsApi};

macro_rules! transient_message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        transient_message!(@impl $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpOptions\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for TransientsApi {
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
        impl QueryHandler<$name> for TransientsApi {
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, OptionsError>;

transient_message!(query GetTransient { name: String } -> R<HookValue>, |a, m| a.get_transient(&m.name));
transient_message!(command SetTransient { name: String, value: HookValue, expiration: HookValue } -> R<bool>,
    |a, m| a.set_transient(&m.name, m.value, &m.expiration));
transient_message!(command DeleteTransient { name: String } -> R<bool>, |a, m| a.delete_transient(&m.name));
transient_message!(query GetSiteTransient { name: String } -> R<HookValue>, |a, m| a.get_site_transient(&m.name));
transient_message!(command SetSiteTransient { name: String, value: HookValue, expiration: HookValue } -> R<bool>,
    |a, m| a.set_site_transient(&m.name, m.value, &m.expiration));
transient_message!(command DeleteSiteTransient { name: String } -> R<bool>, |a, m| a.delete_site_transient(&m.name));
transient_message!(command DeleteExpiredTransients { force_db: bool } -> R<()>,
    |a, m| a.delete_expired_transients(m.force_db));

/// Binds `api` as the handler of every transient message; PHP overrides one by binding the same key.
pub fn register_transient_handlers(container: &Container, api: Rc<TransientsApi>) {
    macro_rules! bind {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( { let a = api.clone(); bind_command_handler::<$c>(container, move |_, _| Ok(a.clone())); } )*
            $( { let a = api.clone(); bind_query_handler::<$q>(container, move |_, _| Ok(a.clone())); } )*
        };
    }
    bind!(commands: SetTransient, DeleteTransient, SetSiteTransient, DeleteSiteTransient, DeleteExpiredTransients;
        queries: GetTransient, GetSiteTransient);
}
