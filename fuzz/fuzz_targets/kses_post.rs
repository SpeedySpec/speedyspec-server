//! `wp_kses_post()` on arbitrary bytes: it must return, stay within five times the input (`&` → `&amp;`), and never
//! leave an exploit the security oracle can find. Seed it with `../crates/speedyspec-wp-kses-testkit/corpus/`.
#![no_main]

use std::rc::Rc;

use libfuzzer_sys::fuzz_target;
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_kses::{DefaultGlobals, KsesApi, KsesDeps, NoValueCallbacks, UploadsLocation};
use speedyspec_wp_kses_contracts::{
    AllowedSpec, AttributeEscaper, Context, KsesError, UnfilteredHtmlPolicy,
};

/// `esc_attr()` without entity normalization: enough for the fuzzer, which checks safety, not PHP equality.
struct Escaper;
impl AttributeEscaper for Escaper {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, KsesError> {
        let mut out = Vec::with_capacity(v.len());
        for b in v {
            match b {
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#039;"),
                b'&' => out.extend_from_slice(b"&amp;"),
                b => out.push(*b),
            }
        }
        Ok(out)
    }
}

struct Policy;
impl UnfilteredHtmlPolicy for Policy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(false)
    }
}

struct Uploads;
impl UploadsLocation for Uploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        Ok(b"http://example.org/wp-content/uploads".to_vec())
    }
}

fuzz_target!(|input: &[u8]| {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let api = KsesApi::new(KsesDeps {
        hooks,
        escaper: Rc::new(Escaper),
        policy: Rc::new(Policy),
        globals: Rc::new(DefaultGlobals::default()),
        uploads: Rc::new(Uploads),
        callbacks: Rc::new(NoValueCallbacks),
    });
    let out = api
        .kses(input, &AllowedSpec::Context(Context::Post), None)
        .unwrap();
    assert!(
        out.len() <= 5 * input.len() + 16,
        "output {} for input {}",
        out.len(),
        input.len()
    );
    let allowed = api.allowed_html(&Context::Post).unwrap();
    let protocols = api.allowed_protocols().unwrap();
    if let Some(exploit) = speedyspec_wp_kses_security::first_exploit(&out, &allowed, &protocols) {
        panic!("exploit in kses output: {exploit}");
    }
});
