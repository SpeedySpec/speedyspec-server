//! The rewrite port's outbound ports, answered by WordPress's PHP functions (Gateway) until those areas are ported.

use std::any::Any;
use std::rc::Rc;

use speedyspec_php_ext::{Key, Value};
use speedyspec_routing::{GuardAnswer, Rule, RuleGuard};
use speedyspec_wp_hooks::{HookApi, HookName};
use speedyspec_wp_rewrite::{
    Multisite, PageUris, PostTypes, PublicQueryVars, QueryableTaxonomy, RandomContentRedirects,
    RandomablePostType, RewriteError, SiteTranslations, SiteUrls, Taxonomies, UriIndex,
    ViewablePosts, WpLoaded,
};

use crate::values::{items, key_text, list, text, truthy};

fn call(function: &str, args: &[Value]) -> Value {
    match speedyspec_php::call(&Value::Str(function.into()), args) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("speedyspec_wordpress: {function}() failed: {err}");
            Value::Null
        }
    }
}

/// `home_url()` and `site_url()`.
pub struct PhpSiteUrls;

impl SiteUrls for PhpSiteUrls {
    fn home_url(&self) -> String {
        text(&call("home_url", &[]))
    }

    fn site_url(&self) -> String {
        text(&call("site_url", &[]))
    }
}

/// `is_multisite() && is_main_site()`.
pub struct PhpMultisite;

impl Multisite for PhpMultisite {
    fn is_multisite_main_site(&self) -> bool {
        truthy(&call("is_multisite", &[])) && truthy(&call("is_main_site", &[]))
    }
}

/// `did_action( 'wp_loaded' )`, answered by the Rust Hook API both languages share.
pub struct HookWpLoaded {
    hooks: Rc<HookApi>,
}

impl HookWpLoaded {
    pub fn new(hooks: Rc<HookApi>) -> HookWpLoaded {
        HookWpLoaded { hooks }
    }
}

impl WpLoaded for HookWpLoaded {
    fn did_wp_loaded(&self) -> bool {
        self.hooks
            .did_action(&HookName::from("wp_loaded"))
            .unwrap_or(0)
            > 0
    }
}

/// `$wp_rewrite->page_uri_index()`, which stays PHP until posts are ported.
pub struct PhpPageUris;

impl PageUris for PhpPageUris {
    fn page_uris(&self) -> (UriIndex, UriIndex) {
        let value = speedyspec_php::eval(
            "speedyspec://wp-rewrite-pages",
            "return isset($GLOBALS['wp_rewrite']) ? $GLOBALS['wp_rewrite']->page_uri_index() : array(array(), array());",
        )
        .unwrap_or(Value::Null);
        let halves: Vec<UriIndex> = items(&value)
            .into_iter()
            .map(|(_, half)| {
                items(&half)
                    .iter()
                    .map(|(k, v)| (key_text(k), text(v)))
                    .collect()
            })
            .collect();
        let mut halves = halves.into_iter();
        (
            halves.next().unwrap_or_default(),
            halves.next().unwrap_or_default(),
        )
    }
}

/// A failed PHP call as a [`RewriteError`]: the exception it threw is the error's cause, so the native functions
/// rethrow it unchanged, as they do a hook callback's.
fn caught(what: &str, failure: speedyspec_php::CallFailure) -> RewriteError {
    RewriteError {
        message: format!("{what} failed: {}", failure.error),
        cause: failure
            .exception
            .map(|exception| Rc::new(exception) as Rc<dyn Any>),
    }
}

/// Runs `code`, which returns a static closure, with `args`. What it throws (WordPress's filters and functions it
/// calls) propagates as the error's cause, as it does out of WordPress's `rewrite_rules()`.
fn run(name: &str, code: &str, args: &[Value]) -> Result<Value, RewriteError> {
    let callback = speedyspec_php::eval(name, code)
        .map_err(|err| RewriteError::new(format!("{name} failed: {err}")))?;
    speedyspec_php::call_catching(&callback, args).map_err(|failure| caught(name, failure))
}

/// A PHP string, or `null`, as an option.
fn optional_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => Some(text(other)),
    }
}

/// Post types through `get_post_types()`, which stays PHP until post types are wired to Rust.
pub struct PhpPostTypes;

impl PostTypes for PhpPostTypes {
    fn custom_post_types(&self) -> Vec<(String, bool)> {
        let args = Value::Array(vec![(Key::Str("_builtin".into()), Value::Bool(false))]);
        items(&call("get_post_types", &[args]))
            .into_iter()
            .map(|(_, name)| {
                let name = text(&name);
                let hierarchical = truthy(&call(
                    "is_post_type_hierarchical",
                    &[Value::Str(name.clone())],
                ));
                (name, hierarchical)
            })
            .collect()
    }

    fn randomable_post_types(&self) -> Result<Vec<RandomablePostType>, RewriteError> {
        let code = r#"return static function () {
            $types = array();
            foreach (get_post_types(array('randomable' => true), 'objects') as $type) {
                $rewrite = $type->rewrite;
                $types[] = array(
                    (string) $type->name,
                    is_string($type->has_archive) ? $type->has_archive : null,
                    is_array($rewrite),
                    is_array($rewrite) && isset($rewrite['slug']) && is_string($rewrite['slug']) ? $rewrite['slug'] : null,
                );
            }
            return $types;
        };"#;
        Ok(list(&run("speedyspec://wp-rewrite-randomable", code, &[])?)
            .iter()
            .map(|entry| {
                let fields = list(entry);
                let at = |i: usize| fields.get(i).cloned().unwrap_or(Value::Null);
                RandomablePostType {
                    name: text(&at(0)),
                    archive_slug: optional_text(&at(1)),
                    rewrites: truthy(&at(2)),
                    rewrite_slug: optional_text(&at(3)),
                }
            })
            .collect())
    }
}

/// `get_taxonomies( array( 'publicly_queryable' => true ), 'objects' )`, which stays PHP until taxonomies are ported.
pub struct PhpTaxonomies;

impl Taxonomies for PhpTaxonomies {
    fn publicly_queryable(&self) -> Result<Vec<QueryableTaxonomy>, RewriteError> {
        let code = r#"return static function () {
            $taxonomies = array();
            foreach (get_taxonomies(array('publicly_queryable' => true), 'objects') as $taxonomy) {
                $rewrite = $taxonomy->rewrite;
                $taxonomies[] = array(
                    (string) $taxonomy->name,
                    is_array($rewrite) && isset($rewrite['slug']) && is_string($rewrite['slug']) ? $rewrite['slug'] : null,
                    array_values(array_map('strval', (array) $taxonomy->object_type)),
                );
            }
            return $taxonomies;
        };"#;
        Ok(list(&run("speedyspec://wp-rewrite-taxonomies", code, &[])?)
            .iter()
            .map(|entry| {
                let fields = list(entry);
                let at = |i: usize| fields.get(i).cloned().unwrap_or(Value::Null);
                QueryableTaxonomy {
                    name: text(&at(0)),
                    rewrite_slug: optional_text(&at(1)),
                    object_types: list(&at(2)).iter().map(text).collect(),
                }
            })
            .collect())
    }
}

/// `get_page_by_path( $path, OBJECT, $post_type )` and `is_post_publicly_viewable()`, which stay PHP until posts are
/// ported.
pub struct PhpViewablePosts;

impl ViewablePosts for PhpViewablePosts {
    fn viewable_at(&self, path: &str, post_type: &str) -> Result<bool, RewriteError> {
        let code = r#"return static function ($path, $post_type) {
            $post = get_page_by_path($path, OBJECT, $post_type);
            return $post instanceof WP_Post && is_post_publicly_viewable($post);
        };"#;
        Ok(truthy(&run(
            "speedyspec://wp-rewrite-viewable-post",
            code,
            &[Value::Str(path.into()), Value::Str(post_type.into())],
        )?))
    }
}

/// `wp_is_random_content_redirect_enabled()`, from query.php, which stays PHP until queries are ported. An exception
/// it throws is the error's cause, so the native functions rethrow it unchanged, as they do a hook callback's.
pub struct PhpRandomContentRedirects;

impl RandomContentRedirects for PhpRandomContentRedirects {
    fn enabled(&self) -> Result<bool, RewriteError> {
        speedyspec_php::call_catching(
            &Value::Str("wp_is_random_content_redirect_enabled".into()),
            &[],
        )
        .map(|enabled| truthy(&enabled))
        .map_err(|failure| caught("wp_is_random_content_redirect_enabled()", failure))
    }
}

/// The default random base in the site language, through WordPress's i18n functions, which stay PHP until i18n is
/// ported.
pub struct PhpSiteTranslations;

impl SiteTranslations for PhpSiteTranslations {
    fn random_base(&self) -> Result<String, RewriteError> {
        let code = r#"return static function () {
            $switched_locale = switch_to_locale(get_locale());
            $random_base = _x('random', 'random content permalink base');
            if ($switched_locale) {
                restore_previous_locale();
            }
            return $random_base;
        };"#;
        Ok(text(&run(
            "speedyspec://wp-rewrite-random-base",
            code,
            &[],
        )?))
    }
}

/// `$wp->add_query_var()`, when `$wp` exists.
pub struct PhpQueryVars;

impl PublicQueryVars for PhpQueryVars {
    fn add_query_var(&self, name: &str) {
        let code = "return static function ($qv) { if (isset($GLOBALS['wp'])) { $GLOBALS['wp']->add_query_var($qv); } };";
        if let Ok(callback) = speedyspec_php::eval("speedyspec://wp-rewrite-query-var", code)
            && let Err(err) = speedyspec_php::call(&callback, &[Value::Str(name.into())])
        {
            eprintln!("speedyspec_wordpress: \\$wp->add_query_var() failed: {err}");
        }
    }
}

/// WordPress's verbose page check, in PHP: a rule whose query sets `pagename=$matches[N]` matches only when that
/// page exists with a status that is public, protected or private, or not excluded from search.
pub struct PhpPageByPath;

impl RuleGuard for PhpPageByPath {
    fn check(&self, rule: &Rule, captures: &[Option<String>]) -> GuardAnswer {
        let Some(index) = verbose_page_index(&rule.query) else {
            return GuardAnswer::Accept;
        };
        let path = captures.get(index).cloned().flatten().unwrap_or_default();
        let code = r#"return static function ($path) {
            if (empty($GLOBALS['wp_rewrite']->use_verbose_page_rules)) { return true; }
            $page = get_page_by_path($path);
            if (!$page) { return false; }
            $status = get_post_status_object($page->post_status);
            return !(!$status->public && !$status->protected && !$status->private && $status->exclude_from_search);
        };"#;
        let accepted = speedyspec_php::eval("speedyspec://wp-rewrite-page-guard", code)
            .and_then(|check| speedyspec_php::call(&check, &[Value::Str(path)]));
        match accepted {
            Ok(value) if truthy(&value) => GuardAnswer::Accept,
            Ok(_) => GuardAnswer::Reject,
            Err(err) => {
                eprintln!("speedyspec_wordpress: the verbose page check failed: {err}");
                GuardAnswer::Reject
            }
        }
    }
}

/// `preg_match( '/pagename=\$matches\[([0-9]+)\]/', $query, $varmatch )`'s capture.
pub fn verbose_page_index(query: &str) -> Option<usize> {
    let start = query.find("pagename=$matches[")? + "pagename=$matches[".len();
    let digits: String = query[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() || !query[start + digits.len()..].starts_with(']') {
        return None;
    }
    digits.parse().ok()
}

/// The rewrite options through WordPress's PHP option functions (`get_option()` and the rest), for a worker whose
/// options are not wired to Rust yet: whatever answers those functions (the options port's native functions, or
/// WordPress's own) answers the rewrite API too.
pub struct PhpRewriteOptions;

fn option_error(
    function: &str,
    err: impl std::fmt::Display,
) -> speedyspec_wp_rewrite::RewriteError {
    speedyspec_wp_rewrite::RewriteError::new(format!("{function}() failed: {err}"))
}

fn php(function: &str, args: &[Value]) -> Result<Value, speedyspec_wp_rewrite::RewriteError> {
    speedyspec_php::call(&Value::Str(function.into()), args)
        .map_err(|err| option_error(function, err))
}

impl speedyspec_wp_rewrite::RewriteOptions for PhpRewriteOptions {
    fn get(&self, name: &str) -> Result<Option<String>, speedyspec_wp_rewrite::RewriteError> {
        Ok(match php("get_option", &[Value::Str(name.into())])? {
            Value::Str(s) => Some(s),
            Value::Int(i) => Some(i.to_string()),
            _ => None,
        })
    }

    fn get_string(
        &self,
        name: &str,
    ) -> Result<Option<String>, speedyspec_wp_rewrite::RewriteError> {
        Ok(match php("get_option", &[Value::Str(name.into())])? {
            Value::Str(s) => Some(s),
            Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            _ => None,
        })
    }

    fn rules(
        &self,
    ) -> Result<Option<Vec<speedyspec_routing::Rule>>, speedyspec_wp_rewrite::RewriteError> {
        let value = php("get_option", &[Value::Str("rewrite_rules".into())])?;
        Ok(match &value {
            Value::Array(entries) if !entries.is_empty() => Some(crate::values::rules(&value)),
            _ => None,
        })
    }

    fn set_rules(
        &self,
        rules: &[speedyspec_routing::Rule],
    ) -> Result<(), speedyspec_wp_rewrite::RewriteError> {
        php(
            "update_option",
            &[
                Value::Str("rewrite_rules".into()),
                crate::values::rules_value(rules),
            ],
        )?;
        Ok(())
    }

    fn delete_rules(&self) -> Result<(), speedyspec_wp_rewrite::RewriteError> {
        php("delete_option", &[Value::Str("rewrite_rules".into())])?;
        Ok(())
    }

    fn update(&self, name: &str, value: &str) -> Result<(), speedyspec_wp_rewrite::RewriteError> {
        php(
            "update_option",
            &[Value::Str(name.into()), Value::Str(value.into())],
        )?;
        Ok(())
    }
}
