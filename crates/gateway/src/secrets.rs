//! Virtual key generation and hashing, and encryption of provider credentials.

use std::fmt;

use anyhow::{anyhow, bail, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const KEY_PREFIX: &str = "uf-sk-";
pub const TOKEN_PREFIX: &str = "uf-at-";
pub const INVITE_PREFIX: &str = "uf-inv-";
const NONCE_LEN: usize = 12;

pub struct NewKey {
    /// Shown to the user once. Never stored.
    pub full: String,
    pub hash: String,
    /// Safe to store and show, for example `uf-sk-…7d2f`.
    pub display: String,
}

/// Redacts the full key, so logging a `NewKey` cannot leak it.
impl fmt::Debug for NewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewKey")
            .field("full", &"<redacted>")
            .field("hash", &self.hash)
            .field("display", &self.display)
            .finish()
    }
}

pub fn generate_key() -> NewKey {
    generate_secret(KEY_PREFIX)
}

/// A secret with the given prefix and 32 random bytes as hex.
pub fn generate_secret(prefix: &str) -> NewKey {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let full = format!("{prefix}{}", hex::encode(bytes));
    let display = format!("{prefix}\u{2026}{}", &full[full.len() - 4..]);
    NewKey {
        hash: hash_key(&full),
        full,
        display,
    }
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Whether two secrets are equal. It compares their SHA-256 digests, so how
/// long the comparison takes says nothing about the secrets themselves.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    hash_key(a) == hash_key(b)
}

pub struct Cipher(ChaCha20Poly1305);

/// Never prints key material.
impl fmt::Debug for Cipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cipher(<redacted>)")
    }
}

impl Cipher {
    pub fn generate_master_hex() -> String {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    pub fn from_hex(master: &str) -> Result<Self> {
        let bytes = hex::decode(master.trim()).map_err(|_| anyhow!("master key is not hex"))?;
        if bytes.len() != 32 {
            bail!("master key must be 32 bytes (64 hex characters)");
        }
        if bytes.iter().all(|b| *b == 0) {
            bail!("master key must not be all zeros");
        }
        Ok(Self(ChaCha20Poly1305::new(Key::from_slice(&bytes))))
    }

    /// Output is the 12-byte nonce followed by the ciphertext.
    pub fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ct = self
            .0
            .encrypt(Nonce::from_slice(&nonce), plain)
            .expect("encryption with a valid key and nonce cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ct);
        out
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() <= NONCE_LEN {
            bail!("encrypted value is too short");
        }
        let (nonce, ct) = data.split_at(NONCE_LEN);
        self.0
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| anyhow!("could not decrypt: wrong master key or corrupted value"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_unique_prefixed_and_hash_consistently() {
        let a = generate_key();
        let b = generate_key();
        assert_ne!(a.full, b.full);
        assert!(a.full.starts_with(KEY_PREFIX));
        assert_eq!(a.full.len(), KEY_PREFIX.len() + 64);
        assert_eq!(a.hash, hash_key(&a.full));
        assert_eq!(a.hash.len(), 64);
        assert!(a.display.ends_with(&a.full[a.full.len() - 4..]));
        assert!(!a
            .display
            .contains(&a.full[KEY_PREFIX.len()..a.full.len() - 4]));
    }

    #[test]
    fn generate_secret_uses_prefix() {
        for prefix in [TOKEN_PREFIX, INVITE_PREFIX, KEY_PREFIX] {
            let a = generate_secret(prefix);
            let b = generate_secret(prefix);
            assert_ne!(a.full, b.full);
            assert!(a.full.starts_with(prefix));
            assert_eq!(a.full.len(), prefix.len() + 64);
            assert_eq!(a.hash, hash_key(&a.full));
            assert!(a.display.starts_with(prefix));
            assert!(a.display.ends_with(&a.full[a.full.len() - 4..]));
            assert!(!a.display.contains(&a.full[prefix.len()..a.full.len() - 4]));
            let shown = format!("{a:?}");
            assert!(!shown.contains(&a.full));
            assert!(!shown.contains(&a.full[prefix.len()..]));
        }
        assert_eq!(TOKEN_PREFIX, "uf-at-");
        assert_eq!(INVITE_PREFIX, "uf-inv-");
        assert_eq!(generate_secret(TOKEN_PREFIX).full.len(), 6 + 64);
    }

    #[test]
    fn secrets_are_compared_by_value() {
        let a = generate_key().full;
        assert!(secrets_equal(&a, &a.clone()));
        assert!(secrets_equal("", ""));
        assert!(!secrets_equal(&a, &generate_key().full));
        assert!(!secrets_equal(&a, &a[..a.len() - 1]));
        assert!(!secrets_equal(&a, &a.to_uppercase()));
        assert!(!secrets_equal(&a, ""));
        assert!(!secrets_equal("", &a));
    }

    #[test]
    fn cipher_round_trips_and_uses_fresh_nonces() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let one = c.encrypt(b"sk-provider");
        let two = c.encrypt(b"sk-provider");
        assert_ne!(one, two);
        assert_eq!(c.decrypt(&one).unwrap(), b"sk-provider");
    }

    #[test]
    fn cipher_rejects_tampering_wrong_key_and_bad_input() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let mut data = c.encrypt(b"secret");
        let last = data.len() - 1;
        data[last] ^= 1;
        assert!(c.decrypt(&data).is_err());
        let other = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        assert!(other.decrypt(&c.encrypt(b"secret")).is_err());
        assert!(c.decrypt(b"short").is_err());
        assert!(Cipher::from_hex("abcd").is_err());
        assert!(Cipher::from_hex("not hex").is_err());
    }

    #[test]
    fn cipher_rejects_an_all_zero_master_key() {
        let err = Cipher::from_hex(&"00".repeat(32)).expect_err("must be rejected");
        assert!(err.to_string().contains("zero"));
    }

    #[test]
    fn cipher_errors_do_not_echo_the_master_key() {
        let bad = "zz".repeat(32);
        let err = Cipher::from_hex(&bad).expect_err("must be rejected");
        assert!(!format!("{err:?}").contains(&bad));
    }

    #[test]
    fn debug_output_does_not_expose_secrets() {
        let master = Cipher::generate_master_hex();
        let c = Cipher::from_hex(&master).unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains(&master));
        assert_eq!(shown, "Cipher(<redacted>)");

        let k = generate_key();
        let shown = format!("{k:?}");
        assert!(!shown.contains(&k.full));
        assert!(!shown.contains(&k.full[KEY_PREFIX.len()..]));
        assert!(shown.contains(&k.display));
    }
}
