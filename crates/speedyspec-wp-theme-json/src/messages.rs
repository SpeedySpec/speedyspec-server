//! Every `ThemeJsonApi` method as a use case on the buses (Command / CQRS), handled by [`ThemeJsonApi`].
//!
//! A PHP override of one method is a rebinding of its key in the container.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler};
use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::model::ThemeJson;

use crate::ThemeJsonApi;

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
            const KEY: &'static str = concat!("SpeedySpec\\WpThemeJson\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpThemeJson\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for ThemeJsonApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for ThemeJsonApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

message!(query ConstructThemeJson { data: Value, origin: Vec<u8> } -> ThemeJson, |a, m| a.construct(m.data, &m.origin));
message!(query MergeThemeJson { base: ThemeJson, incoming: ThemeJson } -> ThemeJson, |a, m| a.merge(&m.base, &m.incoming));
message!(query GetRawData { doc: ThemeJson } -> Value, |a, m| a.get_raw_data(&m.doc));
message!(query GetSettings { doc: ThemeJson } -> Value, |a, m| a.get_settings(&m.doc));
message!(query GetData { doc: ThemeJson } -> Value, |a, m| a.get_data(&m.doc));
message!(query GetCustomTemplates { doc: ThemeJson } -> Value, |a, m| a.get_custom_templates(&m.doc));
message!(query GetTemplateParts { doc: ThemeJson } -> Value, |a, m| a.get_template_parts(&m.doc));
message!(query GetPatterns { doc: ThemeJson } -> Value, |a, m| a.get_patterns(&m.doc));
message!(query GetFromEditorSettings { settings: Value } -> Value, |a, m| a.get_from_editor_settings(&m.settings));
message!(query GetElementClassName { element: Vec<u8> } -> Vec<u8>, |a, m| a.get_element_class_name(&m.element));
message!(query SchemaMigrate { data: Value, origin: Vec<u8> } -> Value, |a, m| a.schema_migrate(m.data, &m.origin));
message!(command SetSpacingSizes { doc: ThemeJson } -> ThemeJson, |a, m| {
    let mut doc = m.doc;
    a.set_spacing_sizes(&mut doc);
    doc
});

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every theme.json message; PHP overrides one by binding the same key.
pub fn register_theme_json_handlers(container: &Container, api: Rc<ThemeJsonApi>) {
    bind_all!(container, api;
        commands: SetSpacingSizes;
        queries: ConstructThemeJson, MergeThemeJson, GetRawData, GetSettings, GetData, GetCustomTemplates,
            GetTemplateParts, GetPatterns, GetFromEditorSettings, GetElementClassName, SchemaMigrate);
}
