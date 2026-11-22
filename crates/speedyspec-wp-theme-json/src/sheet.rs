//! `Sheet`: one document with everything the stylesheet pieces read (its data, the ports, the registered blocks
//! and their computed metadata), standing in for `$this` in `WP_Theme_JSON`'s stylesheet methods.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;

use crate::ThemeJsonDeps;
use crate::declarations::DeclarationContext;
use crate::presets::FluidFontSize;

/// One document, ready for stylesheet generation.
pub struct Sheet<'a> {
    /// The document (`$this->theme_json`).
    pub doc: &'a Value,
    /// The ports.
    pub deps: &'a ThemeJsonDeps,
    /// Fluid font sizes over the ports.
    pub fluid: FluidFontSize,
    /// The registered blocks.
    pub blocks: Rc<Vec<RegisteredBlock>>,
    /// The blocks' computed metadata (`get_blocks_metadata()`).
    pub meta: Rc<Value>,
}

impl<'a> Sheet<'a> {
    /// A sheet for `doc`, reading the registered blocks and their metadata now.
    pub fn new(doc: &'a Value, deps: &'a ThemeJsonDeps, meta: Rc<Value>) -> Sheet<'a> {
        Sheet {
            doc,
            deps,
            fluid: FluidFontSize {
                global: deps.global_settings.clone(),
                diagnostics: deps.diagnostics.clone(),
            },
            blocks: deps.blocks.registered(),
            meta,
        }
    }

    /// What computing declarations needs.
    pub fn decl(&self) -> DeclarationContext<'_> {
        DeclarationContext {
            fluid: &self.fluid,
            diagnostics: &*self.deps.diagnostics,
            gettext: &*self.deps.gettext,
        }
    }

    /// `$this->theme_json['settings'] ?? array()`.
    pub fn settings(&self) -> Value {
        match crate::array_path::get(self.doc, &["settings"]) {
            None | Some(Value::Null) => Value::Array(Vec::new()),
            Some(s) => s.clone(),
        }
    }

    /// `isset( settings.useRootPaddingAwareAlignments ) && true === …`.
    pub fn use_root_padding(&self) -> bool {
        matches!(
            crate::array_path::get(self.doc, &["settings", "useRootPaddingAwareAlignments"]),
            Some(Value::Bool(true))
        )
    }

    /// `is_safe_css_declaration( $property_name, $property_value )`: kses keeps `name: value`.
    pub fn is_safe_css_declaration(&self, name: &str, value: &Value) -> bool {
        crate::security::is_safe_css_declaration(
            name,
            value,
            &*self.deps.safe_css,
            &*self.deps.text,
        )
    }

    /// The registered block of this name.
    pub fn block(&self, name: &str) -> Option<&RegisteredBlock> {
        self.blocks.iter().find(|b| b.name == name)
    }
}
