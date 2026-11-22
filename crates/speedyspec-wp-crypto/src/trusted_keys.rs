//! `wp_trusted_keys()` before its filter.

use speedyspec_wp_crypto_contracts::Clock;

/// WordPress.org key #1, base64.
pub const WPORG_KEY_1: &str = "fRPyrxb/MvVLbdsYi+OOEv4xc+Eqpsj+kkAS6gNOkI0=";

/// Key #1 is trusted only before this time (2021-04-01 00:00:00 UTC).
pub const WPORG_KEY_1_UNTIL: i64 = 1_617_235_200;

/// The keys WordPress trusts before the `wp_trusted_keys` filter runs, base64.
pub fn default_trusted_keys(clock: &dyn Clock) -> Vec<Vec<u8>> {
    if clock.now() < WPORG_KEY_1_UNTIL {
        vec![WPORG_KEY_1.as_bytes().to_vec()]
    } else {
        Vec::new()
    }
}
