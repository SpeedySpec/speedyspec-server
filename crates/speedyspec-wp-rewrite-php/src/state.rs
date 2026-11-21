//! `WP_Rewrite`'s state in its PHP object (Adapter): read from the object's properties, written back to them.

use speedyspec_php_ext::{Key, PhpObject, Value};
use speedyspec_routing::{Endpoint, EndpointMask, Permastruct, PermastructArgs, RewriteTag};
use speedyspec_wp_rewrite::{RewriteError, RewriteState, RewriteStateSource};

use crate::values::{int, items, key_text, list, rules, rules_value, strings_value, text, truthy};

/// The state of one `WP_Rewrite` object.
pub struct PhpRewriteState {
    object: PhpObject,
}

impl PhpRewriteState {
    pub fn new(object: PhpObject) -> PhpRewriteState {
        PhpRewriteState { object }
    }
}

fn permastruct_from(value: &Value) -> Permastruct {
    match value {
        Value::Array(entries) => {
            let get = |name: &str| {
                entries
                    .iter()
                    .find(|(k, _)| key_text(k) == name)
                    .map(|(_, v)| v)
            };
            if entries.len() == 2 && get("struct").is_none() {
                let values = list(value);
                return Permastruct {
                    struct_: text(&values[0]),
                    args: PermastructArgs {
                        ep_mask: EndpointMask(int(&values[1]) as u32),
                        ..PermastructArgs::default()
                    },
                };
            }
            let defaults = PermastructArgs::default();
            let flag = |name: &str, default: bool| get(name).map_or(default, truthy);
            Permastruct {
                struct_: get("struct").map(text).unwrap_or_default(),
                args: PermastructArgs {
                    with_front: flag("with_front", defaults.with_front),
                    ep_mask: EndpointMask(get("ep_mask").map_or(0, int) as u32),
                    paged: flag("paged", defaults.paged),
                    feed: flag("feed", defaults.feed),
                    forcomments: flag("forcomments", defaults.forcomments),
                    walk_dirs: flag("walk_dirs", defaults.walk_dirs),
                    endpoints: flag("endpoints", defaults.endpoints),
                },
            }
        }
        other => Permastruct {
            struct_: text(other),
            args: PermastructArgs::default(),
        },
    }
}

/// The array WordPress keeps for a permastruct: its arguments, then `struct`.
pub fn permastruct_value(permastruct: &Permastruct) -> Value {
    let a = &permastruct.args;
    let entry = |name: &str, value: Value| (Key::Str(name.into()), value);
    Value::Array(vec![
        entry("with_front", Value::Bool(a.with_front)),
        entry("ep_mask", Value::Int(i64::from(a.ep_mask.0))),
        entry("paged", Value::Bool(a.paged)),
        entry("feed", Value::Bool(a.feed)),
        entry("forcomments", Value::Bool(a.forcomments)),
        entry("walk_dirs", Value::Bool(a.walk_dirs)),
        entry("endpoints", Value::Bool(a.endpoints)),
        entry("struct", Value::Str(permastruct.struct_.clone())),
    ])
}

/// Reads `get_object_vars( $wp_rewrite )` into the state; a missing property keeps its default (or stays unset).
pub fn state_from_vars(vars: &Value) -> RewriteState {
    let entries = items(vars);
    let get = |name: &str| {
        entries
            .iter()
            .find(|(k, _)| key_text(k) == name)
            .map(|(_, v)| v)
    };
    let mut s = RewriteState::default();
    let string = |name: &str, target: &mut String| {
        if let Some(v) = get(name) {
            *target = text(v);
        }
    };
    string("permalink_structure", &mut s.permalink_structure);
    string("author_base", &mut s.author_base);
    string("search_base", &mut s.search_base);
    string("comments_base", &mut s.comments_base);
    string("pagination_base", &mut s.pagination_base);
    string("comments_pagination_base", &mut s.comments_pagination_base);
    string("feed_base", &mut s.feed_base);
    string("front", &mut s.front);
    string("root", &mut s.root);
    string("index", &mut s.index);
    string("matches", &mut s.matches);
    s.use_trailing_slashes = get("use_trailing_slashes").is_some_and(truthy);
    s.use_verbose_rules = get("use_verbose_rules").is_some_and(truthy);
    s.use_verbose_page_rules = get("use_verbose_page_rules").is_none_or(truthy);
    s.author_structure = get("author_structure").map(text);
    s.date_structure = get("date_structure").map(text);
    s.page_structure = get("page_structure").map(text);
    s.search_structure = get("search_structure").map(text);
    s.comment_feed_structure = get("comment_feed_structure").map(text);
    s.feed_structure = get("feed_structure").map(text);
    s.rules = match get("rules") {
        Some(v @ Value::Array(_)) => Some(rules(v)),
        _ => None,
    };
    s.extra_rules = get("extra_rules").map(rules).unwrap_or_default();
    s.extra_rules_top = get("extra_rules_top").map(rules).unwrap_or_default();
    s.non_wp_rules = get("non_wp_rules").map(rules).unwrap_or_default();
    s.extra_permastructs = get("extra_permastructs")
        .map(|v| {
            items(v)
                .iter()
                .map(|(k, v)| (key_text(k), permastruct_from(v)))
                .collect()
        })
        .unwrap_or_default();
    s.endpoints = get("endpoints")
        .map(|v| {
            list(v)
                .iter()
                .map(|e| {
                    let parts = list(e);
                    let at = |i: usize| parts.get(i).cloned().unwrap_or(Value::Null);
                    Endpoint {
                        mask: EndpointMask(int(&at(0)) as u32),
                        name: text(&at(1)),
                        query_var: if truthy(&at(2)) {
                            text(&at(2))
                        } else {
                            String::new()
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if let (Some(codes), Some(replaces), Some(queries)) = (
        get("rewritecode"),
        get("rewritereplace"),
        get("queryreplace"),
    ) {
        let (codes, replaces, queries) = (list(codes), list(replaces), list(queries));
        s.tags = codes
            .iter()
            .enumerate()
            .map(|(i, code)| RewriteTag {
                tag: text(code),
                regex: replaces.get(i).map(text).unwrap_or_default(),
                query: queries.get(i).map(text).unwrap_or_default(),
            })
            .collect();
    }
    if let Some(feeds) = get("feeds") {
        s.feeds = list(feeds).iter().map(text).collect();
    }
    s
}

/// The properties to write back, and the names to unset.
pub fn vars_from_state(s: &RewriteState) -> (Value, Vec<String>) {
    let mut vars: Vec<(Key, Value)> = Vec::new();
    let mut unset = Vec::new();
    let mut put = |name: &str, value: Value| vars.push((Key::Str(name.into()), value));
    put(
        "permalink_structure",
        Value::Str(s.permalink_structure.clone()),
    );
    put("use_trailing_slashes", Value::Bool(s.use_trailing_slashes));
    put("author_base", Value::Str(s.author_base.clone()));
    put("search_base", Value::Str(s.search_base.clone()));
    put("comments_base", Value::Str(s.comments_base.clone()));
    put("pagination_base", Value::Str(s.pagination_base.clone()));
    put(
        "comments_pagination_base",
        Value::Str(s.comments_pagination_base.clone()),
    );
    put("feed_base", Value::Str(s.feed_base.clone()));
    put("front", Value::Str(s.front.clone()));
    put("root", Value::Str(s.root.clone()));
    put("index", Value::Str(s.index.clone()));
    put("matches", Value::Str(s.matches.clone()));
    put("use_verbose_rules", Value::Bool(s.use_verbose_rules));
    put(
        "use_verbose_page_rules",
        Value::Bool(s.use_verbose_page_rules),
    );
    // Rules are written only when a method set them: load() leaves them out.
    if let Some(rules) = &s.rules {
        put("rules", rules_value(rules));
    }
    put("extra_rules", rules_value(&s.extra_rules));
    put("extra_rules_top", rules_value(&s.extra_rules_top));
    put("non_wp_rules", rules_value(&s.non_wp_rules));
    put(
        "extra_permastructs",
        Value::Array(
            s.extra_permastructs
                .iter()
                .map(|(n, p)| (Key::Str(n.clone()), permastruct_value(p)))
                .collect(),
        ),
    );
    put(
        "endpoints",
        Value::Array(
            s.endpoints
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let query_var = if e.query_var.is_empty() {
                        Value::Bool(false)
                    } else {
                        Value::Str(e.query_var.clone())
                    };
                    let entry = vec![
                        (Key::Int(0), Value::Int(i64::from(e.mask.0))),
                        (Key::Int(1), Value::Str(e.name.clone())),
                        (Key::Int(2), query_var),
                    ];
                    (Key::Int(i as i64), Value::Array(entry))
                })
                .collect(),
        ),
    );
    put(
        "rewritecode",
        strings_value(&s.tags.iter().map(|t| t.tag.clone()).collect::<Vec<_>>()),
    );
    put(
        "rewritereplace",
        strings_value(&s.tags.iter().map(|t| t.regex.clone()).collect::<Vec<_>>()),
    );
    put(
        "queryreplace",
        strings_value(&s.tags.iter().map(|t| t.query.clone()).collect::<Vec<_>>()),
    );
    put("feeds", strings_value(&s.feeds));
    for (name, value) in [
        ("author_structure", &s.author_structure),
        ("date_structure", &s.date_structure),
        ("page_structure", &s.page_structure),
        ("search_structure", &s.search_structure),
        ("comment_feed_structure", &s.comment_feed_structure),
        ("feed_structure", &s.feed_structure),
    ] {
        match value {
            Some(v) => put(name, Value::Str(v.clone())),
            None => unset.push(name.to_string()),
        }
    }
    (Value::Array(vars), unset)
}

impl RewriteStateSource for PhpRewriteState {
    /// A property that does not convert is an error, never defaults: storing defaults back would blank the object.
    fn load(&self) -> Result<RewriteState, RewriteError> {
        self.object
            .call("__speedyspec_vars", vec![])
            .map(|vars| state_from_vars(&vars))
            .map_err(|err| RewriteError(format!("reading WP_Rewrite's properties failed: {err}")))
    }

    fn load_rules(&self) -> Result<Option<Vec<speedyspec_routing::Rule>>, RewriteError> {
        let value = self
            .object
            .call("__speedyspec_rules", vec![])
            .map_err(|err| RewriteError(format!("reading WP_Rewrite::$rules failed: {err}")))?;
        Ok(match &value {
            Value::Array(_) => Some(crate::values::rules(&value)),
            _ => None,
        })
    }

    fn store(&self, state: &RewriteState) {
        let (vars, unset) = vars_from_state(state);
        if let Err(err) = self
            .object
            .call("__speedyspec_assign", vec![vars, strings_value(&unset)])
        {
            eprintln!("speedyspec_wordpress: writing WP_Rewrite's properties failed: {err}");
        }
    }

    fn hook_value(&self) -> Value {
        self.object.value().clone()
    }
}

/// The state of the `$wp_rewrite` global, looked up at each use: work deferred to `wp_loaded` uses it.
pub struct PhpGlobalRewriteState;

impl PhpGlobalRewriteState {
    fn current() -> Option<PhpRewriteState> {
        let value = speedyspec_php::eval(
            "speedyspec://wp-rewrite-global",
            "return $GLOBALS['wp_rewrite'] ?? null;",
        )
        .ok()?;
        PhpObject::new(value).map(PhpRewriteState::new)
    }
}

impl RewriteStateSource for PhpGlobalRewriteState {
    fn load(&self) -> Result<RewriteState, RewriteError> {
        PhpGlobalRewriteState::current()
            .ok_or_else(|| RewriteError("there is no $wp_rewrite object".into()))?
            .load()
    }

    fn load_rules(&self) -> Result<Option<Vec<speedyspec_routing::Rule>>, RewriteError> {
        PhpGlobalRewriteState::current()
            .ok_or_else(|| RewriteError("there is no $wp_rewrite object".into()))?
            .load_rules()
    }

    fn store(&self, state: &RewriteState) {
        if let Some(current) = PhpGlobalRewriteState::current() {
            current.store(state);
        }
    }

    fn hook_value(&self) -> Value {
        PhpGlobalRewriteState::current().map_or(Value::Null, |s| s.hook_value())
    }
}
