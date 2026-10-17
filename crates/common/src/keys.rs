//! Pool-key material: generation, hashing, and constant-time comparison.
//!
//! Raw keys look like `sk-pool-<64 hex>` (256 bits of entropy). Only the
//! SHA-256 hex digest is ever stored or queried; lookups go through the
//! unique `key_hash` index and the stored digest is additionally verified
//! with a constant-time comparison (defense in depth).
//!
//! Moved to `common` in M2: the control plane issues pool keys and the
//! gateway authenticates them — one implementation, no drift.

use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Public prefix of every pool key (members identify keys by it in UX).
pub const KEY_PREFIX: &str = "sk-pool-";
/// Minimum length of the random part (hex chars) we are willing to consider.
const MIN_BODY_LEN: usize = 16;

/// A freshly generated pool key. `raw` exists exactly once, at creation time.
pub struct GeneratedKey {
    pub raw: String,
    pub key_hash: String,
    /// Display form: `sk-pool-<first5>…<last4>` (secrets convention).
    pub key_prefix: String,
}

/// Generates a new 256-bit pool key with its storage hash and display form.
pub fn generate_key() -> GeneratedKey {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let body = hex::encode(bytes);
    let raw = format!("{KEY_PREFIX}{body}");
    let key_hash = hash_token(&raw);
    let key_prefix = format!("{KEY_PREFIX}{}…{}", &body[..5], &body[body.len() - 4..]);
    GeneratedKey {
        raw,
        key_hash,
        key_prefix,
    }
}

/// SHA-256 of the raw token, hex-encoded (what gets stored/queried).
pub fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Constant-time equality of two hex digests (thwarts timing oracles).
pub fn constant_time_hash_eq(stored_hex: &str, computed_hex: &str) -> bool {
    match (hex::decode(stored_hex), hex::decode(computed_hex)) {
        (Ok(a), Ok(b)) => bool::from(a.ct_eq(&b)),
        _ => false,
    }
}

/// True when a presented bearer token even looks like a pool key body.
///
/// Cheap shape check before any hashing/DB work — malformed tokens are
/// rejected without touching the database.
pub fn is_plausible_pool_key(raw: &str) -> bool {
    let body_len = raw.len().saturating_sub(KEY_PREFIX.len());
    raw.starts_with(KEY_PREFIX) && body_len >= MIN_BODY_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_have_expected_shape() {
        let key = generate_key();
        assert!(key.raw.starts_with("sk-pool-"));
        assert_eq!(key.raw.len(), KEY_PREFIX.len() + 64);
        assert_eq!(key.key_hash.len(), 64);
        // Display form reveals only 9 characters of key material.
        assert!(key.key_prefix.starts_with("sk-pool-") && key.key_prefix.contains('…'));
        let visible: usize = key
            .key_prefix
            .trim_start_matches(KEY_PREFIX)
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .count();
        assert_eq!(visible, 9);
    }

    #[test]
    fn hashing_is_deterministic_and_keyed_by_full_token() {
        let a = hash_token("sk-pool-abc");
        let b = hash_token("sk-pool-abc");
        let c = hash_token("sk-pool-abd");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Hash never contains the raw token.
        assert!(!a.contains("sk-pool"));
    }

    #[test]
    fn constant_time_compare_matches_equality() {
        let a = hash_token("x");
        let b = hash_token("x");
        let c = hash_token("y");
        assert!(constant_time_hash_eq(&a, &b));
        assert!(!constant_time_hash_eq(&a, &c));
        assert!(!constant_time_hash_eq(&a, "not-hex"));
    }

    #[test]
    fn plausibility_check_rejects_short_or_wrong_prefix() {
        assert!(is_plausible_pool_key(&generate_key().raw));
        assert!(!is_plausible_pool_key("sk-pool-short"));
        assert!(!is_plausible_pool_key("Bearer sk-pool-abcdef0123456789"));
        assert!(!is_plausible_pool_key(""));
    }
}
