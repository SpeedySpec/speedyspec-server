//! `verify_file_signature()`'s rules.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha384};
use speedyspec_wp_crypto_contracts::{
    DetachedVerifier, SIGN_BYTES, SIGN_PUBLICKEY_BYTES, Translator,
};

use crate::php_base64_decode;

/// How a signature check ended (Special Case: each outcome carries what WordPress reports for it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureOutcome {
    /// A signature verified against a trusted key.
    Verified,
    /// No signature was given (`signature_verification_no_signature`).
    NoSignature,
    /// None verified (`signature_verification_failed`), with WordPress's debugging data.
    Failed {
        /// The file's sha384, lowercase hex.
        hash_hex: String,
        /// Keys of the wrong length, counted once per signature they were tried with.
        skipped_key: u64,
        /// Signatures of the wrong length.
        skipped_sig: u64,
    },
}

/// Checks signatures against keys as WordPress does, when both are already strings. See
/// [`check_signatures_with`].
pub fn check_signatures(
    verifier: &dyn DetachedVerifier,
    file_sha384: Option<&[u8; 48]>,
    signatures: Option<&[Vec<u8>]>,
    keys: &[Vec<u8>],
) -> SignatureOutcome {
    let text = |value: &Vec<u8>| Ok::<_, std::convert::Infallible>(value.clone());
    match check_signatures_with(verifier, file_sha384, signatures, keys, text, text) {
        Ok(outcome) => outcome,
        Err(never) => match never {},
    }
}

/// Checks signatures against keys as WordPress does.
///
/// `file_sha384` is `None` when the file can't be hashed: WordPress then carries `hash_file()`'s `false` on, so every
/// signature is checked against the empty message and the reported hash is empty. `signatures` is `None` when
/// WordPress's `! $signatures` holds (a falsy value), which ends the check before any key is looked at.
///
/// Each signature and key is read as base64 text (`signature_text`, `key_text`) only when the loop reaches it, as
/// WordPress calls `base64_decode()` inside its loop: a key is read for each signature of the right length it is
/// tried with, and nothing after the first match is read. A read that fails ends the check with its error, as PHP's
/// `TypeError` would. Text is decoded leniently, as PHP does; wrong-length signatures and keys are skipped and
/// counted.
pub fn check_signatures_with<S, K, E>(
    verifier: &dyn DetachedVerifier,
    file_sha384: Option<&[u8; 48]>,
    signatures: Option<&[S]>,
    keys: &[K],
    mut signature_text: impl FnMut(&S) -> Result<Vec<u8>, E>,
    mut key_text: impl FnMut(&K) -> Result<Vec<u8>, E>,
) -> Result<SignatureOutcome, E> {
    let Some(signatures) = signatures else {
        return Ok(SignatureOutcome::NoSignature);
    };
    let message: &[u8] = file_sha384.map_or(&[], |hash| hash);
    let mut skipped_key = 0;
    let mut skipped_sig = 0;
    for signature in signatures {
        let Ok(signature) =
            <[u8; SIGN_BYTES]>::try_from(php_base64_decode(&signature_text(signature)?))
        else {
            skipped_sig += 1;
            continue;
        };
        for key in keys {
            let Ok(key) =
                <[u8; SIGN_PUBLICKEY_BYTES]>::try_from(php_base64_decode(&key_text(key)?))
            else {
                skipped_key += 1;
                continue;
            };
            if verifier.verify(&signature, message, &key) {
                return Ok(SignatureOutcome::Verified);
            }
        }
    }
    Ok(SignatureOutcome::Failed {
        hash_hex: hex(message),
        skipped_key,
        skipped_sig,
    })
}

/// The file's sha384, or `None` where PHP's `hash_file()` fails (missing, unreadable, not a regular file).
pub fn sha384_file(path: &Path) -> Option<[u8; 48]> {
    let mut file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut hasher = Sha384::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hasher.finalize().into())
}

/// Lowercase hex, as `bin2hex()`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Which of `verify_file_signature()`'s error messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureMessage {
    /// `signature_verification_no_signature`'s.
    NoSignature,
    /// `signature_verification_failed`'s.
    Failed,
}

impl SignatureMessage {
    /// WordPress's English text, before translation.
    pub fn text(self) -> &'static str {
        match self {
            SignatureMessage::NoSignature => {
                "The authenticity of %s could not be verified as no signature was found."
            }
            SignatureMessage::Failed => "The authenticity of %s could not be verified.",
        }
    }
}

/// One error message, built as WordPress builds it, only when it returns that error:
/// `sprintf( __( … ), '<span class="code">' . esc_html( $name ) . '</span>' )`. PHP evaluates `__()` before the
/// escaping, so `escaped_name` runs after the translation. A failure of either (a throwing `gettext` or `esc_html`
/// filter) is the message's, and a failed translation leaves the name unescaped.
pub fn signature_message<E>(
    translator: &dyn Translator<Error = E>,
    which: SignatureMessage,
    escaped_name: impl FnOnce() -> Result<String, E>,
) -> Result<String, E> {
    let format = translator.translate(which.text())?;
    let name = format!("<span class=\"code\">{}</span>", escaped_name()?);
    Ok(sprintf_one(&format, &name))
}

/// `sprintf()` with one string argument: `%s` and `%1$s` take it, `%%` is a percent sign, anything else is kept.
fn sprintf_one(format: &str, argument: &str) -> String {
    let mut out = String::with_capacity(format.len() + argument.len());
    let mut rest = format;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix("%%") {
            out.push('%');
            rest = after;
        } else if let Some(after) = tail
            .strip_prefix("%1$s")
            .or_else(|| tail.strip_prefix("%s"))
        {
            out.push_str(argument);
            rest = after;
        } else {
            out.push('%');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}
