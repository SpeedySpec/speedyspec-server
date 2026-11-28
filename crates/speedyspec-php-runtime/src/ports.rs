//! The worker's WordPress port areas beyond the database ones: text, content and the transports (Composition Root).
//!
//! WordPress never runs in the runtime, so every port's PHP functions and classes are the only declarations of their
//! names: each area is registered in every worker. The areas take contracts only; this module is where the concrete
//! adapters are chosen.
//!
//! - [`text_areas`]: UTF-8, `WP_Token_Map`, i18n, kses, the formatting text functions, shortcodes and theme.json. kses
//!   is built first: the text functions and shortcodes take its API.
//! - [`transport_areas`]: operations, then abilities, REST and XML-RPC over them. Capabilities are WordPress's
//!   `user_can()` ([`PhpIdentity`]), sites are told apart by what PHP reports ([`PhpSiteKey`]), and bindings live for
//!   the worker ([`MemoryBindingStore`]).
//! - [`rewrite_area`]: the rewrite API, over the options contracts in a worker with a database, and over PHP's
//!   `get_option()` without one.

use std::rc::Rc;

use speedyspec_abilities_php::{AbilitiesArea, AbilitiesService};
use speedyspec_container::Container;
use speedyspec_operation_contracts::Identity;
use speedyspec_operation_php::{OperationArea, PhpIdentity, PhpSiteKey};
use speedyspec_php::NativeArea;
use speedyspec_rest_php::{RestArea, RestService};
use speedyspec_transport::MemoryBindingStore;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_i18n_php::I18nArea;
use speedyspec_wp_kses_php::KsesArea;
use speedyspec_wp_options_contracts::Options;
use speedyspec_wp_rewrite::{OptionsRewriteOptions, RewriteOptions};
use speedyspec_wp_rewrite_php::{PhpRewriteOptions, RewriteArea};
use speedyspec_wp_shortcode_php::ShortcodeArea;
use speedyspec_wp_text_php::TextArea;
use speedyspec_wp_theme_json_php::ThemeJsonArea;
use speedyspec_wp_token_map_php::TokenMapArea;
use speedyspec_wp_utf8_php::Utf8Area;
use speedyspec_xmlrpc_php::{XmlRpcArea, XmlRpcService};

/// UTF-8, `WP_Token_Map`, i18n, kses, the formatting text functions, shortcodes and theme.json.
pub fn text_areas(container: &Rc<Container>, hooks: &Rc<WpHookApi>) -> Vec<Rc<dyn NativeArea>> {
    let kses = KsesArea::new(container.clone(), hooks.clone());
    let kses_api = kses.api();
    let text = TextArea::new(container.clone(), hooks.clone(), kses_api.clone());
    let shortcode = ShortcodeArea::new(container.clone(), hooks.clone(), kses_api);
    vec![
        Rc::new(Utf8Area),
        Rc::new(TokenMapArea),
        Rc::new(I18nArea::new(container.clone(), hooks.clone())),
        Rc::new(kses),
        Rc::new(text),
        Rc::new(shortcode),
        Rc::new(ThemeJsonArea::new(container.clone(), hooks.clone())),
    ]
}

/// The REST base URL: WordPress's `rest_url()`, without its trailing slash, read when a site's REST server is built.
fn rest_base_url() -> String {
    match speedyspec_php::call(&speedyspec_php_ext::Value::Str("rest_url".into()), &[]) {
        Ok(speedyspec_php_ext::Value::Str(url)) => url.trim_end_matches('/').to_owned(),
        _ => "/wp-json".to_owned(),
    }
}

/// Operations, and abilities, REST and XML-RPC over them.
pub fn transport_areas(container: &Rc<Container>) -> Vec<Rc<dyn NativeArea>> {
    let identity: Rc<dyn Identity> = Rc::new(PhpIdentity::new());
    let bindings = Rc::new(MemoryBindingStore::new());
    let operations = Rc::new(OperationArea::new(
        container.clone(),
        identity.clone(),
        Rc::new(PhpSiteKey),
    ));
    let abilities = AbilitiesService::new(
        container.clone(),
        operations.clone(),
        bindings.clone(),
        identity.clone(),
    );
    let rest = RestService::new(
        container.clone(),
        operations.clone(),
        bindings.clone(),
        identity.clone(),
        Rc::new(rest_base_url),
    );
    let xmlrpc = XmlRpcService::new(
        container.clone(),
        operations.clone(),
        bindings.clone(),
        identity,
    );
    vec![
        operations,
        Rc::new(AbilitiesArea::new(bindings.clone()).with_service(Rc::new(abilities))),
        Rc::new(RestArea::new(bindings.clone()).with_service(Rc::new(rest))),
        Rc::new(XmlRpcArea::new(bindings).with_service(Rc::new(xmlrpc))),
    ]
}

/// The rewrite API, over `options` (the database's options contracts) or, without a database, PHP's `get_option()`.
pub fn rewrite_area(
    container: &Rc<Container>,
    hooks: &Rc<WpHookApi>,
    options: Option<Rc<dyn Options>>,
) -> RewriteArea {
    let options: Rc<dyn RewriteOptions> = match options {
        Some(options) => Rc::new(OptionsRewriteOptions::new(options)),
        None => Rc::new(PhpRewriteOptions),
    };
    RewriteArea::new(container.clone(), hooks.clone(), options)
}
