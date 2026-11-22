//! WordPress's theme.json classes (`WP_Theme_JSON`, `WP_Theme_JSON_Data`, `WP_Theme_JSON_Schema`) as thin PHP
//! classes over the Rust engine.
//!
//! Each `WP_Theme_JSON` object holds a handle into a per-request [`HandleArena`]; its methods are native functions
//! that dispatch through the container to [`ThemeJsonApi`]. The ports are adapted onto WordPress's PHP.

mod adapters;
mod arena;
mod literal;
mod native;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_theme_json::filters::HookApiFilters;
use speedyspec_wp_theme_json::{ThemeJsonApi, ThemeJsonDeps, register_theme_json_handlers};
use speedyspec_wp_theme_json_contracts::schema;

pub use adapters::{
    PhpActiveTheme, PhpBlockRegistry, PhpCapabilities, PhpDiagnostics, PhpGettext,
    PhpGlobalSettings, PhpSafeCss, PhpTextFunctions,
};
pub use arena::HandleArena;
pub use literal::php_literal;

const FACADE: &str = include_str!("theme-json.php");

/// `WP_Theme_JSON_Schema::V1_TO_V2_RENAMED_PATHS`.
const SCHEMA_CONSTANTS: &str = "const V1_TO_V2_RENAMED_PATHS = array( 'border.customRadius' => 'border.radius', \
    'spacing.customMargin' => 'spacing.margin', 'spacing.customPadding' => 'spacing.padding', \
    'typography.customLineHeight' => 'typography.lineHeight' );";

/// The classes' PHP source, with WordPress's constants written in.
pub fn facade_source() -> String {
    let constants = schema::constants();
    let mut lines = Vec::new();
    if let speedyspec_hooks::Value::Array(entries) = &*constants {
        for (key, value) in entries {
            if let speedyspec_hooks::Key::Str(name) = key {
                lines.push(format!("const {name} = {};", php_literal(value)));
            }
        }
    }
    FACADE
        .replace("/*CONSTANTS*/", &lines.join("\n        "))
        .replace("/*SCHEMA_CONSTANTS*/", SCHEMA_CONSTANTS)
}

/// The theme.json area: the native functions and the classes over them.
pub struct ThemeJsonArea {
    api: Rc<ThemeJsonApi>,
    handlers: Rc<native::Handlers>,
    blocks: Rc<PhpBlockRegistry>,
    source: String,
}

impl ThemeJsonArea {
    /// The area, with every port adapted onto WordPress's PHP.
    pub fn new(container: Rc<Container>, hooks: Rc<HookApi>) -> Self {
        let blocks = Rc::new(PhpBlockRegistry::default());
        let deps = ThemeJsonDeps::new(
            blocks.clone(),
            Rc::new(PhpTextFunctions),
            Rc::new(PhpGettext),
            Rc::new(PhpDiagnostics),
            Rc::new(PhpSafeCss),
        )
        .with_global_settings(Rc::new(PhpGlobalSettings))
        .with_filters(Rc::new(HookApiFilters::new(hooks)))
        .with_theme(Rc::new(PhpActiveTheme))
        .with_capabilities(Rc::new(PhpCapabilities));
        let api = Rc::new(ThemeJsonApi::new(deps));
        register_theme_json_handlers(&container, api.clone());
        Self {
            api,
            handlers: Rc::new(native::Handlers::new(&container)),
            blocks,
            source: facade_source(),
        }
    }

    /// The engine behind the classes.
    pub fn api(&self) -> Rc<ThemeJsonApi> {
        self.api.clone()
    }
}

impl NativeArea for ThemeJsonArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions(self.handlers.clone())
    }

    fn request_start(&self) {
        self.blocks.reset();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-theme-json", &self.source) {
            eprintln!("speedyspec_wordpress: installing the theme.json classes failed: {err}");
        }
    }

    fn request_end(&self) {
        HandleArena::clear();
    }
}

/// The compat runner for the `theme-json` area on the port's side: `compat/theme-json/setup.php` with the native
/// classes loaded and WordPress's theme.json files not.
pub fn theme_json_scenario_runner(
    php: speedyspec_php::Php,
    release: &std::path::Path,
) -> speedyspec_compat::PhpRunner {
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); \
         define('SPEEDYSPEC_COMPAT_MULTISITE', false); require {:?};",
        format!("{}/", release.display()),
        speedyspec_compat::compat_dir()
            .join("theme-json/setup.php")
            .display()
            .to_string()
    );
    speedyspec_compat::PhpRunner::new(php, move |php| {
        php.eval("speedyspec://compat/theme-json-setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}
