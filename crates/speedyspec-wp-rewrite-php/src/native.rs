//! The native functions the facade calls. `call` runs a `WP_Rewrite` method: through the bus for the operations that
//! are use cases (so PHP can override one by binding its key), directly on the service for the rest.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{HandlerRef, QueryRef};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{PhpObject, Value};
use speedyspec_router::wordpress::matches_map;
use speedyspec_routing::{EndpointMask, PermastructArgs, Rule, RulePriority};
use speedyspec_wp_rewrite::{
    AddExternalRule, AddPermastruct, AddRewriteEndpoint, AddRewriteRule, AddRewriteTag,
    EndpointQueryVar, FlushRewriteRules, GenerateAllRewriteRules, GetIis7RewriteRules,
    GetModRewriteRules, GetRewriteRules, InitRewrite, RemovePermastruct, RemoveRewriteTag,
    RewriteError, RewriteService, RewriteStateSource, RuleQuery, SetCategoryBase,
    SetPermalinkStructure, SetTagBase, WpOutcome, WpRouting,
};

use crate::values::{arg, int, items, key_text, rules_value, text, truthy};
use crate::{PhpPageByPath, PhpRewriteState};
use speedyspec_php_ext::Key;
use speedyspec_router::WpSubject;
use speedyspec_routing::{QueryValue, RuleMatch};

const NATIVE: &str = "SpeedySpec\\WpRewrite\\Native\\";

pub(crate) struct Shared {
    pub(crate) container: Rc<Container>,
    pub(crate) service: Rc<RewriteService>,
    pub(crate) routing: Rc<WpRouting>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

pub(crate) fn install(
    container: Rc<Container>,
    service: Rc<RewriteService>,
    routing: Rc<WpRouting>,
) {
    SHARED.with(|s| {
        *s.borrow_mut() = Some(Rc::new(Shared {
            container,
            service,
            routing,
        }))
    });
}

pub(crate) fn shared() -> Result<Rc<Shared>, NativeError> {
    SHARED
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("RuntimeException", "the rewrite API is not available"))
}

fn fail(err: impl std::fmt::Display) -> NativeError {
    NativeError::throw("RuntimeException", err.to_string())
}

fn rewrite_error(err: RewriteError) -> NativeError {
    fail(err.0)
}

fn optional(value: Option<String>) -> Value {
    value.map_or(Value::Bool(false), Value::Str)
}

fn flag(args: &[Value], i: usize, default: bool) -> bool {
    arg(args, i).map_or(default, truthy)
}

/// `add_permastruct()`'s `$args`: a non-array is `with_front`, a fourth argument is `ep_mask`, unknown keys are
/// dropped and the rest default as WordPress's `wp_parse_args()` does.
fn permastruct_args(args: &[Value]) -> PermastructArgs {
    let defaults = PermastructArgs::default();
    let mut parsed = defaults;
    match arg(args, 2) {
        Some(Value::Array(entries)) => {
            for (key, value) in entries {
                match key_text(key).as_str() {
                    "with_front" => parsed.with_front = truthy(value),
                    "ep_mask" => parsed.ep_mask = EndpointMask(int(value) as u32),
                    "paged" => parsed.paged = truthy(value),
                    "feed" => parsed.feed = truthy(value),
                    "forcomments" => parsed.forcomments = truthy(value),
                    "walk_dirs" => parsed.walk_dirs = truthy(value),
                    "endpoints" => parsed.endpoints = truthy(value),
                    _ => {}
                }
            }
        }
        Some(other) => parsed.with_front = truthy(other),
        None => {}
    }
    if let Some(mask) = arg(args, 3) {
        parsed.ep_mask = EndpointMask(int(mask) as u32);
    }
    parsed
}

fn rule_query(value: Option<&Value>) -> RuleQuery {
    match value {
        Some(Value::Array(entries)) => RuleQuery::Pairs(
            entries
                .iter()
                .filter(|(_, v)| !matches!(v, Value::Bool(false) | Value::Null))
                .map(|(k, v)| (key_text(k), text(v)))
                .collect(),
        ),
        Some(other) => RuleQuery::Text(text(other)),
        None => RuleQuery::Text(String::new()),
    }
}

/// `$query_var` of `add_endpoint()`: `true` or `null` is the name, any other falsy value is none.
fn endpoint_query_var(value: Option<&Value>) -> EndpointQueryVar {
    match value {
        None | Some(Value::Bool(true)) | Some(Value::Null) => EndpointQueryVar::SameAsName,
        Some(v) if !truthy(v) => EndpointQueryVar::None,
        Some(v) => EndpointQueryVar::Named(text(v)),
    }
}

fn rules_or_empty(rules: Vec<Rule>) -> Value {
    rules_value(&rules)
}

/// Runs one `WP_Rewrite` method on `object`.
fn call_method(
    shared: &Shared,
    object: PhpObject,
    method: &str,
    args: &[Value],
) -> Result<Value, NativeError> {
    let source: Rc<dyn RewriteStateSource> = Rc::new(PhpRewriteState::new(object));
    let c = &shared.container;
    let s = shared.service.as_ref();
    let bus = |err: speedyspec_cqrs::BusError| fail(err);
    let string = |i: usize| arg(args, i).map(text).unwrap_or_default();

    macro_rules! command {
        ($message:expr) => {{
            HandlerRef::new(c.clone())
                .call($message)
                .map_err(bus)?
                .map_err(rewrite_error)?;
            Ok(Value::Null)
        }};
    }
    macro_rules! query {
        ($message:expr) => {
            QueryRef::new(c.clone())
                .call($message)
                .map_err(bus)?
                .map_err(rewrite_error)
        };
    }
    /// Reads the state, runs `$body` on it, writes it back.
    macro_rules! direct {
        (|$state:ident| $body:expr) => {{
            #[allow(unused_mut)]
            let mut $state = source.load().map_err(rewrite_error)?;
            let before = $state.clone();
            let result = $body;
            if $state != before {
                source.store(&$state);
            }
            Ok(result)
        }};
    }

    match method {
        "init" => command!(InitRewrite { source }),
        "add_rule" => {
            let after = if string(2).is_empty() || string(2) == "bottom" {
                RulePriority::Bottom
            } else {
                RulePriority::Top
            };
            command!(AddRewriteRule {
                source,
                regex: string(0),
                query: rule_query(arg(args, 1)),
                after
            })
        }
        "add_external_rule" => command!(AddExternalRule {
            source,
            regex: string(0),
            query: string(1)
        }),
        "add_rewrite_tag" => command!(AddRewriteTag {
            source,
            tag: string(0),
            regex: string(1),
            query: string(2)
        }),
        "remove_rewrite_tag" => command!(RemoveRewriteTag {
            source,
            tag: string(0)
        }),
        "add_permastruct" => {
            command!(AddPermastruct {
                source,
                name: string(0),
                structure: string(1),
                args: permastruct_args(args)
            })
        }
        "remove_permastruct" => command!(RemovePermastruct {
            source,
            name: string(0)
        }),
        "add_endpoint" => command!(AddRewriteEndpoint {
            source,
            name: string(0),
            places: EndpointMask(arg(args, 1).map_or(0, int) as u32),
            query_var: endpoint_query_var(arg(args, 2)),
        }),
        "flush_rules" => command!(FlushRewriteRules {
            source,
            hard: flag(args, 0, true)
        }),
        "set_permalink_structure" => command!(SetPermalinkStructure {
            source,
            structure: string(0)
        }),
        "set_category_base" => command!(SetCategoryBase {
            source,
            base: string(0)
        }),
        "set_tag_base" => command!(SetTagBase {
            source,
            base: string(0)
        }),
        "wp_rewrite_rules" => {
            let rules = query!(GetRewriteRules {
                source: source.clone()
            })?;
            // Plain permalinks leave WordPress's `$this->rules` as '', which is what it returns.
            Ok(if rules.is_empty() {
                Value::Str(String::new())
            } else {
                rules_or_empty(rules)
            })
        }
        "rewrite_rules" => query!(GenerateAllRewriteRules { source }).map(rules_or_empty),
        "mod_rewrite_rules" => query!(GetModRewriteRules { source }).map(Value::Str),
        "iis7_url_rewrite_rules" => query!(GetIis7RewriteRules {
            source,
            add_parent_tags: flag(args, 0, false)
        })
        .map(Value::Str),
        "generate_rewrite_rules" => {
            let permastruct_args = PermastructArgs {
                with_front: true,
                ep_mask: EndpointMask(arg(args, 1).map_or(0, int) as u32),
                paged: flag(args, 2, true),
                feed: flag(args, 3, true),
                forcomments: flag(args, 4, false),
                walk_dirs: flag(args, 5, true),
                endpoints: flag(args, 6, true),
            };
            query!(speedyspec_wp_rewrite::GeneratePermastructRules {
                source,
                structure: string(0),
                args: permastruct_args
            })
            .map(rules_or_empty)
        }
        "generate_rewrite_rule" => {
            let permastruct_args = PermastructArgs {
                with_front: true,
                ep_mask: EndpointMask::NONE,
                paged: false,
                feed: false,
                forcomments: false,
                walk_dirs: flag(args, 1, false),
                endpoints: true,
            };
            query!(speedyspec_wp_rewrite::GeneratePermastructRules {
                source,
                structure: string(0),
                args: permastruct_args
            })
            .map(rules_or_empty)
        }
        "page_rewrite_rules" => {
            direct!(|st| rules_or_empty(s.page_rewrite_rules(&mut st).map_err(rewrite_error)?))
        }
        "get_date_permastruct" => direct!(|st| optional(s.get_date_permastruct(&mut st))),
        "get_year_permastruct" => direct!(|st| optional(s.get_year_permastruct(&mut st))),
        "get_month_permastruct" => direct!(|st| optional(s.get_month_permastruct(&mut st))),
        "get_day_permastruct" => direct!(|st| optional(s.get_day_permastruct(&mut st))),
        "get_category_permastruct" => direct!(|st| optional(s.get_category_permastruct(&st))),
        "get_tag_permastruct" => direct!(|st| optional(s.get_tag_permastruct(&st))),
        "get_extra_permastruct" => direct!(|st| optional(s.get_extra_permastruct(&st, &string(0)))),
        "get_author_permastruct" => direct!(|st| optional(s.get_author_permastruct(&mut st))),
        "get_search_permastruct" => direct!(|st| optional(s.get_search_permastruct(&mut st))),
        "get_page_permastruct" => direct!(|st| optional(s.get_page_permastruct(&mut st))),
        "get_feed_permastruct" => direct!(|st| optional(s.get_feed_permastruct(&mut st))),
        "get_comment_feed_permastruct" => {
            direct!(|st| optional(s.get_comment_feed_permastruct(&mut st)))
        }
        other => Err(fail(format!("WP_Rewrite::{other}() is not ported"))),
    }
}

fn native_call(args: &[Value]) -> Result<Value, NativeError> {
    let shared = shared()?;
    let object = args
        .first()
        .cloned()
        .and_then(PhpObject::new)
        .ok_or_else(|| fail("WP_Rewrite methods need their object"))?;
    let method = args.get(1).map(text).unwrap_or_default();
    let method_args: Vec<Value> = args
        .get(2)
        .map(|a| items(a).into_iter().map(|(_, v)| v).collect())
        .unwrap_or_default();
    call_method(&shared, object, &method, &method_args)
}

fn native_matches_map(args: &[Value]) -> Result<Value, NativeError> {
    let subject = args.first().map(text).unwrap_or_default();
    let entries = args.get(1).map(items).unwrap_or_default();
    let max = entries
        .iter()
        .filter_map(|(k, _)| key_text(k).parse::<usize>().ok())
        .max();
    let mut captures: Vec<Option<String>> = vec![None; max.map_or(0, |m| m + 1)];
    for (key, value) in entries {
        if let Ok(i) = key_text(&key).parse::<usize>()
            && !matches!(value, Value::Null)
        {
            captures[i] = Some(text(&value));
        }
    }
    Ok(Value::Str(matches_map(&subject, &captures)))
}

fn query_value(value: &QueryValue) -> Value {
    match value {
        QueryValue::Text(text) => Value::Str(text.clone()),
        QueryValue::Map(entries) => Value::Array(
            entries
                .iter()
                .map(|(k, v)| (php_key(k), query_value(v)))
                .collect(),
        ),
    }
}

/// PHP turns a decimal-integer string key into an integer key.
fn php_key(key: &str) -> Key {
    match key.parse::<i64>() {
        Ok(i) if i.to_string() == key => Key::Int(i),
        _ => Key::Str(key.to_string()),
    }
}

fn match_value(found: Option<RuleMatch>) -> Value {
    let entry = |name: &str, value: Value| (Key::Str(name.into()), value);
    match found {
        Some(m) => Value::Array(vec![
            entry("matched_rule", Value::Str(m.matched_rule)),
            entry("matched_query", Value::Str(m.matched_query)),
            entry(
                "perma_query_vars",
                Value::Array(
                    m.query_vars
                        .iter()
                        .map(|(k, v)| (php_key(k), query_value(v)))
                        .collect(),
                ),
            ),
        ]),
        None => Value::Array(vec![
            entry("matched_rule", Value::Null),
            entry("matched_query", Value::Null),
            entry("perma_query_vars", Value::Null),
        ]),
    }
}

/// `speedyspec_rewrite_match( $rewrite, $requested_path, $requested_file )`: `WP::parse_request`'s rule loop. Null
/// when there are no rules; otherwise the matched rule, query and query variables, null when nothing matched.
fn native_rewrite_match(args: &[Value]) -> Result<Value, NativeError> {
    let shared = shared()?;
    let rewrite = args.first().cloned().unwrap_or(Value::Null);
    let rules = crate::values::rules(&rewrite);
    let subject = WpSubject {
        requested_path: args.get(1).map(text).unwrap_or_default(),
        requested_file: args.get(2).map(text).unwrap_or_default(),
    };
    let outcome = shared
        .routing
        .resolve_rules(rules, &subject, &PhpPageByPath)
        .map_err(rewrite_error)?;
    let from = match outcome {
        WpOutcome::NoRewrite => return Ok(Value::Null),
        WpOutcome::NoMatch => return Ok(match_value(None)),
        WpOutcome::Matched(m) => return Ok(match_value(Some(m))),
        WpOutcome::PendingGuard(index) | WpOutcome::NeedsPhp(index) => index,
    };
    // WordPress's own loop, from the rule no Rust engine compiles (declared by the facade).
    let args = [
        rewrite,
        Value::Str(subject.requested_path),
        Value::Str(subject.requested_file),
        Value::Int(from as i64),
    ];
    speedyspec_php::call(&Value::Str("speedyspec_rewrite_match_php".into()), &args).map_err(fail)
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    vec![
        native::function("speedyspec_rewrite_match", native_rewrite_match),
        native::function(&format!("{NATIVE}call"), native_call),
        native::function(&format!("{NATIVE}matches_map"), native_matches_map),
    ]
}
