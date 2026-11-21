//! `WP_Rewrite`'s registration methods.

use speedyspec_routing::{
    Endpoint, EndpointMask, Permastruct, PermastructArgs, RewriteTag, Rule, RulePriority,
};

use crate::{RewriteService, RewriteState};

/// The query `add_rewrite_rule` takes: a string, or an array of query variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleQuery {
    Text(String),
    /// Joined after `index.php?` as `add_query_arg()` joins them, unencoded.
    Pairs(Vec<(String, String)>),
}

/// `add_rewrite_endpoint`'s `$query_var`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointQueryVar {
    /// `true` or `null`: the endpoint's name.
    SameAsName,
    /// `false`: no query variable.
    None,
    Named(String),
}

fn push_or_replace(rules: &mut Vec<Rule>, pattern: &str, query: String) {
    match rules.iter_mut().find(|rule| rule.pattern == pattern) {
        Some(rule) => rule.query = query,
        None => rules.push(Rule {
            pattern: pattern.into(),
            query,
        }),
    }
}

impl RewriteService {
    /// `WP_Rewrite::add_rewrite_tag()`: replaces a known tag's regex and query, or appends a new tag.
    pub fn add_rewrite_tag(&self, state: &mut RewriteState, tag: &str, regex: &str, query: &str) {
        match state.tags.iter_mut().find(|t| t.tag == tag) {
            Some(existing) => {
                existing.regex = regex.into();
                existing.query = query.into();
            }
            None => state.tags.push(RewriteTag {
                tag: tag.into(),
                regex: regex.into(),
                query: query.into(),
            }),
        }
    }

    pub fn remove_rewrite_tag(&self, state: &mut RewriteState, tag: &str) {
        state.tags.retain(|t| t.tag != tag);
    }

    /// `WP_Rewrite::add_rule()`: a query not aimed at the index file is an external rule.
    pub fn add_rule(
        &self,
        state: &mut RewriteState,
        regex: &str,
        query: RuleQuery,
        after: RulePriority,
    ) {
        let (query, external) = match query {
            RuleQuery::Pairs(pairs) => {
                let joined: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
                (format!("index.php?{}", joined.join("&")), false)
            }
            RuleQuery::Text(text) => {
                let front = text.find('?').map_or(text.as_str(), |i| &text[..i]);
                let external = front != state.index;
                (text, external)
            }
        };
        if external {
            self.add_external_rule(state, regex, &query);
        } else if after == RulePriority::Bottom {
            push_or_replace(&mut state.extra_rules, regex, query);
        } else {
            push_or_replace(&mut state.extra_rules_top, regex, query);
        }
    }

    pub fn add_external_rule(&self, state: &mut RewriteState, regex: &str, query: &str) {
        push_or_replace(&mut state.non_wp_rules, regex, query.into());
    }

    /// `WP_Rewrite::add_endpoint()`, which also registers its query variable with `$wp`.
    pub fn add_endpoint(
        &self,
        state: &mut RewriteState,
        name: &str,
        places: EndpointMask,
        query_var: EndpointQueryVar,
    ) {
        let query_var = match query_var {
            EndpointQueryVar::SameAsName => name.to_string(),
            EndpointQueryVar::None => String::new(),
            EndpointQueryVar::Named(var) => var,
        };
        state.endpoints.push(Endpoint {
            mask: places,
            name: name.into(),
            query_var: query_var.clone(),
        });
        if !query_var.is_empty() {
            self.deps.query_vars.add_query_var(&query_var);
        }
    }

    /// `WP_Rewrite::add_permastruct()`: the structure is prefixed with `front` or `root`.
    pub fn add_permastruct(
        &self,
        state: &mut RewriteState,
        name: &str,
        structure: &str,
        args: PermastructArgs,
    ) {
        let prefix = if args.with_front {
            &state.front
        } else {
            &state.root
        };
        let permastruct = Permastruct {
            struct_: format!("{prefix}{structure}"),
            args,
        };
        match state.extra_permastructs.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = permastruct,
            None => state.extra_permastructs.push((name.into(), permastruct)),
        }
    }

    pub fn remove_permastruct(&self, state: &mut RewriteState, name: &str) {
        state.extra_permastructs.retain(|(n, _)| n != name);
    }
}
