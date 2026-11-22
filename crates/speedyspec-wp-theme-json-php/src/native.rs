//! The native functions `theme-json.php` calls, one per method, each through a handler resolved once.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_theme_json::messages::{
    ConstructThemeJson, GetCustomCss, GetCustomTemplates, GetData, GetElementClassName,
    GetFromEditorSettings, GetPatterns, GetRawData, GetRootLayoutRules, GetSettings,
    GetStylesBlockNodes, GetStylesForBlock, GetStylesheet, GetSvgFilters, GetTemplateParts,
    GetViewportMediaQueries, MergeRawData, RemoveInsecureProperties, ResolveVariables,
    SchemaMigrate, SetSpacingSizes,
};
use speedyspec_wp_theme_json_contracts::model::ThemeJson;
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;

use crate::arena::HandleArena;

const NATIVE: &str = "SpeedySpec\\WpThemeJson\\Native\\";

/// The handlers every native function dispatches through, each resolved once.
pub(crate) struct Handlers {
    construct: QueryRef<ConstructThemeJson>,
    merge: QueryRef<MergeRawData>,
    raw: QueryRef<GetRawData>,
    settings: QueryRef<GetSettings>,
    data: QueryRef<GetData>,
    templates: QueryRef<GetCustomTemplates>,
    parts: QueryRef<GetTemplateParts>,
    patterns: QueryRef<GetPatterns>,
    editor: QueryRef<GetFromEditorSettings>,
    element: QueryRef<GetElementClassName>,
    migrate: QueryRef<SchemaMigrate>,
    viewport: QueryRef<GetViewportMediaQueries>,
    spacing: HandlerRef<SetSpacingSizes>,
    stylesheet: QueryRef<GetStylesheet>,
    block_styles: QueryRef<GetStylesForBlock>,
    block_nodes: QueryRef<GetStylesBlockNodes>,
    root_layout: QueryRef<GetRootLayoutRules>,
    custom_css: QueryRef<GetCustomCss>,
    insecure: QueryRef<RemoveInsecureProperties>,
    svg: QueryRef<GetSvgFilters>,
    resolve: QueryRef<ResolveVariables>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Self {
        Self {
            construct: QueryRef::new(c.clone()),
            merge: QueryRef::new(c.clone()),
            raw: QueryRef::new(c.clone()),
            settings: QueryRef::new(c.clone()),
            data: QueryRef::new(c.clone()),
            templates: QueryRef::new(c.clone()),
            parts: QueryRef::new(c.clone()),
            patterns: QueryRef::new(c.clone()),
            editor: QueryRef::new(c.clone()),
            element: QueryRef::new(c.clone()),
            migrate: QueryRef::new(c.clone()),
            viewport: QueryRef::new(c.clone()),
            spacing: HandlerRef::new(c.clone()),
            stylesheet: QueryRef::new(c.clone()),
            block_styles: QueryRef::new(c.clone()),
            block_nodes: QueryRef::new(c.clone()),
            root_layout: QueryRef::new(c.clone()),
            custom_css: QueryRef::new(c.clone()),
            insecure: QueryRef::new(c.clone()),
            svg: QueryRef::new(c.clone()),
            resolve: QueryRef::new(c.clone()),
        }
    }
}

type R = Result<Value, NativeError>;
type Body = Box<dyn Fn(&Handlers, &[Value]) -> R>;

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn ask<Q: Query>(r: &QueryRef<Q>, q: Q) -> Result<Q::Result, NativeError> {
    r.call(q).map_err(bus)
}

fn send<C: Command>(r: &HandlerRef<C>, c: C) -> Result<C::Result, NativeError> {
    r.call(c).map_err(bus)
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// An argument as bytes: strings as they are, other scalars by PHP's string conversion.
fn bytes(args: &[Value], i: usize) -> Vec<u8> {
    match arg(args, i) {
        Value::Str(s) => s.into_bytes(),
        Value::Bytes(b) => b,
        Value::Int(n) => n.to_string().into_bytes(),
        Value::Bool(true) => b"1".to_vec(),
        Value::Float(f) => speedyspec_wp_theme_json::php::float_to_string(f).into_bytes(),
        _ => Vec::new(),
    }
}

fn handle(args: &[Value]) -> i64 {
    match arg(args, 0) {
        Value::Int(h) => h,
        _ => -1,
    }
}

fn doc(args: &[Value]) -> Result<Rc<ThemeJson>, NativeError> {
    HandleArena::get(handle(args))
        .ok_or_else(|| NativeError::throw("Error", "WP_Theme_JSON: the document is gone"))
}

fn string_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

/// Every native function, dispatching through `h`.
pub(crate) fn functions(h: Rc<Handlers>) -> Vec<FunctionBuilder<'static>> {
    let f = |name: &str, body: Body| {
        let h = h.clone();
        native::function(&format!("{NATIVE}{name}"), move |args| body(&h, args))
    };
    vec![
        f(
            "construct",
            Box::new(|h, a| {
                let doc = ask(
                    &h.construct,
                    ConstructThemeJson {
                        data: arg(a, 0),
                        origin: bytes(a, 1),
                    },
                )?;
                Ok(Value::Int(HandleArena::insert(doc)))
            }),
        ),
        f(
            "restore",
            Box::new(|_, a| {
                Ok(Value::Int(HandleArena::insert(
                    speedyspec_wp_theme_json::ThemeJsonCodec::decode(&arg(a, 0)),
                )))
            }),
        ),
        f(
            "duplicate",
            Box::new(|_, a| Ok(Value::Int(HandleArena::duplicate(handle(a))))),
        ),
        f(
            "release",
            Box::new(|_, a| {
                HandleArena::release(handle(a));
                Ok(Value::Null)
            }),
        ),
        f(
            "merge",
            Box::new(|h, a| {
                let merged = ask(
                    &h.merge,
                    MergeRawData {
                        base: doc(a)?,
                        incoming: arg(a, 1),
                    },
                )?;
                HandleArena::replace(handle(a), merged);
                Ok(Value::Null)
            }),
        ),
        f(
            "get_raw_data",
            Box::new(|h, a| ask(&h.raw, GetRawData { doc: doc(a)? })),
        ),
        f(
            "get_settings",
            Box::new(|h, a| ask(&h.settings, GetSettings { doc: doc(a)? })),
        ),
        f(
            "get_data",
            Box::new(|h, a| ask(&h.data, GetData { doc: doc(a)? })),
        ),
        f(
            "get_custom_templates",
            Box::new(|h, a| ask(&h.templates, GetCustomTemplates { doc: doc(a)? })),
        ),
        f(
            "get_template_parts",
            Box::new(|h, a| ask(&h.parts, GetTemplateParts { doc: doc(a)? })),
        ),
        f(
            "get_patterns",
            Box::new(|h, a| ask(&h.patterns, GetPatterns { doc: doc(a)? })),
        ),
        f(
            "set_spacing_sizes",
            Box::new(|h, a| {
                let updated = send(&h.spacing, SetSpacingSizes { doc: doc(a)? })?;
                HandleArena::replace(handle(a), updated);
                Ok(Value::Null)
            }),
        ),
        f(
            "get_element_class_name",
            Box::new(|h, a| {
                Ok(string_value(ask(
                    &h.element,
                    GetElementClassName {
                        element: bytes(a, 0),
                    },
                )?))
            }),
        ),
        f(
            "get_from_editor_settings",
            Box::new(|h, a| {
                ask(
                    &h.editor,
                    GetFromEditorSettings {
                        settings: arg(a, 0),
                    },
                )
            }),
        ),
        f(
            "schema_migrate",
            Box::new(|h, a| {
                ask(
                    &h.migrate,
                    SchemaMigrate {
                        data: arg(a, 0),
                        origin: bytes(a, 1),
                    },
                )
            }),
        ),
        f(
            "get_viewport_media_queries",
            Box::new(|h, a| {
                let include_desktop = matches!(arg(a, 1), Value::Bool(true));
                let pairs = ask(
                    &h.viewport,
                    GetViewportMediaQueries {
                        viewport: arg(a, 0),
                        include_desktop,
                    },
                )?;
                Ok(Value::Array(
                    pairs
                        .into_iter()
                        .map(|(k, v)| (Key::Str(k), Value::Str(v)))
                        .collect(),
                ))
            }),
        ),
        f(
            "get_stylesheet",
            Box::new(|h, a| {
                let css = ask(
                    &h.stylesheet,
                    GetStylesheet {
                        doc: doc(a)?,
                        types: arg(a, 1),
                        origins: arg(a, 2),
                        options: arg(a, 3),
                    },
                )?
                .map_err(|e| NativeError::throw("Error", e.to_string()))?;
                Ok(string_value(css))
            }),
        ),
        f(
            "get_styles_for_block",
            Box::new(|h, a| {
                Ok(string_value(ask(
                    &h.block_styles,
                    GetStylesForBlock {
                        doc: doc(a)?,
                        metadata: arg(a, 1),
                    },
                )?))
            }),
        ),
        f(
            "get_styles_block_nodes",
            Box::new(|h, a| ask(&h.block_nodes, GetStylesBlockNodes { doc: doc(a)? })),
        ),
        f(
            "get_root_layout_rules",
            Box::new(|h, a| {
                Ok(string_value(ask(
                    &h.root_layout,
                    GetRootLayoutRules {
                        doc: doc(a)?,
                        metadata: arg(a, 1),
                        options: arg(a, 2),
                    },
                )?))
            }),
        ),
        f(
            "get_custom_css",
            Box::new(|h, a| {
                Ok(string_value(ask(
                    &h.custom_css,
                    GetCustomCss { doc: doc(a)? },
                )?))
            }),
        ),
        f(
            "scope_selector",
            Box::new(|_, a| {
                let scope = String::from_utf8_lossy(&bytes(a, 0)).into_owned();
                let selector = String::from_utf8_lossy(&bytes(a, 1)).into_owned();
                Ok(Value::Str(
                    speedyspec_wp_theme_json::selectors::scope_selector(&scope, &selector),
                ))
            }),
        ),
        f(
            "process_blocks_custom_css",
            Box::new(|_, a| {
                let css = String::from_utf8_lossy(&bytes(a, 0)).into_owned();
                let selector = String::from_utf8_lossy(&bytes(a, 1)).into_owned();
                Ok(Value::Str(
                    speedyspec_wp_theme_json::stylesheet::process_blocks_custom_css(
                        &css, &selector,
                    ),
                ))
            }),
        ),
        f(
            "block_css_selector",
            Box::new(|_, a| {
                let block = RegisteredBlock {
                    name: String::from_utf8_lossy(&bytes(a, 0)).into_owned(),
                    selectors: arg(a, 1),
                    supports: arg(a, 2),
                    style_names: Vec::new(),
                };
                let fallback =
                    !matches!(arg(a, 4), Value::Null | Value::Bool(false) | Value::Int(0));
                Ok(speedyspec_wp_theme_json::selectors::block_css_selector(
                    &block,
                    &arg(a, 3),
                    fallback,
                )
                .map_or(Value::Null, Value::Str))
            }),
        ),
        f(
            "remove_insecure_properties",
            Box::new(|h, a| {
                ask(
                    &h.insecure,
                    RemoveInsecureProperties {
                        data: arg(a, 0),
                        origin: arg(a, 1),
                    },
                )?
                .map_err(|e| NativeError::throw("Error", e.to_string()))
            }),
        ),
        f(
            "get_svg_filters",
            Box::new(|h, a| {
                Ok(string_value(ask(
                    &h.svg,
                    GetSvgFilters {
                        doc: doc(a)?,
                        origins: arg(a, 1),
                    },
                )?))
            }),
        ),
        f(
            "resolve_variables",
            Box::new(|h, a| {
                let resolved = ask(&h.resolve, ResolveVariables { doc: doc(a)? })?;
                HandleArena::replace(handle(a), resolved);
                Ok(Value::Null)
            }),
        ),
    ]
}
