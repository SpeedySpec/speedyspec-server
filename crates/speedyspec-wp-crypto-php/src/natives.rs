//! The native functions behind the facade.

use std::os::unix::ffi::OsStrExt;
use std::rc::Rc;

use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_crypto::{
    SignatureOutcome, check_signatures, default_trusted_keys, messages, sha384_file, wp_basename,
    wp_fast_hash, wp_verify_fast_hash,
};
use speedyspec_wp_crypto_contracts::CryptoLibrary;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks::{HookApi, HookValue};
use speedyspec_wp_text_escape::EscapeApi;

use crate::adapters::{
    HookTrustedKeysFilter, PhpSiteSettings, PhpTranslator, QuietDiagnostics, SystemClock,
    php_string,
};

/// What the native functions share: libsodium, the filter and the escaping facade.
pub(crate) struct Natives {
    sodium: Sodium,
    keys: HookTrustedKeysFilter,
    escape: EscapeApi,
}

impl Natives {
    pub(crate) fn new(hooks: Rc<HookApi>, sodium: Sodium) -> Natives {
        Natives {
            sodium,
            keys: HookTrustedKeysFilter::new(hooks.clone()),
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
            Ok(c.keys.try_apply(default_trusted_keys(&SystemClock))?.raw)
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
        other => Err(NativeError::throw(
            "TypeError",
            format!(
                "SpeedySpec\\WpCrypto\\Native\\{function}(): Argument #{} must be of type string, {other:?} given",
                i + 1
            ),
        )),
    }
}

/// `(filename, signatures: string[]|null, filename_for_errors: ?string)`: `true`, or an error marker the facade turns
/// into a `WP_Error`. `signatures` is null when WordPress's `! $signatures` holds.
fn verify(natives: &Natives, args: &[HookValue]) -> Result<HookValue, NativeError> {
    let filename = bytes(args, 0, "verify_file_signature")?;
    let signatures = match args.get(1) {
        Some(HookValue::Array(items)) => Some(
            items
                .iter()
                .map(|(_, v)| match v {
                    HookValue::Str(s) => s.as_bytes().to_vec(),
                    HookValue::Bytes(b) => b.clone(),
                    _ => Vec::new(),
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    let friendly = match args.get(2) {
        Some(HookValue::Str(s)) => s.as_bytes().to_vec(),
        Some(HookValue::Bytes(b)) => b.clone(),
        _ => wp_basename(&filename),
    };
    let escaped = match natives
        .escape
        .esc_html(&friendly)
        .map_err(|e| NativeError::throw("Error", e.message))?
    {
        HookValue::Str(s) => s,
        HookValue::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
        other => strval(&other)?,
    };
    let text = messages(&PhpTranslator, &escaped);
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
    let Some(signatures) = signatures else {
        return Ok(marker(
            "signature_verification_no_signature",
            text.no_signature,
            Vec::new(),
        ));
    };
    let filtered = natives.keys.try_apply(default_trusted_keys(&SystemClock))?;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&filename));
    let hash = sha384_file(path);
    match check_signatures(
        &natives.sodium,
        hash.as_ref(),
        Some(&signatures),
        &filtered.keys,
    ) {
        SignatureOutcome::Verified => Ok(HookValue::Bool(true)),
        SignatureOutcome::NoSignature => Ok(marker(
            "signature_verification_no_signature",
            text.no_signature,
            Vec::new(),
        )),
        SignatureOutcome::Failed {
            hash_hex,
            skipped_key,
            skipped_sig,
        } => Ok(marker(
            "signature_verification_failed",
            text.failed,
            vec![
                ("keys", filtered.raw),
                ("hash", HookValue::Str(hash_hex)),
                ("skipped_key", HookValue::Int(skipped_key as i64)),
                ("skipped_sig", HookValue::Int(skipped_sig as i64)),
                ("sodium", HookValue::Str(natives.sodium.version())),
            ],
        )),
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
        Ok(HookValue::Str(s)) => Ok(s),
        Ok(other) => Ok(format!("{other:?}")),
        Err(failure) => Err(match failure.exception {
            Some(exception) => NativeError::Rethrow(exception),
            None => NativeError::Silent,
        }),
    }
}
