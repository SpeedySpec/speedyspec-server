//! WordPress's sodium functions as native PHP functions, over the Rust rules and libsodium (Facade, Adapter).
//!
//! [`CryptoArea`] declares `wp_fast_hash()`, `wp_verify_fast_hash()`, `wp_trusted_keys()` and
//! `verify_file_signature()` at the start of every request (from `crypto.php`), each inside a `function_exists` guard.
//! Each casts its arguments as WordPress's would and calls the native function of the same name in
//! `SpeedySpec\WpCrypto\Native`, which runs WordPress's rules (`speedyspec-wp-crypto`) over libsodium
//! (`speedyspec-wp-crypto-sodium`). Nothing goes through PHP's `sodium_*` functions.
//!
//! What WordPress does through other WordPress code still happens: the `wp_trusted_keys` filter runs through the Hook
//! API ([`HookTrustedKeysFilter`]), the file is hashed by PHP's own `hash_file()` ([`PhpFileHasher`]), error messages through `__()` ([`PhpTranslator`]) and the `esc_html` port, and the
//! facade returns a real `WP_Error`, so `wp_error_added` fires.
//!
//! Two answers differ from WordPress on purpose: a phpass hash never verifies (SpeedySpec carries no phpass), and
//! `signature_verification_unsupported` is never returned (the runtime always has ext-sodium and sha384).

mod adapters;
mod natives;
mod release;

use std::rc::Rc;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::WpHookApi;

pub use adapters::{
    HookTrustedKeysFilter, PhpFileHasher, PhpSiteSettings, PhpTranslator, QuietDiagnostics,
    SystemClock,
};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

const FACADE: &str = include_str!("crypto.php");

/// The native area of WordPress's sodium functions.
pub struct CryptoArea {
    natives: Rc<natives::Natives>,
}

impl CryptoArea {
    /// The area, hashing and verifying with `sodium` and filtering and escaping through `hooks`. The caller
    /// initializes libsodium ([`Sodium::init`]) and hands it over, so a process initializes it once.
    pub fn new(hooks: Rc<WpHookApi>, sodium: Sodium) -> CryptoArea {
        CryptoArea {
            natives: Rc::new(natives::Natives::new(hooks, sodium)),
        }
    }
}

impl NativeArea for CryptoArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        natives::functions(&self.natives)
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-crypto", FACADE) {
            eprintln!(
                "speedyspec_wordpress: installing WordPress's sodium functions failed: {err}"
            );
        }
    }
}
