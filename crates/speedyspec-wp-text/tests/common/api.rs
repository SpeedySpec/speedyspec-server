//! A `TextApi` on in-memory adapters, as the corpus was recorded.

#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_shortcode_contracts::{ShortcodeTagNames, TagName};
use speedyspec_wp_kses::KsesApi;
use speedyspec_wp_text::adapters::{FixedMimeTypes, FixedSmilies, HostLinkContext, PhpMtRand, kses_for};
use speedyspec_wp_text::{TextApi, TextDeps};
use speedyspec_wp_text_contracts::{RandomSource, Smilies, Translator};
use speedyspec_wp_text_escape::EscapeApi;
use speedyspec_wp_text_memory::{
    EnglishTranslator, FixedLocale, FixedShortcodeTags, FixedSiteSettings, FixedTexturizeOverrides, RecordingDiagnostics,
};

/// The home URL the corpus was recorded with (setup.php's home_url()).
pub const HOME: &[u8] = b"http://example.org";
/// setup.php's includes_url().
pub const INCLUDES: &[u8] = b"https://example.org/wp-includes/";

/// The shortcodes the corpus was recorded with, in WordPress's registration order.
pub fn tags() -> Vec<TagName> {
    ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect()
}

pub struct Harness {
    pub hooks: Rc<HookApi>,
    pub diagnostics: Rc<RecordingDiagnostics>,
    pub kses: Rc<KsesApi>,
    pub api: Rc<TextApi>,
}

/// What a test may replace in the harness.
pub struct Options {
    pub settings: FixedSiteSettings,
    pub tags: Rc<dyn ShortcodeTagNames>,
    pub translator: Rc<dyn Translator>,
    pub random: Rc<dyn RandomSource>,
    pub smilies: Option<Rc<dyn Smilies>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            settings: FixedSiteSettings::default(),
            tags: Rc::new(FixedShortcodeTags(tags())),
            translator: Rc::new(EnglishTranslator),
            random: Rc::new(PhpMtRand::seeded(1)),
            smilies: None,
        }
    }
}

/// A harness with the given options, on the adapters the corpus was recorded with.
pub fn harness_options(options: Options) -> Harness {
    let hooks = Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)));
    let diagnostics = Rc::new(RecordingDiagnostics::default());
    let settings = Rc::new(options.settings);
    let escape = Rc::new(EscapeApi::new(hooks.clone(), settings.clone(), diagnostics.clone()));
    let kses = kses_for(hooks.clone(), escape.clone());
    let smilies = options.smilies.unwrap_or_else(|| Rc::new(FixedSmilies::wordpress(INCLUDES, HOME)));
    let api = Rc::new(TextApi::new(TextDeps {
        hooks: hooks.clone(),
        settings,
        translator: options.translator,
        diagnostics: diagnostics.clone(),
        texturize_overrides: Rc::new(FixedTexturizeOverrides::default()),
        shortcode_tags: options.tags,
        escape,
        kses: kses.clone(),
        locale: Rc::new(FixedLocale::default()),
        mime_types: Rc::new(FixedMimeTypes::wordpress()),
        random: options.random,
        smilies,
        link_context: Rc::new(HostLinkContext::new(kses.clone(), HOME)),
    }));
    Harness { hooks, diagnostics, kses, api }
}

pub fn harness_with(settings: FixedSiteSettings, tags: Rc<dyn ShortcodeTagNames>) -> Harness {
    harness_full(settings, tags, Rc::new(EnglishTranslator))
}

pub fn harness_full(
    settings: FixedSiteSettings,
    tags: Rc<dyn ShortcodeTagNames>,
    translator: Rc<dyn Translator>,
) -> Harness {
    harness_options(Options { settings, tags, translator, ..Options::default() })
}

pub fn harness() -> Harness {
    harness_with(FixedSiteSettings::default(), Rc::new(FixedShortcodeTags(tags())))
}
