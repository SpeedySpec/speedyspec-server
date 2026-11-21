use std::rc::Rc;

use speedyspec_routing::Rule;
use speedyspec_wp_hooks::{HookValue, Key};
use speedyspec_wp_options::OptionsApi;

use crate::{RewriteError, RewriteState};

/// Where `WP_Rewrite`'s state lives between calls (in PHP, the `$wp_rewrite` object).
pub trait RewriteStateSource {
    /// The current state; an error when it cannot be read, in which case the caller must not store over it.
    fn load(&self) -> Result<RewriteState, RewriteError>;
    fn store(&self, state: &RewriteState);
    /// The object hooks receive as `$wp_rewrite` (`generate_rewrite_rules` passes it by reference).
    fn hook_value(&self) -> HookValue;
}

/// `home_url()` and `site_url()`.
pub trait SiteUrls {
    fn home_url(&self) -> String;
    fn site_url(&self) -> String;
}

/// `is_multisite() && is_main_site()`, which adds the signup and activation rules.
pub trait Multisite {
    fn is_multisite_main_site(&self) -> bool;
}

/// `did_action( 'wp_loaded' )`.
pub trait WpLoaded {
    fn did_wp_loaded(&self) -> bool;
}

/// URIs with their post IDs, in WordPress's order.
pub type UriIndex = Vec<(String, String)>;

/// `WP_Rewrite::page_uri_index()`'s data: every page's URI and every page attachment's URI, each with its ID, in
/// WordPress's order. Reading posts belongs to the posts port; until it exists, PHP answers.
pub trait PageUris {
    fn page_uris(&self) -> (UriIndex, UriIndex);
}

/// `get_post_types( array( '_builtin' => false ) )`, each with `is_post_type_hierarchical()`.
pub trait PostTypes {
    fn custom_post_types(&self) -> Vec<(String, bool)>;
}

/// `$wp->add_query_var()`, which `add_endpoint` calls.
pub trait PublicQueryVars {
    fn add_query_var(&self, name: &str);
}

/// The options `WP_Rewrite` reads and writes.
pub trait RewriteOptions {
    /// `get_option( $name )` as text; `None` for `false` or a value that is not text.
    fn get(&self, name: &str) -> Result<Option<String>, RewriteError>;
    /// `get_option( 'rewrite_rules' )` as rules; `None` when it is empty or not an array.
    fn rules(&self) -> Result<Option<Vec<Rule>>, RewriteError>;
    fn set_rules(&self, rules: &[Rule]) -> Result<(), RewriteError>;
    fn delete_rules(&self) -> Result<(), RewriteError>;
    fn update(&self, name: &str, value: &str) -> Result<(), RewriteError>;
}

/// [`RewriteOptions`] over the ported options API (Adapter).
pub struct OptionsRewriteOptions {
    api: Rc<OptionsApi>,
}

impl OptionsRewriteOptions {
    pub fn new(api: Rc<OptionsApi>) -> OptionsRewriteOptions {
        OptionsRewriteOptions { api }
    }
}

fn options_error(err: impl std::fmt::Display) -> RewriteError {
    RewriteError(err.to_string())
}

/// Reads an option value as WordPress stores `rewrite_rules`: an array of regex => query.
pub fn rules_from_value(value: &HookValue) -> Option<Vec<Rule>> {
    let HookValue::Array(items) = value else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    let rules = items
        .iter()
        .map(|(key, query)| {
            let pattern = match key {
                Key::Str(s) => s.clone(),
                Key::Int(i) => i.to_string(),
            };
            let query = match query {
                HookValue::Str(s) => s.clone(),
                HookValue::Int(i) => i.to_string(),
                _ => String::new(),
            };
            Rule { pattern, query }
        })
        .collect();
    Some(rules)
}

/// Writes rules as the PHP array WordPress stores.
pub fn rules_to_value(rules: &[Rule]) -> HookValue {
    HookValue::Array(
        rules
            .iter()
            .map(|rule| {
                (
                    Key::Str(rule.pattern.clone()),
                    HookValue::Str(rule.query.clone()),
                )
            })
            .collect(),
    )
}

impl RewriteOptions for OptionsRewriteOptions {
    fn get(&self, name: &str) -> Result<Option<String>, RewriteError> {
        let value = self
            .api
            .get_option(name, None)
            .map_err(options_error)?
            .read()
            .into_owned();
        Ok(match value {
            HookValue::Str(s) => Some(s),
            HookValue::Int(i) => Some(i.to_string()),
            _ => None,
        })
    }

    fn rules(&self) -> Result<Option<Vec<Rule>>, RewriteError> {
        let value = self
            .api
            .get_option("rewrite_rules", None)
            .map_err(options_error)?
            .read()
            .into_owned();
        Ok(rules_from_value(&value))
    }

    fn set_rules(&self, rules: &[Rule]) -> Result<(), RewriteError> {
        self.api
            .update_option("rewrite_rules", rules_to_value(rules), HookValue::Null)
            .map_err(options_error)?;
        Ok(())
    }

    fn delete_rules(&self) -> Result<(), RewriteError> {
        self.api
            .delete_option("rewrite_rules")
            .map_err(options_error)?;
        Ok(())
    }

    fn update(&self, name: &str, value: &str) -> Result<(), RewriteError> {
        self.api
            .update_option(name, HookValue::Str(value.into()), HookValue::Null)
            .map_err(options_error)?;
        Ok(())
    }
}
