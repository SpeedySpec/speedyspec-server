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

/// Checks signatures against keys as WordPress does.
///
/// `signatures` is `None` when WordPress's `! $signatures` holds (a falsy value), which ends the check before any key
/// is looked at. Signatures and keys are base64 text, decoded leniently as PHP does; wrong-length ones are skipped and
/// counted, and the first signature and key that verify end the check.
pub fn check_signatures(
    verifier: &dyn DetachedVerifier,
    file_sha384: &[u8; 48],
    signatures: Option<&[Vec<u8>]>,
    keys: &[Vec<u8>],
) -> SignatureOutcome {
    let Some(signatures) = signatures else {
        return SignatureOutcome::NoSignature;
    };
    let mut skipped_key = 0;
    let mut skipped_sig = 0;
    for signature in signatures {
        let Ok(signature) = <[u8; SIGN_BYTES]>::try_from(php_base64_decode(signature)) else {
            skipped_sig += 1;
            continue;
        };
        for key in keys {
            let Ok(key) = <[u8; SIGN_PUBLICKEY_BYTES]>::try_from(php_base64_decode(key)) else {
                skipped_key += 1;
                continue;
            };
            if verifier.verify(&signature, file_sha384, &key) {
                return SignatureOutcome::Verified;
            }
        }
    }
    SignatureOutcome::Failed {
        hash_hex: hex(file_sha384),
        skipped_key,
        skipped_sig,
    }
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

/// `verify_file_signature()`'s error messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureMessages {
    /// `signature_verification_no_signature`'s message.
    pub no_signature: String,
    /// `signature_verification_failed`'s message.
    pub failed: String,
}

/// Translates WordPress's messages and puts the file name in them, as `sprintf( __( … ), '<span class="code">' .
/// esc_html( $name ) . '</span>' )`. The name must already be escaped.
pub fn messages(translator: &dyn Translator, escaped_filename: &str) -> SignatureMessages {
    let name = format!("<span class=\"code\">{escaped_filename}</span>");
    let message = |text: &str| sprintf_one(&translator.translate(text), &name);
    SignatureMessages {
        no_signature: message(
            "The authenticity of %s could not be verified as no signature was found.",
        ),
        failed: message("The authenticity of %s could not be verified."),
    }
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
