//! Envelope encryption for provider keys (SPEC §8, rule 2, D30).
//!
//! Construction — "envelope" because each secret gets its OWN one-time
//! data-encryption key (DEK), and only the DEK is encrypted under the
//! instance-wide master key:
//!
//! ```text
//! plaintext provider key ──XChaCha20-Poly1305(DEK, AD = "table:id")──▶ ciphertext
//! random 32-byte DEK     ──XChaCha20-Poly1305(master, AD = "table:id:dek")──▶ dek_wrapped
//! ```
//!
//! Both blobs are stored as `BASE64(24-byte nonce ‖ ciphertext+16-byte tag)`
//! TEXT in the provider_keys row. The master key comes from the environment
//! (`TRIBUTARY_MASTER_KEY`, 32 bytes hex) and NEVER touches the database —
//! stealing the DB yields only AEAD ciphertexts.
//!
//! Associated data (AD) binds every blob to its exact row: decrypting with a
//! different row's AD fails the Poly1305 tag, so ciphertext cannot be
//! copy-pasted between rows (e.g. swapping pool_id in a DB dump does not
//! rebind a key). The algorithm is XChaCha20-Poly1305-IETF (D30: RustCrypto
//! `chacha20poly1305`; byte-compatible with libsodium's
//! `crypto_aead_xchacha20poly1305_ietf`).

use base64::Engine;
use chacha20poly1305::Key;
use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use rand::RngCore;
use zeroize::Zeroizing;

/// XChaCha20 nonce size in bytes (192-bit — large nonces make random
/// generation collision-safe without any state).
const NONCE_LEN: usize = 24;
/// AEAD tag appended to every ciphertext (Poly1305).
const TAG_LEN: usize = 16;
/// Environment variable holding the hex-encoded 32-byte master key.
pub const MASTER_KEY_ENV: &str = "TRIBUTARY_MASTER_KEY";

/// The instance-wide key-encryption key (KEK).
///
/// Wrapped in `Zeroizing` so the raw bytes are scrubbed from memory the
/// moment the last clone is dropped (AGENTS secrets convention). `Debug`
/// prints a redaction marker so the key can never leak into logs.
#[derive(Clone)]
pub struct MasterKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for MasterKey {
    /// Manual Debug impl: struct-update-free redacted rendering. Derived
    /// Debug would print the raw key bytes — never acceptable here.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}

impl MasterKey {
    /// Loads the master key from `TRIBUTARY_MASTER_KEY` (64 hex chars).
    ///
    /// Fails loudly at startup when unset/malformed — a gateway that cannot
    /// decrypt its provider keys must not boot.
    pub fn from_env() -> anyhow::Result<Self> {
        let raw = std::env::var(MASTER_KEY_ENV).map_err(|_| {
            anyhow::anyhow!("required environment variable {MASTER_KEY_ENV} is not set")
        })?;
        Self::from_hex(&raw)
    }

    /// Parses a 64-char hex string into a master key.
    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(s.trim())
            .map_err(|e| anyhow::anyhow!("{MASTER_KEY_ENV} is not valid hex: {e}"))?;
        let arr: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| {
            anyhow::anyhow!(
                "{MASTER_KEY_ENV} must be 32 bytes (64 hex chars), got {} bytes",
                v.len()
            )
        })?;
        Ok(Self(Zeroizing::new(arr)))
    }

    /// Random master key — tests and `generate` tooling only, never startup.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Self(Zeroizing::new(bytes))
    }
}

/// Associated data binding a blob to its row: `"provider_keys:<id>"`.
///
/// The (table, id) pair is the row's identity; including it in the AEAD
/// input means moving the ciphertext to any other row fails decryption.
pub fn row_ad(table: &str, row_id: &str) -> Vec<u8> {
    format!("{table}:{row_id}").into_bytes()
}

/// A sealed secret: two independent base64(nonce‖ct‖tag) TEXT blobs.
///
/// Kept as plain `String`s (not `Zeroizing`) — base64 ciphertext is not
/// secret material; only the DEK and plaintext need zeroization.
#[derive(Debug, Clone)]
pub struct SealedEnvelope {
    /// The secret itself, encrypted under the per-row DEK.
    pub ciphertext: String,
    /// The per-row DEK, encrypted under the master key.
    pub dek_wrapped: String,
}

/// One AEAD operation: `base64(nonce ‖ encrypt(key, nonce, msg, aad))`.
fn seal_blob(key: &Key, msg: &[u8], aad: &[u8]) -> anyhow::Result<String> {
    let cipher = XChaCha20Poly1305::new(key);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes); // fresh random nonce per blob
    let nonce = XNonce::from_slice(&nonce_bytes);
    let sealed = cipher
        .encrypt(
            nonce,
            Payload {
                msg,
                aad, // authenticated, NOT encrypted — binds blob to its row
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    // Store nonce ‖ ciphertext ‖ tag — the nonce is not secret, but it must
    // travel with the ciphertext or decryption cannot rebuild the keystream.
    let mut blob = Vec::with_capacity(NONCE_LEN + sealed.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&sealed);
    Ok(base64::engine::general_purpose::STANDARD.encode(blob))
}

/// Decrypts one blob, returning the bare ciphertext ready for unpadding by
/// the caller (who knows the expected shape).
fn open_blob(key: &Key, blob: &str, aad: &[u8]) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    let sealed = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .map_err(|_| anyhow::anyhow!("sealed blob is not valid base64"))?;
    if sealed.len() < NONCE_LEN + TAG_LEN {
        anyhow::bail!("sealed blob too short to be a nonce + tag + ciphertext");
    }
    let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key);
    let plain = cipher
        .decrypt(XNonce::from_slice(nonce_bytes), Payload { msg: ct, aad })
        // One opaque error for every failure cause (wrong key, tampered
        // bytes, mismatched AD) — callers must not learn which part failed.
        .map_err(|_| {
            anyhow::anyhow!("decryption failed (wrong master key, tampered data, or row mismatch)")
        })?;
    Ok(Zeroizing::new(plain))
}

/// Seal `plaintext` into a [`SealedEnvelope` bound to `ad` (see [`row_ad`]).
pub fn seal(master: &MasterKey, plaintext: &[u8], ad: &[u8]) -> anyhow::Result<SealedEnvelope> {
    // Fresh DEK per call: compromising one row's ciphertext never affects
    // another row, and rotating the master key later only re-wraps DEKs.
    let dek = Zeroizing::new({
        let mut b = [0u8; 32];
        rand::rng().fill_bytes(&mut b);
        b
    });
    let ciphertext = seal_blob(Key::from_slice(dek.as_ref()), plaintext, ad)?;
    // The wrapped DEK gets its own AD suffix: swapping the two blob columns
    // inside a row, or moving a DEK to a different row, fails the tag.
    let mut dek_ad = Vec::with_capacity(ad.len() + 4);
    dek_ad.extend_from_slice(ad);
    dek_ad.extend_from_slice(b":dek");
    let dek_wrapped = seal_blob(Key::from_slice(master.0.as_ref()), dek.as_ref(), &dek_ad)?;
    Ok(SealedEnvelope {
        ciphertext,
        dek_wrapped,
    })
}

/// Open a [`SealedEnvelope`] bound to `ad`, returning zeroized plaintext.
///
/// Both AEAD layers must verify (row AD, then DEK AD) — a mismatch in either
/// is an error. Plaintext lives only in the returned `Zeroizing` wrapper.
pub fn open(
    master: &MasterKey,
    sealed: &SealedEnvelope,
    ad: &[u8],
) -> anyhow::Result<Zeroizing<Vec<u8>>> {
    let mut dek_ad = Vec::with_capacity(ad.len() + 4);
    dek_ad.extend_from_slice(ad);
    dek_ad.extend_from_slice(b":dek");
    let dek = open_blob(
        Key::from_slice(master.0.as_ref()),
        &sealed.dek_wrapped,
        &dek_ad,
    )?;
    let dek_arr: Zeroizing<[u8; 32]> = Zeroizing::new(dek.as_slice().try_into()?);
    open_blob(Key::from_slice(dek_arr.as_ref()), &sealed.ciphertext, ad)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in provider key shaped like the real thing.
    const PLAINTEXT: &[u8] = b"sk-provider-4f9d2c1aSUPERSECRET8b7e";

    fn seal_plain(master: &MasterKey, ad: &[u8]) -> SealedEnvelope {
        seal(master, PLAINTEXT, ad).expect("seal")
    }

    #[test]
    fn roundtrip_returns_original_plaintext() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "abc123");
        let sealed = seal_plain(&master, &ad);
        let opened = open(&master, &sealed, &ad).expect("open");
        assert_eq!(opened.as_slice(), PLAINTEXT);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "row-1");
        let sealed = seal_plain(&master, &ad);
        // Flip one bit inside the ciphertext portion (after the nonce).
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&sealed.ciphertext)
            .unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = SealedEnvelope {
            ciphertext: base64::engine::general_purpose::STANDARD.encode(bytes),
            dek_wrapped: sealed.dek_wrapped.clone(),
        };
        assert!(open(&master, &tampered, &ad).is_err());
    }

    #[test]
    fn tampered_wrapped_dek_fails() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "row-1");
        let sealed = seal_plain(&master, &ad);
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&sealed.dek_wrapped)
            .unwrap();
        bytes[NONCE_LEN + 2] ^= 0x80; // inside the wrapped-DEK ciphertext
        let tampered = SealedEnvelope {
            ciphertext: sealed.ciphertext.clone(),
            dek_wrapped: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        assert!(open(&master, &tampered, &ad).is_err());
    }

    #[test]
    fn wrong_associated_data_fails() {
        let master = MasterKey::generate();
        let sealed = seal_plain(&master, &row_ad("provider_keys", "row-1"));
        // Same table, different row id.
        assert!(open(&master, &sealed, &row_ad("provider_keys", "row-2")).is_err());
        // Different table, same id.
        assert!(open(&master, &sealed, &row_ad("pool_keys", "row-1")).is_err());
        // Empty AD.
        assert!(open(&master, &sealed, &[]).is_err());
    }

    #[test]
    fn wrong_master_key_fails() {
        let sealed = seal_plain(&MasterKey::generate(), &row_ad("provider_keys", "r"));
        assert!(
            open(
                &MasterKey::generate(),
                &sealed,
                &row_ad("provider_keys", "r")
            )
            .is_err()
        );
    }

    #[test]
    fn every_seal_uses_fresh_dek_and_nonces() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "same-row");
        let a = seal_plain(&master, &ad);
        let b = seal_plain(&master, &ad);
        assert_ne!(a.ciphertext, b.ciphertext, "nonce must differ");
        assert_ne!(a.dek_wrapped, b.dek_wrapped, "DEK must differ");
        // Both still decrypt.
        assert_eq!(open(&master, &a, &ad).unwrap().as_slice(), PLAINTEXT);
        assert_eq!(open(&master, &b, &ad).unwrap().as_slice(), PLAINTEXT);
    }

    #[test]
    fn debug_output_never_contains_secret_material() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "row-9");
        let sealed = seal_plain(&master, &ad);
        let opened = open(&master, &sealed, &ad).unwrap();

        // The automatable "plaintext is never logged" proof: every Debug
        // string a log line could plausibly render contains no plaintext and
        // no raw key bytes.
        let master_hex = hex::encode(master.0.as_ref());
        assert!(!format!("{master:?}").contains(&master_hex));
        assert!(!format!("{master:?}").contains("sk-provider"));
        assert!(!format!("{sealed:?}").contains("sk-provider"));
        assert!(!format!("{opened:?}").contains(&master_hex));
        // zeroize 1.9 redacts Zeroizing's own Debug ("Zeroizing { .. }"), so
        // even the opened plaintext container cannot leak via a stray
        // Debug-print. Asserted, not assumed:
        assert_eq!(format!("{opened:?}"), "Zeroizing { .. }");
        assert!(!format!("{opened:?}").contains("115"));
    }

    #[test]
    fn master_key_parsing_rejects_bad_input() {
        assert!(MasterKey::from_hex("zz").is_err(), "non-hex");
        assert!(MasterKey::from_hex(&"a".repeat(63)).is_err(), "too short");
        assert!(MasterKey::from_hex(&"a".repeat(65)).is_err(), "too long");
        let ok = MasterKey::from_hex(&"ab".repeat(32));
        assert!(ok.is_ok(), "32 bytes of hex is exactly right");
    }

    #[test]
    fn truncated_blob_fails_cleanly() {
        let master = MasterKey::generate();
        let ad = row_ad("provider_keys", "r");
        let sealed = SealedEnvelope {
            ciphertext: base64::engine::general_purpose::STANDARD.encode([0u8; 5]),
            dek_wrapped: sealed_ok(&master),
        };
        assert!(open(&master, &sealed, &ad).is_err());
    }

    /// Helper: a valid wrapped DEK (independent of the broken ciphertext).
    fn sealed_ok(master: &MasterKey) -> String {
        seal(master, b"x", &row_ad("provider_keys", "r"))
            .expect("seal")
            .dek_wrapped
    }
}
