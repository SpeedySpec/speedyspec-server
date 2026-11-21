//! A `TextApi` on in-memory adapters, as the corpus was recorded.

#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_shortcode_contracts::{ShortcodeTagNames, TagName};
use speedyspec_wp_text::{TextApi, TextDeps};
use speedyspec_wp_text_contracts::Translator;
use speedyspec_wp_text_memory::{
    EnglishTranslator, FixedShortcodeTags, FixedSiteSettings, FixedTexturizeOverrides, RecordingDiagnostics,
};

/// The shortcodes the corpus was recorded with, in WordPress 7.1.5's registration order.
pub fn tags() -> Vec<TagName> {
    ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect()
}

pub struct Harness {
    pub hooks: Rc<HookApi>,
    pub diagnostics: Rc<RecordingDiagnostics>,
    pub api: Rc<TextApi>,
}

pub fn harness_with(settings: FixedSiteSettings, tags: Rc<dyn ShortcodeTagNames>) -> Harness {
    harness_full(settings, tags, Rc::new(EnglishTranslator))
}

pub fn harness_full(
    settings: FixedSiteSettings,
    tags: Rc<dyn ShortcodeTagNames>,
    translator: Rc<dyn Translator>,
) -> Harness {
    let hooks = Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)));
    let diagnostics = Rc::new(RecordingDiagnostics::default());
    let api = Rc::new(TextApi::new(TextDeps {
        hooks: hooks.clone(),
        settings: Rc::new(settings),
        translator,
        diagnostics: diagnostics.clone(),
        texturize_overrides: Rc::new(FixedTexturizeOverrides::default()),
        shortcode_tags: tags,
    }));
    Harness { hooks, diagnostics, api }
}

pub fn harness() -> Harness {
    harness_with(FixedSiteSettings::default(), Rc::new(FixedShortcodeTags(tags())))
}
