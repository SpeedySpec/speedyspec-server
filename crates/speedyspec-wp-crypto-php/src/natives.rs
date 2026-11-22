//! The native functions behind the facade.

use std::rc::Rc;

use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_crypto::{
    SignatureMessage, SignatureOutcome, check_signatures_with, default_trusted_keys,
    signature_message, wp_basename, wp_fast_hash, wp_verify_fast_hash,
};
use speedyspec_wp_crypto_contracts::{CryptoLibrary, FileHasher, TrustedKeysFilter};
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::{HookApi, HookValue};
use speedyspec_wp_text_escape::EscapeApi;

use crate::adapters::{
    HookTrustedKeysFilter, PhpFileHasher, PhpSiteSettings, PhpTranslator, QuietDiagnostics,
    SystemClock, base64_text, elements, php_string,
};

/// What the native functions share: libsodium, the filter, the file hasher and the escaping facade.
pub(crate) struct Natives {
    sodium: Sodium,
    keys: HookTrustedKeysFilter,
    files: PhpFileHasher,
    escape: EscapeApi,
}

impl Natives {
    pub(crate) fn new(hooks: Rc<HookApi>, sodium: Sodium) -> Natives {
        Natives {
            sodium,
            keys: HookTrustedKeysFilter::new(hooks.clone()),
            files: PhpFileHasher,
            escape: EscapeApi::new(hooks, Rc::new(PhpSiteSettings), Rc::new(QuietDiagnostics)),
        }
    }
}

pub(crate) fn functions(natives: &Rc<Natives>) -> Vec<FunctionBuilder<'static>> {
    let (a, b, c, d) = (
        natives.clone(),
        natives.clone(),
        natives.clone(),
        natives.clone(),
    );
    vec![
        native::function("SpeedySpec\\WpCrypto\\Native\\wp_fast_hash", move |args| {
            let message = bytes(args, 0, "wp_fast_hash")?;
            Ok(HookValue::Str(wp_fast_hash(&a.sodium, &a.sodium, &message)))
        }),
        native::function(
            "SpeedySpec\\WpCrypto\\Native\\wp_verify_fast_hash",
            move |args| {
                let message = bytes(args, 0, "wp_verify_fast_hash")?;
                let hash = bytes(args, 1, "wp_verify_fast_hash")?;
                Ok(HookValue::Bool(wp_verify_fast_hash(
                    &b.sodium, &b.sodium, &message, &hash,
                )))
            },
        ),
        native::function("SpeedySpec\\WpCrypto\\Native\\wp_trusted_keys", move |_| {
            c.keys.apply(default_trusted_keys(&SystemClock))
        }),
        native::function(
            "SpeedySpec\\WpCrypto\\Native\\verify_file_signature",
            move |args| verify(&d, args),
        ),
    ]
}

fn bytes(args: &[HookValue], i: usize, function: &str) -> Result<Vec<u8>, NativeError> {
    match args.get(i) {
        Some(HookValue::Str(s)) => Ok(s.as_bytes().to_vec()),
        Some(HookValue::Bytes(b)) => Ok(b.clone()),
        Some(other) => Err(NativeError::throw(
            "TypeError",
            format!(
                "SpeedySpec\\WpCrypto\\Native\\{function}(): Argument #{} must be of type string, {} given",
                i + 1,
                debug_type(other)
            ),
        )),
        None => Err(NativeError::throw(
            "ArgumentCountError",
            format!(
                "SpeedySpec\\WpCrypto\\Native\\{function}() expects at least {} argument{}, {} given",
                i + 1,
                if i == 0 { "" } else { "s" },
                args.len()
            ),
        )),
    }
}

/// PHP's `get_debug_type( $value )`, as PHP names a type in a `TypeError`.
fn debug_type(value: &HookValue) -> String {
    speedyspec_php::call_catching(
        &HookValue::Str("get_debug_type".into()),
        std::slice::from_ref(value),
    )
    .ok()
    .and_then(|name| text(&name))
    .unwrap_or_else(|| "mixed".to_string())
}

/// A PHP string's text; `None` for any other value.
fn text(value: &HookValue) -> Option<String> {
    match value {
        HookValue::Str(s) => Some(s.clone()),
        HookValue::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

/// `(filename, signatures: array|null, filename_for_errors: ?string)`: `true`, or an error marker the facade turns into
/// a `WP_Error`. `signatures` is `(array) $signatures`, or null when WordPress's `! $signatures` holds.
///
/// The steps run in WordPress's order, firing what WordPress fires and nothing else: the `wp_trusted_keys` filter only
/// when there are signatures, each signature and key read only when the loop reaches it, and `__()` and `esc_html`
/// only for the one message of the error returned.
fn verify(natives: &Natives, args: &[HookValue]) -> Result<HookValue, NativeError> {
    let filename = bytes(args, 0, "verify_file_signature")?;
    let signatures = match args.get(1) {
        Some(HookValue::Array(items)) => {
            Some(items.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>())
        }
        _ => None,
    };
    let friendly = match args.get(2) {
        Some(HookValue::Str(s)) => s.as_bytes().to_vec(),
        Some(HookValue::Bytes(b)) => b.clone(),
        _ => wp_basename(&filename),
    };
    let message = |which| signature_message(&PhpTranslator, which, || escape(natives, &friendly));
    let marker = |code: &str, message: String, more: Vec<(&str, HookValue)>| {
        let mut items = vec![
            (key("__speedyspec_error"), HookValue::Bool(true)),
            (key("code"), HookValue::Str(code.into())),
            (key("message"), HookValue::Str(message)),
            (key("filename"), php_string(&friendly)),
        ];
        items.extend(more.into_iter().map(|(k, v)| (key(k), v)));
        HookValue::Array(items)
    };
    let no_signature = || -> Result<HookValue, NativeError> {
        Ok(marker(
            "signature_verification_no_signature",
            message(SignatureMessage::NoSignature)?,
            Vec::new(),
        ))
    };
    let Some(signatures) = signatures else {
        return no_signature();
    };
    let raw = natives.keys.apply(default_trusted_keys(&SystemClock))?;
    let keys = elements(&raw.read());
    let hash = natives.files.sha384(&filename)?;
    match check_signatures_with(
        &natives.sodium,
        hash.as_ref(),
        Some(&signatures),
        &keys,
        base64_text,
        base64_text,
    )? {
        SignatureOutcome::Verified => Ok(HookValue::Bool(true)),
        SignatureOutcome::NoSignature => no_signature(),
        SignatureOutcome::Failed {
            hash_hex,
            skipped_key,
            skipped_sig,
        } => Ok(marker(
            "signature_verification_failed",
            message(SignatureMessage::Failed)?,
            vec![
                ("keys", raw),
                ("hash", HookValue::Str(hash_hex)),
                ("skipped_key", HookValue::Int(skipped_key as i64)),
                ("skipped_sig", HookValue::Int(skipped_sig as i64)),
                ("sodium", HookValue::Str(natives.sodium.version())),
            ],
        )),
    }
}

/// `esc_html( $name )` as the string `sprintf()` receives.
fn escape(natives: &Natives, name: &[u8]) -> Result<String, NativeError> {
    let escaped = natives
        .escape
        .esc_html(name)
        .map_err(|e| NativeError::throw("Error", e.message))?;
    match text(&escaped) {
        Some(s) => Ok(s),
        None => strval(&escaped),
    }
}

fn key(name: &str) -> speedyspec_hooks::Key {
    speedyspec_hooks::Key::Str(name.into())
}

/// PHP's own string conversion, for a filtered `esc_html` result that isn't a string.
fn strval(value: &HookValue) -> Result<String, NativeError> {
    match speedyspec_php::call_catching(
        &HookValue::Str("strval".into()),
        std::slice::from_ref(value),
    ) {
        // strval() returns a string or throws.
        Ok(converted) => Ok(text(&converted).unwrap_or_default()),
        Err(failure) => Err(match failure.exception {
            Some(exception) => NativeError::Rethrow(exception),
            None => NativeError::Silent,
        }),
    }
}
