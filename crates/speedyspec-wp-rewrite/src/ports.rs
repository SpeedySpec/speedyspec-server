use std::rc::Rc;

use speedyspec_routing::Rule;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};
use speedyspec_wp_options_contracts::Options;

use crate::{RewriteError, RewriteState};

/// Where `WP_Rewrite`'s state lives between calls (in PHP, the `$wp_rewrite` object).
pub trait RewriteStateSource {
    /// The current state; an error when it cannot be read, in which case the caller must not store over it.
    fn load(&self) -> Result<RewriteState, RewriteError>;
    /// The rules alone (`load()` may leave them out, since only rule generation reads them back).
    fn load_rules(&self) -> Result<Option<Vec<Rule>>, RewriteError> {
        Ok(self.load()?.rules)
    }
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

/// What the rewrite rules read from post types. Reading post types belongs to the posts port; until it is wired here,
/// PHP answers.
pub trait PostTypes {
    /// `get_post_types( array( '_builtin' => false ) )`, each with `is_post_type_hierarchical()`.
    fn custom_post_types(&self) -> Vec<(String, bool)>;
    /// `get_post_types( array( 'randomable' => true ), 'objects' )`, in registration order; an error carries what
    /// reading them threw.
    fn randomable_post_types(&self) -> Result<Vec<RandomablePostType>, RewriteError>;
}

/// A post type registered as `randomable`, as `WP_Rewrite::random_rewrite_rules()` reads it from
/// `get_post_types( array( 'randomable' => true ), 'objects' )`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RandomablePostType {
    pub name: String,
    /// `has_archive` when it is a string.
    pub archive_slug: Option<String>,
    /// Whether `rewrite` is an array: a post type without one has no pretty permalinks.
    pub rewrites: bool,
    /// `rewrite['slug']` when `rewrite` is an array holding a string slug.
    pub rewrite_slug: Option<String>,
}

/// A publicly queryable taxonomy, as `WP_Rewrite::random_rewrite_rules()` reads it from
/// `get_taxonomies( array( 'publicly_queryable' => true ), 'objects' )`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueryableTaxonomy {
    pub name: String,
    /// `rewrite['slug']` when `rewrite` is an array holding a string slug.
    pub rewrite_slug: Option<String>,
    /// `object_type`: the post types it is registered for.
    pub object_types: Vec<String>,
}

/// `get_taxonomies( array( 'publicly_queryable' => true ), 'objects' )`. Reading taxonomies belongs to the taxonomy
/// port; until it exists, PHP answers. An error carries what reading them threw.
pub trait Taxonomies {
    fn publicly_queryable(&self) -> Result<Vec<QueryableTaxonomy>, RewriteError>;
}

/// Whether a published post sits at a path: `get_page_by_path( $path, OBJECT, $post_type )` is a `WP_Post` and
/// `is_post_publicly_viewable()` says yes. Reading posts belongs to the posts port; until it exists, PHP answers. An
/// error carries what the lookup threw.
pub trait ViewablePosts {
    fn viewable_at(&self, path: &str, post_type: &str) -> Result<bool, RewriteError>;
}

/// `wp_is_random_content_redirect_enabled()`, from query.php: the `wp_enable_random_content_redirect` filter.
/// Queries are not ported yet, so PHP answers.
pub trait RandomContentRedirects {
    fn enabled(&self) -> Result<bool, RewriteError>;
}

/// Translations in the site language, which the i18n port will own; until it exists, PHP answers.
pub trait SiteTranslations {
    /// `_x( 'random', 'random content permalink base' )` in the site language: `switch_to_locale( get_locale() )`
    /// around it, and `restore_previous_locale()` after when the switch happened. An error carries what translating
    /// threw.
    fn random_base(&self) -> Result<String, RewriteError>;
}

/// `$wp->add_query_var()`, which `add_endpoint` calls.
pub trait PublicQueryVars {
    fn add_query_var(&self, name: &str);
}

/// The options `WP_Rewrite` reads and writes.
pub trait RewriteOptions {
    /// `get_option( $name )` as text; `None` for `false` or a value that is not text.
    fn get(&self, name: &str) -> Result<Option<String>, RewriteError>;
    /// `get_option( $name )` when it `is_string()`; `None` for any other value, integers included.
    fn get_string(&self, name: &str) -> Result<Option<String>, RewriteError>;
    /// `get_option( 'rewrite_rules' )` as rules; `None` when it is empty or not an array.
    fn rules(&self) -> Result<Option<Vec<Rule>>, RewriteError>;
    fn set_rules(&self, rules: &[Rule]) -> Result<(), RewriteError>;
    fn delete_rules(&self) -> Result<(), RewriteError>;
    fn update(&self, name: &str, value: &str) -> Result<(), RewriteError>;
}

/// [`RandomContentRedirects`] over the Hook API: `(bool) apply_filters( 'wp_enable_random_content_redirect', true )`,
/// as query.php has it.
pub struct FilteredRandomContentRedirects {
    hooks: Rc<WpHookApi>,
}

impl FilteredRandomContentRedirects {
    pub fn new(hooks: Rc<WpHookApi>) -> FilteredRandomContentRedirects {
        FilteredRandomContentRedirects { hooks }
    }
}

impl RandomContentRedirects for FilteredRandomContentRedirects {
    fn enabled(&self) -> Result<bool, RewriteError> {
        let enabled = self.hooks.apply_filters(
            &HookName::from("wp_enable_random_content_redirect"),
            HookValue::Bool(true),
            vec![],
        )?;
        Ok(php_bool(&enabled.read()))
    }
}

/// PHP's `(bool)` cast.
fn php_bool(value: &HookValue) -> bool {
    match value {
        HookValue::Null | HookValue::Bool(false) => false,
        HookValue::Bool(true) | HookValue::Opaque(_) => true,
        HookValue::Int(i) => *i != 0,
        HookValue::Float(f) => *f != 0.0,
        HookValue::Str(s) => !s.is_empty() && s != "0",
        HookValue::Bytes(b) => !b.is_empty() && b.as_slice() != b"0",
        HookValue::Array(items) => !items.is_empty(),
    }
}

/// [`RewriteOptions`] over the options contracts (Adapter).
pub struct OptionsRewriteOptions {
    api: Rc<dyn Options>,
}

impl OptionsRewriteOptions {
    pub fn new(api: Rc<dyn Options>) -> OptionsRewriteOptions {
        OptionsRewriteOptions { api }
    }
}

fn options_error(err: impl std::fmt::Display) -> RewriteError {
    RewriteError::new(err.to_string())
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

    fn get_string(&self, name: &str) -> Result<Option<String>, RewriteError> {
        let value = self
            .api
            .get_option(name, None)
            .map_err(options_error)?
            .read()
            .into_owned();
        Ok(match value {
            HookValue::Str(s) => Some(s),
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
