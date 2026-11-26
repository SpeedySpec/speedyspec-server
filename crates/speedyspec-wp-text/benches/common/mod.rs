//! What the text benches share: Criterion's measurement as thread CPU time, and a `TextApi` on the adapters the
//! corpus was recorded with.

use std::rc::Rc;
use std::time::Duration;

use criterion::measurement::{Measurement, ValueFormatter, WallTime};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_shortcode_contracts::TagName;
use speedyspec_wp_text::{TextApi, TextDeps, adapters};
use speedyspec_wp_text_memory::{
    EnglishTranslator, FixedShortcodeTags, FixedSiteSettings, FixedTexturizeOverrides, RecordingDiagnostics,
};

/// The calling thread's CPU time.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes one timespec through a valid pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Criterion's measurement as thread CPU time.
pub struct CpuTime;

static WALL: WallTime = WallTime;

impl Measurement for CpuTime {
    type Intermediate = Duration;
    type Value = Duration;

    fn start(&self) -> Duration {
        thread_cpu()
    }

    fn end(&self, start: Duration) -> Duration {
        thread_cpu() - start
    }

    fn add(&self, a: &Duration, b: &Duration) -> Duration {
        *a + *b
    }

    fn zero(&self) -> Duration {
        Duration::ZERO
    }

    fn to_f64(&self, value: &Duration) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        WALL.formatter()
    }
}


/// A `TextApi` on the adapters the corpus was recorded with.
pub fn api() -> TextApi {
    let tags = ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect();
    let hooks = Rc::new(WpHookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations)));
    let settings = Rc::new(FixedSiteSettings::default());
    let diagnostics = Rc::new(RecordingDiagnostics::default());
    let escape = Rc::new(speedyspec_wp_text_escape::EscapeApi::new(hooks.clone(), settings.clone(), diagnostics.clone()));
    let kses = adapters::kses_for(hooks.clone(), escape.clone());
    TextApi::new(TextDeps {
        hooks,
        settings,
        translator: Rc::new(EnglishTranslator),
        diagnostics,
        texturize_overrides: Rc::new(FixedTexturizeOverrides::default()),
        shortcode_tags: Rc::new(FixedShortcodeTags(tags)),
        escape,
        kses: kses.clone(),
        locale: Rc::new(speedyspec_wp_text_memory::FixedLocale::default()),
        mime_types: Rc::new(adapters::FixedMimeTypes::wordpress()),
        random: Rc::new(adapters::PhpMtRand::seeded(1)),
        smilies: Rc::new(adapters::FixedSmilies::wordpress(b"https://example.org/wp-includes/", b"http://example.org")),
        link_context: Rc::new(adapters::HostLinkContext::new(kses, b"http://example.org")),
    })
}
