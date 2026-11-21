//! The rewrite functions as use cases on the buses (Command / CQRS), handled by [`RewriteService`]; PHP overrides
//! one by binding the same key. Each message carries the state source it works on (in PHP, `$wp_rewrite`), and the
//! handler loads the state, runs the method and stores the state back.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_routing::{EndpointMask, PermastructArgs, Rule, RulePriority};

use crate::{EndpointQueryVar, RewriteError, RewriteService, RewriteStateSource, RuleQuery};

macro_rules! rewrite_message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$svc:ident, $state:ident, $src:ident, $m:ident| $body:expr) => {
        #[derive(Clone)]
        pub struct $name { pub source: Rc<dyn RewriteStateSource>, $(pub $field: $ty),* }
        rewrite_message!(@impl $kind $name $result, $svc, $state, $src, $m, $body);
    };
    (@impl command $name:ident $result:ty, $svc:ident, $state:ident, $src:ident, $m:ident, $body:expr) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpRewrite\\", stringify!($name), "Handler");
        }
        impl CommandHandler<$name> for RewriteService {
            #[allow(unused_variables, unused_mut)]
            fn handle(&self, $m: $name) -> $result {
                let $svc = self;
                let $src = $m.source.clone();
                let mut $state = $src.load();
                let result = $body;
                $src.store(&$state);
                result
            }
        }
    };
    (@impl query $name:ident $result:ty, $svc:ident, $state:ident, $src:ident, $m:ident, $body:expr) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpRewrite\\", stringify!($name), "Handler");
        }
        impl QueryHandler<$name> for RewriteService {
            #[allow(unused_variables, unused_mut)]
            fn handle(&self, $m: $name) -> $result {
                let $svc = self;
                let $src = $m.source.clone();
                let mut $state = $src.load();
                let result = $body;
                $src.store(&$state);
                result
            }
        }
    };
}

type R<T> = Result<T, RewriteError>;

rewrite_message!(command InitRewrite {} -> R<()>, |s, st, src, m| s.init(&mut st));
rewrite_message!(command AddRewriteRule { regex: String, query: RuleQuery, after: RulePriority } -> R<()>,
    |s, st, src, m| { s.add_rule(&mut st, &m.regex, m.query, m.after); Ok(()) });
rewrite_message!(command AddExternalRule { regex: String, query: String } -> R<()>,
    |s, st, src, m| { s.add_external_rule(&mut st, &m.regex, &m.query); Ok(()) });
rewrite_message!(command AddRewriteTag { tag: String, regex: String, query: String } -> R<()>,
    |s, st, src, m| { s.add_rewrite_tag(&mut st, &m.tag, &m.regex, &m.query); Ok(()) });
rewrite_message!(command RemoveRewriteTag { tag: String } -> R<()>,
    |s, st, src, m| { s.remove_rewrite_tag(&mut st, &m.tag); Ok(()) });
rewrite_message!(command AddPermastruct { name: String, structure: String, args: PermastructArgs } -> R<()>,
    |s, st, src, m| { s.add_permastruct(&mut st, &m.name, &m.structure, m.args); Ok(()) });
rewrite_message!(command RemovePermastruct { name: String } -> R<()>,
    |s, st, src, m| { s.remove_permastruct(&mut st, &m.name); Ok(()) });
rewrite_message!(command AddRewriteEndpoint { name: String, places: EndpointMask, query_var: EndpointQueryVar } -> R<()>,
    |s, st, src, m| { s.add_endpoint(&mut st, &m.name, m.places, m.query_var); Ok(()) });
rewrite_message!(command FlushRewriteRules { hard: bool } -> R<()>,
    |s, st, src, m| s.flush_rules(&mut st, src.as_ref(), m.hard));
rewrite_message!(command SetPermalinkStructure { structure: String } -> R<()>,
    |s, st, src, m| s.set_permalink_structure(&mut st, &m.structure));
rewrite_message!(command SetCategoryBase { base: String } -> R<()>, |s, st, src, m| s.set_category_base(&mut st, &m.base));
rewrite_message!(command SetTagBase { base: String } -> R<()>, |s, st, src, m| s.set_tag_base(&mut st, &m.base));
rewrite_message!(query GetRewriteRules {} -> R<Vec<Rule>>, |s, st, src, m| s.wp_rewrite_rules(&mut st, src.as_ref()));
rewrite_message!(query GenerateAllRewriteRules {} -> R<Vec<Rule>>, |s, st, src, m| s.rewrite_rules(&mut st, src.as_ref()));
rewrite_message!(query GeneratePermastructRules { structure: String, args: PermastructArgs } -> R<Vec<Rule>>,
    |s, st, src, m| s.expand(&st, &m.structure, m.args).map(|rules| rules.into_rules()));
rewrite_message!(query GetModRewriteRules {} -> R<String>, |s, st, src, m| s.mod_rewrite_rules(&mut st, src.as_ref()));
rewrite_message!(query GetIis7RewriteRules { add_parent_tags: bool } -> R<String>,
    |s, st, src, m| s.iis7_url_rewrite_rules(&st, m.add_parent_tags));

/// Binds `service` as the handler of every rewrite message.
pub fn register_rewrite_handlers(container: &Container, service: Rc<RewriteService>) {
    macro_rules! bind {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( { let s = service.clone(); bind_command_handler::<$c>(container, move |_, _| Ok(s.clone())); } )*
            $( { let s = service.clone(); bind_query_handler::<$q>(container, move |_, _| Ok(s.clone())); } )*
        };
    }
    bind!(
        commands: InitRewrite, AddRewriteRule, AddExternalRule, AddRewriteTag, RemoveRewriteTag, AddPermastruct,
            RemovePermastruct, AddRewriteEndpoint, FlushRewriteRules, SetPermalinkStructure, SetCategoryBase,
            SetTagBase;
        queries: GetRewriteRules, GenerateAllRewriteRules, GeneratePermastructRules, GetModRewriteRules,
            GetIis7RewriteRules
    );
}
