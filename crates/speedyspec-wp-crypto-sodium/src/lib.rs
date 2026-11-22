//! The WordPress crypto ports over libsodium.
//!
//! [`Sodium`] calls libsodium directly from Rust, never through PHP's `sodium_*` functions. It links the static
//! PHP's own `libsodium.a` (`SODIUM_LIB_DIR`, set in `.cargo/config.toml`), so ext-sodium and the Rust port run the
//! same library at the version `scripts/build-php.sh` pins.

use std::ffi::CStr;

use libsodium_sys as ffi;
use speedyspec_wp_crypto_contracts::{
    Argon2, Argon2Variant, Base64UrlNoPad, CryptoLibrary, DetachedVerifier, GenericHash,
    RandomBytes, SIGN_BYTES, SIGN_PUBLICKEY_BYTES,
};

/// libsodium, initialized. Only [`Sodium::init`] makes one, so every call runs after `sodium_init()`. A copy is the
/// same proof, so one initialization serves every area of a worker.
#[derive(Clone, Copy, Debug)]
pub struct Sodium(());

impl Sodium {
    /// Initializes libsodium (`sodium_init()`, safe to call more than once).
    ///
    /// # Errors
    ///
    /// When libsodium can't initialize, which leaves its functions unsafe to use.
    pub fn init() -> Result<Sodium, String> {
        // SAFETY: sodium_init() takes no arguments and may be called repeatedly and from any thread.
        match unsafe { ffi::sodium_init() } {
            0 | 1 => Ok(Sodium(())),
            code => Err(format!(
                "libsodium failed to initialize (sodium_init() returned {code})"
            )),
        }
    }
}

impl GenericHash for Sodium {
    /// # Panics
    ///
    /// When `key` is longer than 64 bytes or `out_len` is outside 16..=64, libsodium's limits.
    fn hash(&self, message: &[u8], key: &[u8], out_len: usize) -> Vec<u8> {
        assert!(
            key.len() <= ffi::crypto_generichash_KEYBYTES_MAX as usize,
            "BLAKE2b keys are at most 64 bytes"
        );
        assert!(
            (ffi::crypto_generichash_BYTES_MIN as usize
                ..=ffi::crypto_generichash_BYTES_MAX as usize)
                .contains(&out_len),
            "BLAKE2b digests are 16 to 64 bytes"
        );
        let mut out = vec![0u8; out_len];
        // SAFETY: every pointer is valid for the length passed with it, and the lengths are within libsodium's limits.
        let status = unsafe {
            ffi::crypto_generichash(
                out.as_mut_ptr(),
                out_len,
                message.as_ptr(),
                message.len() as u64,
                if key.is_empty() {
                    std::ptr::null()
                } else {
                    key.as_ptr()
                },
                key.len(),
            )
        };
        assert_eq!(status, 0, "crypto_generichash() failed within its limits");
        out
    }
}

impl DetachedVerifier for Sodium {
    fn verify(
        &self,
        signature: &[u8; SIGN_BYTES],
        message: &[u8],
        public_key: &[u8; SIGN_PUBLICKEY_BYTES],
    ) -> bool {
        // SAFETY: the signature and key arrays have the sizes libsodium reads; the message pointer is valid for its
        // length.
        unsafe {
            ffi::crypto_sign_verify_detached(
                signature.as_ptr(),
                message.as_ptr(),
                message.len() as u64,
                public_key.as_ptr(),
            ) == 0
        }
    }
}

impl Base64UrlNoPad for Sodium {
    fn encode(&self, bytes: &[u8]) -> String {
        // Unpadded base64 needs at most 4 characters per 3 bytes, plus libsodium's terminating NUL.
        let mut out = vec![0u8; bytes.len().div_ceil(3) * 4 + 1];
        // SAFETY: `out` holds the encoding and its NUL; the input pointer is valid for its length.
        unsafe {
            ffi::sodium_bin2base64(
                out.as_mut_ptr().cast(),
                out.len(),
                bytes.as_ptr(),
                bytes.len(),
                ffi::sodium_base64_VARIANT_URLSAFE_NO_PADDING as i32,
            );
        }
        let end = out.iter().position(|b| *b == 0).unwrap_or(out.len());
        out.truncate(end);
        String::from_utf8(out).expect("base64 is ASCII")
    }
}

impl CryptoLibrary for Sodium {
    fn version(&self) -> String {
        // SAFETY: sodium_version_string() returns a static NUL-terminated string.
        unsafe { CStr::from_ptr(ffi::sodium_version_string()) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Argon2 for Sodium {
    fn derive(
        &self,
        variant: Argon2Variant,
        password: &[u8],
        salt: &[u8; 16],
        time_cost: u32,
        memory_kib: u32,
        out_len: usize,
    ) -> Result<Vec<u8>, String> {
        let alg = match variant {
            Argon2Variant::Argon2i => ffi::crypto_pwhash_ALG_ARGON2I13,
            Argon2Variant::Argon2id => ffi::crypto_pwhash_ALG_ARGON2ID13,
        };
        let mut out = vec![0u8; out_len];
        // SAFETY: `out` is valid for `out_len` bytes, the password pointer for its length, and the salt is the
        // 16 bytes (crypto_pwhash_SALTBYTES) libsodium reads.
        let status = unsafe {
            ffi::crypto_pwhash(
                out.as_mut_ptr(),
                out_len as u64,
                password.as_ptr().cast(),
                password.len() as u64,
                salt.as_ptr(),
                u64::from(time_cost),
                memory_kib as usize * 1024,
                alg as i32,
            )
        };
        if status == 0 {
            Ok(out)
        } else {
            Err(format!(
                "argon2 can't run with time cost {time_cost} and memory {memory_kib} KiB"
            ))
        }
    }
}

impl RandomBytes for Sodium {
    fn fill(&self, out: &mut [u8]) {
        // SAFETY: `out` is valid for its length.
        unsafe { ffi::randombytes_buf(out.as_mut_ptr().cast(), out.len()) }
    }
}
